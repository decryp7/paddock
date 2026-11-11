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
    // Before anything can load a library: dynamic loads (`LoadLibrary`, delay loads) search the system folder only, as the link flag
    // does for the imports the loader resolves at start (`/DEPENDENTLOADFLAG:0x800`, src/linkargs.rs). Dyno 1.1.1 WP-6.
    #[cfg(windows)]
    restrict_dll_search_to_system32();
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

/// `SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32)`. A failure (a Windows without the API) is not fatal: the link flag
/// still covers the start-up imports.
#[cfg(windows)]
fn restrict_dll_search_to_system32() {
    use windows_sys::Win32::System::LibraryLoader::{
        LOAD_LIBRARY_SEARCH_SYSTEM32, SetDefaultDllDirectories,
    };
    // SAFETY: a plain call with a documented flag value; it touches no memory of ours.
    let _ = unsafe { SetDefaultDllDirectories(LOAD_LIBRARY_SEARCH_SYSTEM32) };
}
