//! `verify`: open a package (layout, limits, checksums, signature) and decide trust against the keys given.

use crate::Context;
use crate::args::VerifyArgs;
use crate::errors::package;
use paddock_cli::{CliError, Report};
use paddock_plugin_package::{
    Package, PackageError, PublicKey, Signature, Trust, TrustPolicy, TrustSettings,
};
use serde_json::json;
use std::path::Path;

/// A public key given as key text, or as a file holding it (at most 4 KiB).
fn trusted_key(cwd: &Path, given: &str) -> Result<PublicKey, CliError> {
    let file = cwd.join(given);
    let text = if file.is_file() {
        let meta = std::fs::metadata(&file).map_err(|e| package(&PackageError::from(e)))?;
        if meta.len() > 4096 {
            return Err(CliError::failure(format!(
                "{} is too large to be a key file.",
                file.display()
            ))
            .with_hint("A public key file holds one line of text."));
        }
        std::fs::read_to_string(&file).map_err(|e| package(&PackageError::from(e)))?
    } else {
        given.to_string()
    };
    PublicKey::parse(&text).map_err(|e| package(&e))
}

/// `verify`.
///
/// # Errors
///
/// A package that is damaged, changed, or (with `--trust`) not signed by a trusted key is refused (exit 4); a file that is
/// not a package, or a key that cannot be read, fails (exit 1).
pub fn verify(ctx: &Context, a: &VerifyArgs) -> Result<Report, CliError> {
    let path = ctx.cwd.join(&a.package);
    let pkg = Package::open(&path).map_err(|e| package(&e))?;
    let (signature, key) = match &pkg.signature {
        Signature::Unsigned => ("unsigned", None),
        Signature::Signed { key } => ("signed", Some(key.id())),
    };
    let trust = if a.trust.is_empty() {
        None
    } else {
        let keys = a
            .trust
            .iter()
            .map(|t| trusted_key(&ctx.cwd, t).map(|k| (format!("key {}", k.id()), k.to_text())))
            .collect::<Result<Vec<_>, _>>()?;
        let named: Vec<(&str, &str)> = keys.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
        let policy = TrustPolicy::new(&named, TrustSettings::default()).map_err(|e| package(&e))?;
        Some(policy.decide(&pkg.signature).map_err(|e| package(&e))?)
    };
    let trusted = trust
        .as_ref()
        .map(|t| matches!(t, Trust::Builtin { .. } | Trust::Signed { .. }));
    let m = &pkg.manifest;
    let verdict = match (&key, trusted) {
        (None, _) => "not signed".to_string(),
        (Some(k), Some(_)) => format!("signed with key {k}, a key you trust"),
        (Some(k), None) => format!(
            "signed with key {k} (pass --trust <public key> to check it is the publisher's)"
        ),
    };
    let targets: Vec<&str> = m.targets.iter().map(String::as_str).collect();
    let text = format!(
        "{} {} by {}: checksums and signature are intact, {verdict}\ntargets {}\ndigest {}",
        m.id,
        m.version,
        m.publisher,
        targets.join(", "),
        pkg.digest()
    );
    Ok(Report::new(
        json!({
            "package": path.to_string_lossy(), "id": m.id.as_str(), "version": m.version.to_string(),
            "publisher": m.publisher, "targets": targets, "files": pkg.entries.len(),
            "digest": pkg.digest(), "signature": signature, "key": key, "trusted": trusted,
        }),
        text,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{FakeCargo, context};
    use paddock_plugin_package::SecretKey;
    use paddock_plugin_package::archive::{Build, build};
    use paddock_plugin_package::manifest::host_target;

    fn make(dir: &Path, name: &str, key: Option<&SecretKey>) -> String {
        let program = dir.join("prog");
        std::fs::write(&program, b"program bytes").unwrap();
        build(
            Build {
                manifest: paddock_plugin_test::fixture::manifest("2.0", "prog", ""),
                programs: vec![(host_target().into(), &program)],
                sign: key,
            },
            &dir.join(name),
        )
        .unwrap();
        name.into()
    }

    fn args(package: &str, trust: &[&str]) -> VerifyArgs {
        VerifyArgs {
            package: package.into(),
            trust: trust.iter().map(ToString::to_string).collect(),
        }
    }

    #[test]
    fn a_good_package_verifies_and_trust_is_decided_by_the_keys_given() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = context(dir.path(), FakeCargo::making_nothing(), false);
        let key = SecretKey::generate().unwrap();
        let other = SecretKey::generate().unwrap();
        let signed = make(dir.path(), "s.paddockplugin", Some(&key));
        let unsigned = make(dir.path(), "u.paddockplugin", None);
        // Intact, nothing to trust against.
        let r = verify(&ctx, &args(&signed, &[])).unwrap();
        assert_eq!(r.data["signature"], "signed");
        assert_eq!(r.data["trusted"], serde_json::Value::Null);
        assert_eq!(r.data["id"], paddock_plugin_test::fixture::ID);
        assert!(r.text.contains("pass --trust"), "{}", r.text);
        let r = verify(&ctx, &args(&unsigned, &[])).unwrap();
        assert_eq!(r.data["signature"], "unsigned");
        assert!(r.text.contains("not signed"));
        // Trusted, as key text and as a file.
        let r = verify(&ctx, &args(&signed, &[&key.public().to_text()])).unwrap();
        assert_eq!(r.data["trusted"], true);
        assert!(r.text.contains("a key you trust"));
        std::fs::write(dir.path().join("pub.txt"), key.public().to_text()).unwrap();
        assert_eq!(
            verify(&ctx, &args(&signed, &["pub.txt"])).unwrap().data["trusted"],
            true
        );
        // The wrong key, and no signature when one is required: refused.
        let e = verify(&ctx, &args(&signed, &[&other.public().to_text()])).unwrap_err();
        assert_eq!(e.exit, paddock_cli::Exit::Refused);
        assert!(e.message.contains(&key.public().id()), "{e:?}");
        let e = verify(&ctx, &args(&unsigned, &[&key.public().to_text()])).unwrap_err();
        assert_eq!(e.exit, paddock_cli::Exit::Refused);
        assert!(e.message.contains("not signed"));
    }

    #[test]
    fn junk_a_missing_file_and_bad_key_files_fail_with_a_reason() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = context(dir.path(), FakeCargo::making_nothing(), false);
        std::fs::write(dir.path().join("junk"), "not a zip").unwrap();
        assert!(
            verify(&ctx, &args("junk", &[]))
                .unwrap_err()
                .message
                .contains("not a plugin package")
        );
        assert!(verify(&ctx, &args("missing", &[])).is_err());
        let fresh = make(dir.path(), "f.paddockplugin", None);
        assert!(
            verify(&ctx, &args(&fresh, &["not a key"]))
                .unwrap_err()
                .hint
                .contains("keygen")
        );
        std::fs::write(dir.path().join("big.key"), vec![b'a'; 5000]).unwrap();
        assert!(
            verify(&ctx, &args(&fresh, &["big.key"]))
                .unwrap_err()
                .message
                .contains("too large")
        );
    }
}
