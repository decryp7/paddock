//! `keygen` and `sign`: publisher keys and signing an existing package.

use crate::Context;
use crate::args::{KeygenArgs, SignArgs};
use crate::errors::package;
use paddock_cli::{CliError, Report};
use paddock_plugin_package::archive::{Build, build};
use paddock_plugin_package::{Package, SecretKey};
use serde_json::json;

/// Reads the secret key file `--key` names.
///
/// # Errors
///
/// When it is missing, too large or not a key; the hint says how to make one.
pub fn load_key(path: &std::path::Path) -> Result<SecretKey, CliError> {
    SecretKey::load(path).map_err(|e| {
        package(&e).with_hint("Make a key with `cargo paddock-plugin keygen --out <file>`, or check the path of --key.")
    })
}

/// `keygen`: a new key pair; the secret half goes to an owner-only file that is never overwritten.
///
/// # Errors
///
/// When the file exists or cannot be written.
pub fn keygen(ctx: &Context, a: &KeygenArgs) -> Result<Report, CliError> {
    let path = ctx.cwd.join(&a.out);
    let key = SecretKey::generate_to(&path).map_err(|e| package(&e))?;
    let public = key.public();
    let text = format!(
        "wrote the secret key to {} (readable by you only; keep it private and back it up)\npublic key {}\nkey id {}\nGive the public key to users who should trust your plugins (`verify --trust`).",
        path.display(),
        public.to_text(),
        public.id()
    );
    Ok(Report::new(
        json!({"file": path.to_string_lossy(), "publicKey": public.to_text(), "keyId": public.id()}),
        text,
    ))
}

/// `sign`: the same package with a signature by `--key` (any earlier signature is replaced).
///
/// # Errors
///
/// When the package is not valid, the key cannot be read or the signed package cannot be written.
pub fn sign(ctx: &Context, a: &SignArgs) -> Result<Report, CliError> {
    let path = ctx.cwd.join(&a.package);
    let key = load_key(&ctx.cwd.join(&a.key))?;
    let pkg = Package::open(&path).map_err(|e| package(&e))?;
    let out = a
        .out
        .as_ref()
        .map_or_else(|| path.clone(), |o| ctx.cwd.join(o));
    (ctx.note)(&format!("Signing {}...", out.display()));
    // The programs are taken out of the package (each checked against its checksum) and packed again with the signature.
    let work = tempfile::tempdir()
        .map_err(|e| CliError::failure(format!("no temporary folder ({e}).")))?;
    let mut programs = Vec::new();
    for (i, target) in pkg.manifest.targets.iter().enumerate() {
        let dest = work.path().join(format!("program-{i}"));
        pkg.extract(&path, target, &dest).map_err(|e| package(&e))?;
        programs.push((target.clone(), dest));
    }
    let digest = build(
        Build {
            manifest: pkg.manifest.clone(),
            programs: programs
                .iter()
                .map(|(t, p)| (t.clone(), p.as_path()))
                .collect(),
            sign: Some(&key),
        },
        &out,
    )
    .map_err(|e| package(&e))?;
    let id = key.public().id();
    Ok(Report::new(
        json!({"package": out.to_string_lossy(), "digest": digest, "keyId": id}),
        format!("signed {} with key {id}\ndigest {digest}", out.display()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{FakeCargo, context};
    use paddock_plugin_package::Signature;

    #[test]
    fn keygen_writes_a_private_file_once_and_prints_the_public_key() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = context(dir.path(), FakeCargo::making_nothing(), false);
        let a = KeygenArgs {
            out: "keys/me.key".into(),
        };
        let r = keygen(&ctx, &a).unwrap();
        let file = dir.path().join("keys/me.key");
        let key = SecretKey::load(&file).unwrap();
        assert_eq!(r.data["publicKey"], key.public().to_text());
        assert_eq!(r.data["keyId"], key.public().id());
        assert!(r.text.contains("keep it private") && !r.text.contains(&key.to_text()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        // Never overwrites.
        let e = keygen(&ctx, &a).unwrap_err();
        assert!(e.message.contains("already exists"), "{e:?}");
        assert_eq!(SecretKey::load(&file).unwrap(), key);
    }

    #[test]
    fn signing_replaces_the_signature_and_keeps_the_programs() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = context(dir.path(), FakeCargo::making_nothing(), false);
        let program = dir.path().join("prog");
        std::fs::write(&program, b"program bytes").unwrap();
        let target = paddock_plugin_package::manifest::host_target();
        let unsigned = dir.path().join("a.paddockplugin");
        build(
            Build {
                manifest: paddock_plugin_test::fixture::manifest("2.0", "prog", ""),
                programs: vec![(target.into(), &program)],
                sign: None,
            },
            &unsigned,
        )
        .unwrap();
        let first = SecretKey::generate().unwrap();
        let second = SecretKey::generate().unwrap();
        first.save(&dir.path().join("first.key")).unwrap();
        second.save(&dir.path().join("second.key")).unwrap();
        let sign_with = |key: &str, out: Option<&str>| SignArgs {
            package: "a.paddockplugin".into(),
            key: key.into(),
            out: out.map(Into::into),
        };
        // To another file; the original stays unsigned.
        let r = sign(&ctx, &sign_with("first.key", Some("b.paddockplugin"))).unwrap();
        assert_eq!(r.data["keyId"], first.public().id());
        assert_eq!(
            Package::open(&unsigned).unwrap().signature,
            Signature::Unsigned
        );
        let b = Package::open(&dir.path().join("b.paddockplugin")).unwrap();
        assert_eq!(
            b.signature,
            Signature::Signed {
                key: first.public()
            }
        );
        assert_eq!(b.entries.len(), 2);
        // In place, over an earlier signature.
        sign(&ctx, &sign_with("first.key", None)).unwrap();
        sign(&ctx, &sign_with("second.key", None)).unwrap();
        let signed = Package::open(&unsigned).unwrap();
        assert_eq!(
            signed.signature,
            Signature::Signed {
                key: second.public()
            }
        );
        let out = dir.path().join("extracted");
        signed.extract(&unsigned, target, &out).unwrap();
        assert_eq!(std::fs::read(out).unwrap(), b"program bytes");
    }

    #[test]
    fn signing_something_that_is_not_a_package_or_without_a_key_fails_with_a_next_step() {
        let dir = tempfile::tempdir().unwrap();
        let (ctx, _) = context(dir.path(), FakeCargo::making_nothing(), false);
        let a = SignArgs {
            package: "x.paddockplugin".into(),
            key: "k.key".into(),
            out: None,
        };
        assert!(sign(&ctx, &a).unwrap_err().hint.contains("keygen"));
        SecretKey::generate()
            .unwrap()
            .save(&dir.path().join("k.key"))
            .unwrap();
        assert!(
            sign(&ctx, &a)
                .unwrap_err()
                .message
                .contains("x.paddockplugin")
        );
        std::fs::write(dir.path().join("x.paddockplugin"), "not a zip").unwrap();
        let e = sign(&ctx, &a).unwrap_err();
        assert!(e.message.contains("not a plugin package"), "{e:?}");
    }
}
