//! "Who installed this program?": Linux programs carry no code signatures, so the closest honest answer to Windows'
//! "signed by" is the distribution package that owns the file (`dpkg -S`, `rpm -qf`, `pacman -Qo`). Distribution
//! packages are signed by their repository, which is what that answer rests on.

use super::tools::Tools;
use crate::parse::linux_tools::{dpkg_owner, dpkg_owners, pacman_owner, pacman_owners, rpm_owner};
use crate::signatures::{Looked, SignatureLookup};
use dyno_core::ports::PlatformError;
use dyno_core::process::SignatureInfo;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// How long one package lookup may take (`dpkg -S` scans the whole file database, once however many paths it is given).
const LOOKUP_TIMEOUT: Duration = Duration::from_secs(4);

/// Reads the owning package out of a manager's output.
type OwnerParser = fn(&str) -> Option<String>;

/// The package managers, in the order they are tried: program, arguments before the path, output parser.
const MANAGERS: [(&str, &[&str], OwnerParser); 3] = [
    ("dpkg", &["-S", "--"], dpkg_owner),
    ("rpm", &["-qf", "--"], rpm_owner),
    ("pacman", &["-Qo", "--"], pacman_owner),
];

/// Looks up package ownership for a batch of files ([`crate::signatures::CachedSignatures`] keeps the answers): one `dpkg -S`
/// or `pacman -Qo` run for the whole batch (both take many paths and name each in their answer); `rpm -qf` once per file.
pub(crate) struct PackageLookup {
    tools: Arc<dyn Tools>,
}

impl PackageLookup {
    /// A lookup using `tools`.
    pub(crate) fn new(tools: Arc<dyn Tools>) -> Self {
        PackageLookup { tools }
    }

    /// A file that is not asked about: its answer, or `None` for a file to ask about.
    fn not_asked(exe: &Path) -> Option<Looked> {
        let Some(text) = exe.to_str().filter(|e| e.starts_with('/')) else {
            return Some(Looked::Kept(SignatureInfo::Unknown("The program file is unknown".into())));
        };
        text.ends_with(" (deleted)").then(|| {
            Looked::Once(SignatureInfo::Unknown(
                "The program file was replaced or removed while it runs (usually an update)".into(),
            ))
        })
    }

    /// One run of `manager` over `paths`: each path's owning package, or the run's failure as every path's answer.
    fn run_batch(&self, manager: &str, args: &[&str], paths: &[&str], owners: fn(&str) -> Vec<(String, String)>) -> Vec<Looked> {
        let mut full: Vec<&str> = args.to_vec();
        full.extend_from_slice(paths);
        match self.tools.run(manager, &full, LOOKUP_TIMEOUT) {
            // A path no package owns makes the run fail (dpkg exits 1, pacman too) while the others are still answered.
            Ok(out) => {
                let found = owners(&out.stdout);
                paths
                    .iter()
                    .map(|p| match found.iter().find(|(path, _)| path == p) {
                        Some((_, pkg)) => Looked::Kept(from_package(manager, pkg)),
                        None => Looked::Kept(not_from_a_package()),
                    })
                    .collect()
            }
            Err(e) => paths.iter().map(|_| failed(manager, &e)).collect(),
        }
    }
}

fn from_package(manager: &str, pkg: &str) -> SignatureInfo {
    SignatureInfo::Signed(format!("Installed from the {manager} package {pkg}"))
}

fn failed(manager: &str, e: &PlatformError) -> Looked {
    match e {
        // A timeout is not kept: the next look may be quicker.
        PlatformError::Timeout(_) => Looked::Once(SignatureInfo::Unknown(format!("{manager} did not answer in time"))),
        e => Looked::Kept(SignatureInfo::Unknown(e.to_string())),
    }
}

