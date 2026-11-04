//! `paddock-setup.exe`: the entry point. Everything is in the library; this file only turns the command line into a
//! [`paddock_setup::args::Command`], runs it and exits with its code.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(run(&args));
}

#[cfg(windows)]
fn run(args: &[String]) -> i32 {
    paddock_setup::native::main(args)
}

#[cfg(not(windows))]
fn run(_args: &[String]) -> i32 {
    eprintln!("paddock-setup runs on Windows only");
    paddock_setup::exit::Exit::Failed.code()
}
