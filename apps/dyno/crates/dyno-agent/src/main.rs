//! The `dyno-agent` program.
//!
//! A Windows release build is a GUI-subsystem program: the per-user `Run` entry starts it at logon with `run`, and a
//! console-subsystem program would open a visible command prompt for as long as it runs. Started from a terminal
//! (`dyno-agent --version`, `service install`) it attaches to that terminal's console to print.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    paddock_proc::console::attach_parent_console();
    std::process::exit(dyno_agent::cli::main(std::env::args().skip(1).collect()));
}

#[cfg(test)]
mod tests {
    /// The Run entry and the app start the agent without a console; only the GUI subsystem keeps a window from opening
    /// when explorer starts it.
    #[test]
    fn a_release_build_is_a_gui_subsystem_program() {
        let source = include_str!("main.rs");
        assert!(source.contains("windows_subsystem = \"windows\""));
        assert!(source.contains("attach_parent_console"));
    }
}
