//! `pitwall`: see the library crate. Parses, runs on a small async runtime, prints errors to stderr and exits with the
//! documented code.

use pitwall_cli::args::{self, Parsed};
use pitwall_cli::discover::Env;
use pitwall_cli::exit::{CliError, Exit};

fn fail(e: &CliError) -> ! {
    eprintln!("pitwall: {}", e.message);
    std::process::exit(e.exit.code());
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let inv = match args::parse(&argv) {
        Ok(Parsed::Help(h)) => {
            print!("{h}");
            std::process::exit(Exit::Ok.code());
        }
        Ok(Parsed::Version) => {
            println!("pitwall {}", env!("CARGO_PKG_VERSION"));
            std::process::exit(Exit::Ok.code());
        }
        Ok(Parsed::Run(i)) => i,
        Err(e) => fail(&e.into()),
    };
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => fail(&CliError::failure(format!("could not start: {e}"))),
    };
    if let Err(e) = rt.block_on(pitwall_cli::run::run(inv, Env::from_process())) {
        fail(&e);
    }
}
