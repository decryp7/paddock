//! The real [`Spawner`]: starts the sibling detached. Platform glue, left out of the coverage gate like the other native
//! files; the behaviour around it is tested with [`crate::testing::FakeSpawner`], and no test starts an installed app.

use crate::probe::Spawner;
use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;

/// Starts programs for real.
#[derive(Debug, Clone, Copy, Default)]
pub struct ProcessSpawner;

impl Spawner for ProcessSpawner {
    fn launch(&self, program: &Path, args: &[OsString]) -> Result<(), String> {
        // `paddock_proc::command` hides the console window on Windows; the child keeps running after this returns.
        let mut cmd = paddock_proc::command(program);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd.spawn().map(drop).map_err(|e| e.to_string())
    }
}
