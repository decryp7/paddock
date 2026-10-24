//! The call into tauri-build: native glue, left out of coverage (it needs a Tauri project to build).

use crate::{Build, Env};

impl Build {
    /// Runs the build step: prints the directives, writes the manifest and the placeholders, then runs tauri-build with the
    /// attributes the plan chose. Panics with the reason when the step fails (a build script's way of failing the build).
    pub fn run(self) {
        let env = Env::process();
        let plan = match self.apply(&env, &mut |line| println!("{line}")) {
            Ok(plan) => plan,
            Err(e) => panic!("paddock-build: {e}"),
        };
        let windows = if plan.own_manifest {
            tauri_build::WindowsAttributes::new_without_app_manifest()
        } else {
            tauri_build::WindowsAttributes::new()
        };
        if let Err(e) =
            tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(windows))
        {
            panic!("tauri-build failed: {e:#}");
        }
    }
}
