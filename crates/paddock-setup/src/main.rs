//! `paddock-setup.exe`: the entry point. Everything is in the library; this file only turns the command line into a
//! [`paddock_setup::args::Command`], runs it and exits with its code.
//!
//! A GUI-subsystem program on Windows: the installer, the uninstaller and the updater start it on every user's PC, and a
//! console-subsystem program opens a command prompt window when it is started without redirection (`Exec` in NSIS, `ShellExecute`
//! by the app). Its output still reaches whoever redirected it (`nsExec`, a pipe, a file): redirected standard handles are
//! inherited and kept. Run by hand from a terminal it attaches to that terminal's console (see `paddock_proc::console`); started
//! by a program without redirection it prints nothing, and `setup.log` is the record. The release gate
//! (conformance/release/check-per-machine-installer.mjs) checks the subsystem of the staged program.
#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    #[cfg(windows)]
    paddock_proc::console::attach_parent_console();
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
