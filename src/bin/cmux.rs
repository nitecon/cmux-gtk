//! cmux CLI binary entry point.
//!
//! This binary is independent of GTK4 — it communicates with the running
//! cmux-app instance via Unix socket JSON-RPC.

#[path = "../browser_timeout.rs"]
mod browser_timeout;
#[path = "../cli/mod.rs"]
mod cli;
#[path = "../review_comments.rs"]
mod review_comments;
#[path = "../task.rs"]
#[allow(dead_code)]
mod task;

use clap::Parser;

/// Parse arguments and translate command failures into the documented process exit codes.
fn main() -> std::process::ExitCode {
    let cli_args = cli::Cli::parse();
    match cli::run(cli_args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(cli::CliError::Connection(msg)) => {
            eprintln!("Error: {}", msg);
            std::process::ExitCode::from(2)
        }
        Err(
            cli::CliError::Command(msg) | cli::CliError::Protocol(msg) | cli::CliError::Output(msg),
        ) => {
            eprintln!("Error: {}", msg);
            std::process::ExitCode::from(1)
        }
    }
}
