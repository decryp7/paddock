//! The call into tauri-build: native glue, left out of coverage (it needs a Tauri project to build).

use crate::{Applied, Build, Env};

impl Build {
    /// Runs the build step: prints the directives, writes the manifest and the placeholders, then runs tauri-build with the
    /// attributes the plan chose. Panics with the reason when the step fails (a build script's way of failing the build).
    pub fn run(self) {
        let env = Env::process();
        let Applied {
            plan,
            tauri_config,
            removed,
        } = match self.apply(&env, &mut |line| println!("{line}")) {
            Ok(applied) => applied,
            Err(e) => panic!("paddock-build: {e}"),
        };
        for file in &removed {
            println!(
                "cargo:warning=removed the empty sidecar copy {}",
                file.display()
            );
        }
        if let Some(config) = tauri_config {
            // The sidecars are placeholders: tauri-build must not copy them over the programs cargo linked.
            // A build script is single-threaded at this point.
            #[allow(unsafe_code)]
            unsafe {
                std::env::set_var("TAURI_CONFIG", config);
            }
        }
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
