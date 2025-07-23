//  SERVER.rs
//    by Lut99
//
//  Description:
//!   Defines the main [`Server`] interface.
//

use std::fs::File;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::extract::connect_info::IntoMakeServiceWithConnectInfo;
use axum::routing::get;
use error_trace::toplevel;
use hyper::body::Incoming;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as HyperBuilder;
use log::{debug, error, info, warn};
use serde::Deserialize;
use thiserror::Error;
use tokio::net::{TcpListener, TcpStream};
use tokio::signal::unix::{SignalKind, signal};
use tower_service::Service as _;

use crate::context::Context;
use crate::www;


/***** ERRORS *****/
/// Defines errors originating from the [`Server`] itself.
#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to open the given config file {path:?}")]
    ConfigOpen { path: PathBuf, source: std::io::Error },
    #[error("Failed to read or parse the given config file {path:?}")]
    ConfigLoad { path: PathBuf, source: serde_yml::Error },
}





/***** LIBRARY *****/
/// Implements the server as a whole.
///
/// This is the main object you will be working with. By default, it implements a simple webserver
/// for hosting files only. However, you can configure it yourself by adding additional endpoints
/// behind an API that can be called by your website.
///
/// # Generics
/// - `C`: The custom context that is added to this server. Use `()` to encode no custom context is
///   necessary.
pub struct Server<C = ()> {
    /// The router we building.
    router:  Router<Arc<Context<C>>>,
    /// The state struct that is used to be consistent across calls.
    context: Context<C>,
}

// Constructors
impl<C: 'static + Default + Send + Sync> Server<C> {
    /// Constructor for the Server that initializes it with a default custom context.
    ///
    /// # Arguments
    /// - `name`: The name for this server. If you need inspiration, use
    ///   `format!("{}/{}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"))` instead.
    /// - `addr`: Some [`SocketAddr`] to bind the server on.
    /// - `site`: The path to the folder that we'll serve website files from.
    /// - `not_found_file`: The path to a special file that we serve when the user when they try to
    ///   navigate to a non-existing file.
    ///
    /// # Returns
    /// A new Server with the given properties, plus other sensible defaults (including your own
    /// custom `C`ontext).
    #[inline]
    pub fn new_with_default_custom(
        name: impl Into<String>,
        addr: impl Into<SocketAddr>,
        site: impl Into<PathBuf>,
        not_found_file: impl Into<PathBuf>,
    ) -> Self {
        Self {
            router:  Router::new(),
            context: Context {
                name: name.into(),
                addr: addr.into(),
                site: site.into(),
                not_found_file: not_found_file.into(),
                header_settings: Default::default(),
                custom: Default::default(),
            },
        }
    }
}
impl<C: 'static + for<'de> Deserialize<'de> + Send + Sync> Server<C> {
    /// Constructor for the Server that initializes it with a config read from the given path.
    ///
    /// # Arguments
    /// - `name`: The name for this server. If you need inspiration, use
    ///   `format!("{}/{}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"))` instead.
    /// - `config`: The path to the config file to read.
    ///
    /// # Returns
    /// A new Server initialized with the settings in the given `config`.
    #[inline]
    pub fn new_from_config(name: impl Into<String>, config: impl AsRef<Path>) -> Result<Self, Error> {
        let path: &Path = config.as_ref();

        // Open the file
        debug!("Reading config file at '{}'...", path.display());
        let handle: File = match File::open(path) {
            Ok(handle) => handle,
            Err(source) => return Err(Error::ConfigOpen { path: path.into(), source }),
        };

        // Read it and parse it
        debug!("Parsing config file at '{}'...", path.display());
        let mut context: Context<C> = match serde_yml::from_reader(handle) {
            Ok(context) => context,
            Err(source) => return Err(Error::ConfigLoad { path: path.into(), source }),
        };
        context.name = name.into();

        // Done, initialize
        Ok(Self { router: Router::new(), context })
    }
}
impl<C: 'static + Send + Sync> Server<C> {
    /// Constructor for the Server that initializes it with a given custom context.
    ///
    /// # Arguments
    /// - `name`: The name for this server. If you need inspiration, use
    ///   `format!("{}/{}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"))` instead.
    /// - `addr`: Some [`SocketAddr`] to bind the server on.
    /// - `site`: The path to the folder that we'll serve website files from.
    /// - `not_found_file`: The path to a special file that we serve when the user when they try to
    ///   navigate to a non-existing file.
    /// - `custom`: The custom `C`ontext to load.
    ///
    /// # Returns
    /// A new Server with the given properties and custom `C`ontext, plus other sensible defaults.
    #[inline]
    pub fn new_with_custom(
        name: impl Into<String>,
        addr: impl Into<SocketAddr>,
        site: impl Into<PathBuf>,
        not_found_file: impl Into<PathBuf>,
        custom: C,
    ) -> Self {
        Self {
            router:  Router::new(),
            context: Context {
                name: name.into(),
                addr: addr.into(),
                site: site.into(),
                not_found_file: not_found_file.into(),
                header_settings: Default::default(),
                custom,
            },
        }
    }
}

// Building

// Running
impl<C: 'static + Send + Sync> Server<C> {
    /// Runs the server.
    ///
    /// This function will also listen for certain interrupts. It can therefore return if the
    /// server has been interrupted.
    ///
    /// Note that this function will heavily create new futures as it goes. As such, it's
    /// recommended to run this as the main future in a [`Runtime`](tokio::runtime::Runtime).
    pub fn run(self) -> impl Future<Output = ()> {
        let Self { router, context } = self;

        // Finish building the router first
        let context: Arc<Context<C>> = Arc::new(context);
        let router = router.route("/", get(www::handle)).route("/{*path}", get(www::handle));
        let router = router.with_state(context.clone());
        let router: IntoMakeServiceWithConnectInfo<Router, SocketAddr> = Router::new().merge(router).into_make_service_with_connect_info();

        // Now move into the future
        async move {
            // Bind the TCP Listener
            debug!("Binding server on '{}'...", context.addr);
            let listener: TcpListener = match TcpListener::bind(context.addr).await {
                Ok(listener) => listener,
                Err(err) => {
                    error!("{}", toplevel!(("Failed to bind server to '{}'", context.addr), err));
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
                                error!("{}", toplevel!(("Failed to accept incoming connection"), err));
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
                                error!("{}", toplevel!(("Failed to serve incoming connection"), *err));
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
                            warn!("{}", toplevel!(("Failed to register SIGINT signal handler"), err));
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
                            warn!("{}", toplevel!(("Failed to register SIGTERM signal handler"), err));
                            warn!("Graceful shutdown by Docker disabled");
                            None
                        },
                    }
                } => {
                    debug!("Received SIGTERM");
                },
            }
        }
    }
}