impl SignatureLookup for PackageLookup {
    fn lookup(&self, exes: &[&Path]) -> Vec<Looked> {
        let mut out: Vec<Option<Looked>> = exes.iter().map(|e| Self::not_asked(e)).collect();
        let asked: Vec<(usize, &str)> = exes
            .iter()
            .enumerate()
            .filter(|(i, _)| out[*i].is_none())
            .filter_map(|(i, e)| e.to_str().map(|t| (i, t)))
            .collect();
        if asked.is_empty() {
            return out.into_iter().flatten().collect();
        }
        let paths: Vec<&str> = asked.iter().map(|(_, p)| *p).collect();
        let answers: Vec<Looked> = match MANAGERS.iter().find(|(m, _, _)| self.tools.exists(m)) {
            None => paths
                .iter()
                .map(|_| {
                    Looked::Kept(SignatureInfo::Unknown(
                        "No package manager (dpkg, rpm or pacman) was found to ask".into(),
                    ))
                })
                .collect(),
            Some(("dpkg", args, _)) => self.run_batch("dpkg", args, &paths, dpkg_owners),
            Some(("pacman", args, _)) => self.run_batch("pacman", args, &paths, pacman_owners),
            Some((manager, args, parse)) => paths
                .iter()
                // per-item: `rpm -qf` names no path in its answer (a package per line, and two lines for a file two packages
                // own), so the lines of a batch cannot be matched to its files.
                .map(|p| {
                    let mut full: Vec<&str> = args.to_vec();
                    full.push(p);
                    match self.tools.run(manager, &full, LOOKUP_TIMEOUT) {
                        Ok(o) if o.ok => Looked::Kept(parse(&o.stdout).map_or_else(not_from_a_package, |pkg| from_package(manager, &pkg))),
                        Ok(_) => Looked::Kept(not_from_a_package()),
                        Err(e) => failed(manager, &e),
                    }
                })
                .collect(),
        };
        for ((i, _), a) in asked.iter().zip(answers) {
            out[*i] = Some(a);
        }
        out.into_iter()
            .map(|a| a.unwrap_or_else(|| Looked::Kept(SignatureInfo::Unknown("The program file is unknown".into()))))
            .collect()
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
    use crate::signatures::CachedSignatures;
    use dyno_core::ports::SignatureProbe;

    fn owners(tools: Arc<FakeTools>) -> CachedSignatures {
        CachedSignatures::new(Box::new(PackageLookup::new(tools)))
    }

    fn one(p: &CachedSignatures, exe: &str) -> SignatureInfo {
        p.signatures(&[Path::new(exe)]).remove(0)
    }

    #[test]
    fn a_packaged_program_is_attributed_to_its_package_and_cached() {
        // A real file, so the cache can key it by size and time; dpkg is scripted to own it.
        let dir = paddock_testkit::private_tempdir().unwrap();
        let ls = dir.path().join("ls");
        std::fs::write(&ls, b"x").unwrap();
        let ls = ls.to_str().unwrap().to_string();
        let tools = Arc::new(FakeTools::with("dpkg", &format!("coreutils: {ls}\n")));
        let p = owners(tools.clone());
        for _ in 0..3 {
            assert_eq!(
                one(&p, &ls),
                SignatureInfo::Signed("Installed from the dpkg package coreutils".into())
            );
        }
        assert_eq!(tools.calls(), [format!("dpkg -S -- {ls}")]);
    }

    /// DY-5 (ADR 0108, guard D): new programs are one package-manager run, not one each (dpkg and pacman take many paths).
    #[test]
    fn a_batch_of_new_programs_is_one_dpkg_or_pacman_run() {
        let exes = ["/usr/bin/a", "/usr/bin/b", "/opt/own/c"].map(Path::new);
        let dpkg = Arc::new(FakeTools::default());
        dpkg.answer(
            "dpkg",
            Ok(ToolOutput {
                ok: false,
                stdout: "diversion by x from: /usr/bin/a\npa: /usr/bin/a\npb, pb2: /usr/bin/b\n".into(),
                stderr: "dpkg-query: no path found matching pattern /opt/own/c".into(),
            }),
        );
        let got = owners(dpkg.clone()).signatures(&exes);
        assert_eq!(dpkg.calls(), ["dpkg -S -- /usr/bin/a /usr/bin/b /opt/own/c"]);
        assert_eq!(got[0], SignatureInfo::Signed("Installed from the dpkg package pa".into()));
        assert_eq!(got[1], SignatureInfo::Signed("Installed from the dpkg package pb".into()));
        assert!(matches!(&got[2], SignatureInfo::Unknown(m) if m.contains("Not installed from a package")));
        let pacman = Arc::new(FakeTools::with(
            "pacman",
            "/usr/bin/a is owned by pa 1.0-1\n/usr/bin/b is owned by pb 2.0-1\n",
        ));
        let got = owners(pacman.clone()).signatures(&exes);
        assert_eq!(pacman.calls().len(), 1);
        assert!(matches!(&got[1], SignatureInfo::Signed(m) if m.contains("pb 2.0-1")));
        assert!(matches!(&got[2], SignatureInfo::Unknown(_)));
        // rpm answers without the path, so it is asked once per file.
        let rpm = Arc::new(FakeTools::with("rpm", "bash-5.2-1.x86_64\n"));
        owners(rpm.clone()).signatures(&exes);
        assert_eq!(rpm.calls().len(), 3);
    }

    #[test]
    fn other_managers_and_unpackaged_files() {
        let rpm = Arc::new(FakeTools::with("rpm", "bash-5.2-1.x86_64\n"));
        assert!(matches!(one(&owners(rpm), "/usr/bin/bash"), SignatureInfo::Signed(m) if m.contains("rpm package bash-5.2")));
        let pac = Arc::new(FakeTools::with("pacman", "/usr/bin/ls is owned by coreutils 9.4-3\n"));
        assert!(matches!(one(&owners(pac), "/usr/bin/ls"), SignatureInfo::Signed(m) if m.contains("coreutils 9.4-3")));
        let none = Arc::new(FakeTools::default());
        none.answer(
            "dpkg",
            Ok(ToolOutput {
                ok: false,
                stdout: String::new(),
                stderr: "no path found".into(),
            }),
        );
        assert!(matches!(one(&owners(none), "/home/me/tool"), SignatureInfo::Unknown(m) if m.contains("Not installed from a package")));
    }

    #[test]
    fn missing_tools_files_and_timeouts_are_unknown_with_a_reason() {
        let p = owners(Arc::new(FakeTools::default()));
        assert!(matches!(one(&p, "/usr/bin/ls"), SignatureInfo::Unknown(m) if m.contains("No package manager")));
        assert!(matches!(one(&p, "relative/x"), SignatureInfo::Unknown(_)));
        assert!(matches!(one(&p, "/tmp/x (deleted)"), SignatureInfo::Unknown(m) if m.contains("replaced")));
        let dir = paddock_testkit::private_tempdir().unwrap();
        let ls = dir.path().join("ls");
        std::fs::write(&ls, b"x").unwrap();
        let ls = ls.to_str().unwrap().to_string();
        let slow = Arc::new(FakeTools::default());
        slow.answer("dpkg", Err(PlatformError::Timeout("dpkg did not finish within 4 s.".into())));
        let p = owners(slow.clone());
        assert!(matches!(one(&p, &ls), SignatureInfo::Unknown(m) if m.contains("did not answer")));
        slow.answer(
            "dpkg",
            Ok(ToolOutput {
                ok: true,
                stdout: format!("coreutils: {ls}\n"),
                stderr: String::new(),
            }),
        );
        assert!(matches!(one(&p, &ls), SignatureInfo::Signed(_)), "a timeout is not cached");
    }
}
