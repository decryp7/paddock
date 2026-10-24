//! The `cargo-paddock-plugin` program: `cargo paddock-plugin <command>` or `cargo-paddock-plugin <command>`. Everything is in
//! the library; this file supplies the real working folder, `cargo` and standard streams (it is left out of the coverage gate
//! with the other process entries).

use cargo_paddock_plugin::cargo::ProcCargo;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let exit = cargo_paddock_plugin::run_cli(
        &args,
        cwd,
        &ProcCargo::from_env(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    std::process::exit(exit.code());
}
