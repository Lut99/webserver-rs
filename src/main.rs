//  MAIN.rs
//    by Lut99
//
//  Created:
//    17 Jul 2024, 18:54:35
//  Last edited:
//    13 Jan 2025, 22:36:56
//  Auto updated?
//    Yes
//
//  Description:
//!   Entrypoint to the `static-website-host` binary.
//

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::Request;
use axum::extract::connect_info::IntoMakeServiceWithConnectInfo;
use axum::routing::get;
use clap::Parser;
use error_trace::trace;
use humanlog::{DebugMode, HumanLogger};
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as HyperBuilder;
use log::{debug, error, info, warn};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::{Builder, Runtime};
use tokio::signal::unix::{SignalKind, signal};
use tower_service::Service as _;
use webserver::state::{AuthMode, Context};
use webserver::{auth, www};


/***** CONSTANTS *****/
/// The number of seconds we gracefully shutdown.
const SHUTDOWN_TIMEOUT_S: u64 = 10 * 60;





/***** ARGUMENTS *****/
/// Defines the toplevel arguments for the `de-hoek-studio` binary.
#[derive(Debug, Parser)]
struct Arguments {
    /// If given, enables TRACE-level log statements in addition to the normal ones.
    #[clap(long, global = true, help = "If given, enables TRACE-level log statements. Also provides further details for other log levels.")]
    trace: bool,

    /// The address on which the server binds itself.
    #[clap(short, long, default_value = "127.0.0.1:42080")]
    address:     SocketAddr,
    /// The location to the server configuration.
    #[clap(
        short,
        long = "config",
        default_value = "./config.yml",
        help = "The location to the configuration file that describes the server's behaviour. Will generate a default one if omitted."
    )]
    config_path: PathBuf,
}





/***** ENTRYPOINT *****/
fn main() {
    // Parse the arguments
    let args = Arguments::parse();

    // Setup the logger
    if let Err(err) = HumanLogger::terminal(if args.trace { DebugMode::Full } else { DebugMode::Debug }).init() {
        eprintln!("WARNING: Failed to setup logger: {err} (no logging for this session)");
    }
    info!("{} v{}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"));

    // Create the tokio runtime
    debug!("Creating tokio runtime...");
    let runtime: Runtime = match Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(err) => {
            error!("{}", trace!(("Failed to create tokio runtime"), err));
            std::process::exit(1);
        },
    };

    // Initialize the state
    let state: Arc<Context> = match Context::new(env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"), &args.config_path) {
        Ok(state) => Arc::new(state),
        Err(err) => {
            error!("{}", trace!(("Failed to initialize server context"), err));
            std::process::exit(1);
        },
    };

    // Build the paths
    let www = Router::new().route("/", get(www::handle)).route("/{*path}", get(www::handle));
    let www = match &state.auth {
        AuthMode::Basic { username, password: _ } => {
            info!("Enabling Basic authentication with username {username:?}");
            www.layer(axum::middleware::from_fn_with_state(state.clone(), auth::basic))
        },
        AuthMode::None => www,
    };
    let www = www.with_state(state.clone());
    let router: IntoMakeServiceWithConnectInfo<Router, SocketAddr> = Router::new().merge(www).into_make_service_with_connect_info();

    // Run the main async function
    runtime.block_on(async move {
        // Bind the TCP Listener
        debug!("Binding server on '{}'...", args.address);
        let listener: TcpListener = match TcpListener::bind(args.address).await {
            Ok(listener) => listener,
            Err(err) => {
                error!("{}", trace!(("Failed to bind server to '{}'", args.address), err));
                std::process::exit(1);
            },
        };

        // Accept new connections!
        info!("Initialization OK, awaiting connections...");
        tokio::select! {
            _ = async move {
                loop {
                    // Accept a new connection
                    let (socket, remote_addr): (TcpStream, SocketAddr) = match listener.accept().await {
                        Ok(res) => res,
                        Err(err) => {
                            error!("{}", trace!(("Failed to accept incoming connection"), err));
                            std::process::exit(1);
                        },
                    };

                    // Move the rest to a separate task
                    let router: IntoMakeServiceWithConnectInfo<_, _> = router.clone();
                    tokio::spawn(async move {
                        debug!("Handling incoming connection from '{remote_addr}'");

                        // Build  the service
                        let service = hyper::service::service_fn(|request: Request<Incoming>| {
                            // Sadly, we must `move` again because this service could be called multiple times (at least according to the typesystem)
                            let mut router = router.clone();
                            async move {
                                // SAFETY: We can call `unwrap()` because the call returns an infallible.
                                router.call(remote_addr).await.unwrap().call(request).await
                            }
                        });

                        // Create a service that handles this for us
                        let socket: TokioIo<_> = TokioIo::new(socket);
                        if let Err(err) = HyperBuilder::new(TokioExecutor::new()).serve_connection_with_upgrades(socket, service).await {
                            error!("{}", trace!(("Failed to serve incoming connection"), *err));
                        }
                    });
                }
            } => {
                unreachable!();
            },

            _ = async move {
                match signal(SignalKind::interrupt()) {
                    Ok(mut sign) => sign.recv().await,
                    Err(err) => {
                        warn!("{}", trace!(("Failed to register SIGINT signal handler"), err));
                        warn!("Graceful shutdown by Ctrl+C disabled");
                        None
                    },
                }
            } => {
                debug!("Received SIGINT");
            },
            _ = async move {
                match signal(SignalKind::terminate()) {
                    Ok(mut sign) => sign.recv().await,
                    Err(err) => {
                        warn!("{}", trace!(("Failed to register SIGTERM signal handler"), err));
                        warn!("Graceful shutdown by Docker disabled");
                        None
                    },
                }
            } => {
                debug!("Received SIGTERM");
            },
        }
    });

    // When the server stops, quit the runtime too
    info!("Terminating tokio runtime ({SHUTDOWN_TIMEOUT_S}s timeout)...");
    runtime.shutdown_timeout(Duration::from_secs(SHUTDOWN_TIMEOUT_S));
    info!("Done.");
}
