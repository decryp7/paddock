//! `pitcrew-service`: the background process that keeps the runners going (ADR 0001, 0003).

use pitcrew_service::cli::{self, Command};
use pitcrew_service::error::Error;
use pitcrew_service::{dispatch, paths, platform, supervisor};

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
            println!("pitcrew-service {}", dispatch::SERVICE_VERSION);
            Ok(())
        }
        Command::Run => supervisor::run(&data_dir()?),
        Command::Service => platform::dispatch(),
        Command::Install => platform::install(),
        Command::Uninstall => platform::uninstall(),
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

fn data_dir() -> Result<std::path::PathBuf, Error> {
    paths::data_dir().ok_or(Error::Os {
        what: "Finding the data folder",
        reason: "the environment names no data folder".into(),
    })
}
