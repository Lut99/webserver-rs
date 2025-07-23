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

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use error_trace::toplevel;
use humanlog::{DebugMode, HumanLogger};
use log::{debug, error, info};
use tokio::runtime::{Builder, Runtime};
use webserver::Server;


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
fn main() -> ExitCode {
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
            error!("{}", toplevel!(("Failed to create tokio runtime"), err));
            return ExitCode::FAILURE;
        },
    };

    // Initialize the server
    let server: Server = match Server::new_from_config(format!("{}/{}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION")), args.config_path) {
        Ok(server) => server,
        Err(err) => {
            error!("{}", toplevel!(("Failed to setup server"), err));
            return ExitCode::FAILURE;
        },
    };

    // Run the main async function
    runtime.block_on(server.run());

    // When the server stops, quit the runtime too
    info!("Terminating tokio runtime ({SHUTDOWN_TIMEOUT_S}s timeout)...");
    runtime.shutdown_timeout(Duration::from_secs(SHUTDOWN_TIMEOUT_S));
    info!("Done.");
    ExitCode::SUCCESS
}
