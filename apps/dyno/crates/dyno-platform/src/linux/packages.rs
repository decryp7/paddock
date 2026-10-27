//! "Who installed this program?": Linux programs carry no code signatures, so the closest honest answer to Windows'
//! "signed by" is the distribution package that owns the file (`dpkg -S`, `rpm -qf`, `pacman -Qo`). Distribution
//! packages are signed by their repository, which is what that answer rests on.

use super::tools::Tools;
use crate::parse::linux_tools::{dpkg_owner, pacman_owner, rpm_owner};
use dyno_core::ports::PlatformError;
use dyno_core::process::SignatureInfo;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// How long one package lookup may take (`dpkg -S` scans the whole file database).
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(4);
/// Answers kept; the cache is dropped when it grows past this.
const CACHE_LIMIT: usize = 1024;

/// Reads the owning package out of a manager's output.
type OwnerParser = fn(&str) -> Option<String>;

/// The package managers, in the order they are tried: program, arguments before the path, output parser.
const MANAGERS: [(&str, &[&str], OwnerParser); 3] = [
    ("dpkg", &["-S", "--"], dpkg_owner),
    ("rpm", &["-qf", "--"], rpm_owner),
    ("pacman", &["-Qo", "--"], pacman_owner),
];

/// Looks up package ownership, remembering answers.
pub struct PackageOwners {
    tools: Arc<dyn Tools>,
    cache: HashMap<String, SignatureInfo>,
}

impl PackageOwners {
    /// A lookup using `tools`.
    pub fn new(tools: Arc<dyn Tools>) -> Self {
        PackageOwners {
            tools,
            cache: HashMap::new(),
        }
    }

    /// The signature-like verdict for the program at `exe`.
    pub fn signature(&mut self, exe: Option<&str>) -> SignatureInfo {
        let Some(exe) = exe.filter(|e| e.starts_with('/')) else {
            return SignatureInfo::Unknown("The program file is unknown".into());
        };
        if exe.ends_with(" (deleted)") {
            return SignatureInfo::Unknown("The program file was replaced or removed while it runs (usually an update)".into());
        }
        if let Some(known) = self.cache.get(exe) {
            return known.clone();
        }
        let answer = self.lookup(exe);
        // Timeouts are not remembered: the next look may be quicker.
        if !matches!(&answer, SignatureInfo::Unknown(m) if m.contains("did not answer")) {
            if self.cache.len() >= CACHE_LIMIT {
                self.cache.clear();
            }
            self.cache.insert(exe.to_string(), answer.clone());
        }
        answer
    }

    fn lookup(&self, exe: &str) -> SignatureInfo {
        let Some((manager, args, parse)) = MANAGERS.iter().find(|(m, _, _)| self.tools.exists(m)) else {
            return SignatureInfo::Unknown("No package manager (dpkg, rpm or pacman) was found to ask".into());
        };
        let mut full: Vec<&str> = args.to_vec();
        full.push(exe);
        match self.tools.run(manager, &full, LOOKUP_TIMEOUT) {
            Ok(out) if out.ok => match parse(&out.stdout) {
                Some(pkg) => SignatureInfo::Signed(format!("Installed from the {manager} package {pkg}")),
                None => not_from_a_package(),
            },
            Ok(_) => not_from_a_package(),
            Err(PlatformError::Timeout(_)) => SignatureInfo::Unknown(format!("{manager} did not answer in time")),
            Err(e) => SignatureInfo::Unknown(e.to_string()),
        }
    }
}

fn not_from_a_package() -> SignatureInfo {
    SignatureInfo::Unknown("Not installed from a package (programs on Linux carry no signature to check)".into())
}

#[cfg(test)]
mod tests {
    use super::super::tools::fake::FakeTools;
    use super::super::tools::ToolOutput;
    use super::*;

    #[test]
    fn a_packaged_program_is_attributed_to_its_package_and_cached() {
        let tools = Arc::new(FakeTools::with("dpkg", "coreutils: /usr/bin/ls\n"));
        let mut p = PackageOwners::new(tools.clone());
        for _ in 0..3 {
            assert_eq!(
                p.signature(Some("/usr/bin/ls")),
                SignatureInfo::Signed("Installed from the dpkg package coreutils".into())
            );
        }
        assert_eq!(tools.calls(), ["dpkg -S -- /usr/bin/ls"]);
    }

    #[test]
    fn other_managers_and_unpackaged_files() {
        let rpm = Arc::new(FakeTools::with("rpm", "bash-5.2-1.x86_64\n"));
        assert!(
            matches!(PackageOwners::new(rpm).signature(Some("/usr/bin/bash")), SignatureInfo::Signed(m) if m.contains("rpm package bash-5.2"))
        );
        let pac = Arc::new(FakeTools::with("pacman", "/usr/bin/ls is owned by coreutils 9.4-3\n"));
        assert!(
            matches!(PackageOwners::new(pac).signature(Some("/usr/bin/ls")), SignatureInfo::Signed(m) if m.contains("coreutils 9.4-3"))
        );
        let none = Arc::new(FakeTools::default());
        none.answer(
            "dpkg",
            Ok(ToolOutput {
                ok: false,
                stdout: String::new(),
                stderr: "no path found".into(),
            }),
        );
        assert!(
            matches!(PackageOwners::new(none).signature(Some("/home/me/tool")), SignatureInfo::Unknown(m) if m.contains("Not installed from a package"))
        );
    }

    #[test]
    fn missing_tools_files_and_timeouts_are_unknown_with_a_reason() {
        let mut p = PackageOwners::new(Arc::new(FakeTools::default()));
        assert!(matches!(p.signature(Some("/usr/bin/ls")), SignatureInfo::Unknown(m) if m.contains("No package manager")));
        assert!(matches!(p.signature(None), SignatureInfo::Unknown(_)));
        assert!(matches!(p.signature(Some("relative/x")), SignatureInfo::Unknown(_)));
        assert!(matches!(p.signature(Some("/tmp/x (deleted)")), SignatureInfo::Unknown(m) if m.contains("replaced")));
        let slow = Arc::new(FakeTools::default());
        slow.answer("dpkg", Err(PlatformError::Timeout("dpkg did not finish within 4 s.".into())));
        let mut p = PackageOwners::new(slow.clone());
        assert!(matches!(p.signature(Some("/usr/bin/ls")), SignatureInfo::Unknown(m) if m.contains("did not answer")));
        slow.answer(
            "dpkg",
            Ok(ToolOutput {
                ok: true,
                stdout: "coreutils: /usr/bin/ls\n".into(),
                stderr: String::new(),
            }),
        );
        assert!(
            matches!(p.signature(Some("/usr/bin/ls")), SignatureInfo::Signed(_)),
            "a timeout is not cached"
        );
    }
}
