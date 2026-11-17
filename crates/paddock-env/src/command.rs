//! The one place a program is started from: a [`Command`] that opens no console window on Windows.
//!
//! `paddock_proc::command` re-exports [`command`] (the apps and tools start programs through it, and clippy forbids
//! `Command::new` there), and the test kit's `TestCommand` builds on it, so a test's child has the same start flags as the
//! product's. It lives in this leaf crate because `paddock-proc`'s own tests use the test kit: the kit cannot depend on
//! `paddock-proc` (that made a publish cycle, ci.yml 7989; ADR 0100).

use std::ffi::OsStr;
use std::process::Command;

/// Windows process creation flag: no console window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A command for `program` that opens no console window on Windows (a GUI app starting a console tool would flash one,
/// or, under Remote Desktop, show it). The only place the apps start programs; clippy forbids `Command::new` elsewhere.
#[allow(clippy::disallowed_methods)] // the single allowed place
pub fn command(program: impl AsRef<OsStr>) -> Command {
    let cmd = Command::new(program);
    #[cfg(windows)]
    let cmd = {
        use std::os::windows::process::CommandExt;
        let mut cmd = cmd;
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd
    };
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_is_for_the_program_and_nothing_else_is_added() {
        let c = command("some-tool");
        assert_eq!(c.get_program(), "some-tool");
        assert_eq!(c.get_args().count(), 0);
        assert_eq!(c.get_envs().count(), 0);
    }
}
