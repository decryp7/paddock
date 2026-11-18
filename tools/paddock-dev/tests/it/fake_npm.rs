//! The `npm` of the release-isolation fixture, shared by `tests/it/release_isolation.rs` and the unit tests of the module (each
//! includes this file by path; it is not a module of `main.rs`). The fixture lives in the library, where the testkit is not a
//! dependency, so the caller installs the stand-in.

use super::Fixture;
use paddock_testkit::{Actions, StandIn, template_literal};
use std::path::Path;

/// A job whose `npm` is a stand-in that answers `npm config get userconfig|globalconfig` the way npm would for this fake install.
pub fn build(root: &Path) -> Fixture {
    Fixture::build(root, &|bin, prefix, home| {
        // The global config sits under the prefix: `<prefix>/etc/npmrc`, and on Windows `<prefix>\bin\etc\npmrc`.
        let global = if cfg!(windows) {
            format!("{}\\bin\\etc\\npmrc", prefix.display())
        } else {
            format!("{}/etc/npmrc", prefix.display())
        };
        StandIn::new(bin, "npm")
            .when(
                &paddock_testkit::Case::new().has_arg("globalconfig"),
                Actions::new().stdout_with(&template_literal(&global)),
            )
            .when(
                &paddock_testkit::Case::new().has_arg("userconfig"),
                Actions::new().stdout_with(&template_literal(&format!(
                    "{}{}.npmrc",
                    home.display(),
                    if cfg!(windows) { '\\' } else { '/' }
                ))),
            )
            .install()
            .expect("fake npm");
    })
}
