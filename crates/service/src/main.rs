//! clashr-service — root-privileged daemon that owns the mihomo subprocess.
//!
//! Usage:
//!   clashr-service              # run the daemon (default)
//!   clashr-service install      # write launchd plist + launchctl load
//!   clashr-service uninstall    # launchctl unload + remove plist
//!   clashr-service status       # report whether the daemon is loaded
//!
//! Lifecycle: install runs once (with sudo). After that launchd keeps the
//! daemon alive forever; the ClashR app talks to it via Unix socket.

use std::env;
use std::process::ExitCode;

mod daemon;
mod install;
mod process;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("run");

    match subcmd {
        "run" => match daemon::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("daemon exited with error: {e:#}");
                ExitCode::FAILURE
            }
        },
        "install" => match install::install() {
            Ok(()) => {
                println!("clashr-service installed");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("install failed: {e:#}");
                ExitCode::FAILURE
            }
        },
        "uninstall" => match install::uninstall() {
            Ok(()) => {
                println!("clashr-service uninstalled");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("uninstall failed: {e:#}");
                ExitCode::FAILURE
            }
        },
        "status" => {
            let loaded = install::is_installed();
            println!("{}", if loaded { "installed" } else { "not installed" });
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("unknown subcommand: {other}");
            eprintln!("usage: clashr-service [run|install|uninstall|status]");
            ExitCode::FAILURE
        }
    }
}
