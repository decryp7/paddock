//! The linker arguments `paddock-setup.exe` is built with on Windows (MSVC target). `build.rs` includes this file by path and
//! hands them to the linker; a test here pins them, and the release gate (`conformance/release/check-per-machine-installer.mjs`)
//! reads the result back from the shipped file.

/// `/Brepro`: lld-link derives the header time stamp from the contents, not the clock (Dyno 1.1.1 WP-7).
pub const REPRODUCIBLE: &str = "/Brepro";
/// `/DEBUG:NONE`: no PDB path or GUID (a PDB holds the build folder's absolute paths).
pub const NO_DEBUG_INFO: &str = "/DEBUG:NONE";
/// `/DEPENDENTLOADFLAG:0x800` (`LOAD_LIBRARY_SEARCH_SYSTEM32`): the program runs from `$PLUGINSDIR` and imports `version.dll`; with
/// the default search order a `version.dll` planted beside it would load with the installer's rights (Dyno 1.1.1 WP-6 (2)).
pub const SYSTEM32_ONLY: &str = "/DEPENDENTLOADFLAG:0x800";

/// Every linker argument of the bin, in order.
pub fn msvc_link_args() -> [&'static str; 3] {
    [REPRODUCIBLE, NO_DEBUG_INFO, SYSTEM32_ONLY]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_program_loads_its_dlls_from_system32_only() {
        assert!(
            msvc_link_args().contains(&"/DEPENDENTLOADFLAG:0x800"),
            "{:?}",
            msvc_link_args()
        );
    }

    #[test]
    fn the_link_stays_deterministic() {
        let args = msvc_link_args();
        assert!(
            args.contains(&"/Brepro") && args.contains(&"/DEBUG:NONE"),
            "{args:?}"
        );
    }
}
