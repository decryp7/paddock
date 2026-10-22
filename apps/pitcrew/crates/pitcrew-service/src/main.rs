//! `pitcrew-service`: the background process that keeps the runners going (ADR 0001, 0003).
// No console window when Windows starts the Ctrl+Break helper or the service.
#![cfg_attr(not(debug_assertions), windows_subsystem = "console")]

use pitcrew_service::cli::{self, Command};
use pitcrew_service::error::Error;
use pitcrew_service::{host, paths, platform, supervisor};

fn main() {
    let command = match cli::parse(std::env::args().skip(1)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = execute(command) {
        eprintln!("pitcrew-service: {e}");
        std::process::exit(e.exit_code());
    }
}

fn execute(command: Command) -> Result<(), Error> {
    match command {
        Command::Help => {
            println!("{}", cli::USAGE);
            Ok(())
        }
        Command::Version => {
            println!("pitcrew-service {}", supervisor::SERVICE_VERSION);
            Ok(())
        }
        Command::Run => host::run_foreground(data_dir()?),
        Command::Status => pitcrew_service::status::print(&paths::socket_dir().map_or_else(data_dir, Ok)?),
        Command::Service => platform::dispatch(),
        Command::Install => platform::install(),
        Command::Uninstall => platform::uninstall(),
        Command::CtrlBreak(pid) => ctrl_break(pid),
        Command::CheckConfig => {
            let dir = data_dir()?;
            let loaded = supervisor::load_config(&dir)?;
            println!(
                "{} ({:?}): {} runner(s)",
                dir.join(paths::CONFIG_FILE).display(),
                loaded.source,
                loaded.config.runners.len()
            );
            for p in &loaded.problems {
                println!("  problem: {p}");
            }
            Ok(())
        }
    }
}

#[cfg(windows)]
fn ctrl_break(pid: u32) -> Result<(), Error> {
    platform::win_session::send_ctrl_break(pid).map_err(|reason| Error::Os {
        what: "Stopping the runner",
        reason,
    })
}

#[cfg(not(windows))]
fn ctrl_break(_pid: u32) -> Result<(), Error> {
    Err(Error::Unsupported("ctrl-break (Windows only)"))
}

fn data_dir() -> Result<std::path::PathBuf, Error> {
    paths::data_dir().ok_or(Error::Os {
        what: "Finding the data folder",
        reason: "the environment names no data folder".into(),
    })
}
