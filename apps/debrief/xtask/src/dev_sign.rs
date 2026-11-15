//! `dev-sign` and `build-app`: a self-signed code-signing identity ("Debrief Local Dev") in the login keychain, created once, so local
//! builds keep one identity across rebuilds and macOS remembers privacy grants (for example removable-volume access) instead of
//! prompting after every build. Not for distribution. The ports of `scripts/macos/dev-signing.sh` and `build-app.sh`.

use crate::{command, run, Step};
use std::path::{Path, PathBuf};

/// The identity's name (the certificate's common name).
pub const NAME: &str = "Debrief Local Dev";

/// The login keychain of `home`.
pub fn keychain(home: &Path) -> PathBuf {
    home.join("Library/Keychains/login.keychain-db")
}

/// True when `security find-identity -v -p codesigning` output lists the identity.
pub fn has_identity(listing: &str) -> bool {
    listing.contains(&format!("\"{NAME}\""))
}

/// The `openssl req` step: a 10-year self-signed code-signing certificate and key.
pub fn req_step(dir: &Path) -> Step {
    let (key, cert) = (dir.join("key.pem"), dir.join("cert.pem"));
    Step::new(
        "openssl",
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "3650",
            "-keyout",
            &key.to_string_lossy(),
            "-out",
            &cert.to_string_lossy(),
            "-subj",
            &format!("/CN={NAME}"),
            "-addext",
            "basicConstraints=critical,CA:false",
            "-addext",
            "keyUsage=critical,digitalSignature",
            "-addext",
            "extendedKeyUsage=critical,codeSigning",
        ],
    )
}

/// The `openssl pkcs12` step. macOS `security` only reads PKCS#12 with legacy (SHA1/3DES) encryption.
pub fn pkcs12_step(dir: &Path, pass: &str) -> Step {
    Step::new(
        "openssl",
        &[
            "pkcs12",
            "-export",
            "-inkey",
            &dir.join("key.pem").to_string_lossy(),
            "-in",
            &dir.join("cert.pem").to_string_lossy(),
            "-name",
            NAME,
            "-keypbe",
            "PBE-SHA1-3DES",
            "-certpbe",
            "PBE-SHA1-3DES",
            "-macalg",
            "sha1",
            "-passout",
            &format!("pass:{pass}"),
            "-out",
            &dir.join("debrief.p12").to_string_lossy(),
        ],
    )
    .with_secret(pass)
}

/// The import (`-T` lets codesign use the key without a keychain prompt) and the trust steps.
pub fn import_steps(dir: &Path, keychain: &Path, pass: &str) -> Vec<Step> {
    let kc = keychain.to_string_lossy().into_owned();
    vec![
        Step::new(
            "security",
            &[
                "import",
                &dir.join("debrief.p12").to_string_lossy(),
                "-k",
                &kc,
                "-P",
                pass,
                "-T",
                "/usr/bin/codesign",
            ],
        )
        .with_secret(pass),
        Step::new(
            "security",
            &[
                "add-trusted-cert",
                "-r",
                "trustRoot",
                "-p",
                "codeSign",
                "-k",
                &kc,
                &dir.join("cert.pem").to_string_lossy(),
            ],
        ),
    ]
}

/// A random 32-hex-digit password for the one-off PKCS#12 file, from the OS (`/dev/urandom` on macOS).
fn random_pass() -> Result<String, String> {
    use std::io::Read;
    let mut b = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut b))
        .map_err(|e| format!("no random source: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

fn output_of(step: &Step) -> Result<String, String> {
    let o = command(&step.program)
        .args(&step.args)
        .output()
        .map_err(|e| format!("could not start `{}`: {e}", step.program))?;
    Ok(String::from_utf8_lossy(&o.stdout).into_owned())
}

fn home() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_string())
}

fn listing(kc: &Path) -> Result<String, String> {
    output_of(&Step::new(
        "security",
        &["find-identity", "-v", "-p", "codesigning", &kc.to_string_lossy()],
    ))
}

/// `cargo xtask dev-sign [--remove]`.
pub fn dev_sign(args: &[String], root: &Path) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        eprintln!("macOS only");
        return Ok(());
    }
    let kc = keychain(&home()?);
    match args {
        [a] if a == "--remove" => {
            while command("security")
                .args(["find-certificate", "-c", NAME, &kc.to_string_lossy()])
                .output()
                .is_ok_and(|o| o.status.success())
            {
                let by_identity = run(
                    &Step::new("security", &["delete-identity", "-c", NAME, &kc.to_string_lossy()]),
                    root,
                );
                if by_identity.is_err() {
                    run(
                        &Step::new("security", &["delete-certificate", "-c", NAME, &kc.to_string_lossy()]),
                        root,
                    )?;
                }
            }
            eprintln!("Removed \"{NAME}\".");
            return Ok(());
        }
        [] => {}
        _ => return Err("usage: cargo xtask dev-sign [--remove]".to_string()),
    }
    if has_identity(&listing(&kc)?) {
        eprintln!("Signing identity \"{NAME}\" already present.");
        return Ok(());
    }
    let dir = private_dir()?;
    let result = create(&dir, &kc, root);
    let _ = std::fs::remove_dir_all(&dir);
    result?;
    if has_identity(&listing(&kc)?) {
        eprintln!("Created signing identity \"{NAME}\".");
        Ok(())
    } else {
        Err("Certificate imported but not yet valid for code signing; check Keychain Access.".to_string())
    }
}

fn create(dir: &Path, kc: &Path, root: &Path) -> Result<(), String> {
    let pass = random_pass()?;
    run(&req_step(dir), root)?;
    run(&pkcs12_step(dir, &pass), root)?;
    let steps = import_steps(dir, kc, &pass);
    run(&steps[0], root)?;
    eprintln!("Trusting the certificate for code signing (macOS asks for your password once)...");
    run(&steps[1], root)
}

fn private_dir() -> Result<PathBuf, String> {
    let base = ["TMPDIR", "TEMP", "TMP"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .ok_or("no temp folder is set (TMPDIR); set it to a scratch folder")?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    let dir = Path::new(&base).join(format!("debrief-sign-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|e| format!("cannot protect {}: {e}", dir.display()))?;
    Ok(dir)
}

/// The `tauri build` step of `build-app`, with the signing identity when one is present and none is set.
pub fn build_app_step(extra: &[String], identity: Option<&str>) -> Step {
    let mut args = vec!["tauri", "build"];
    args.extend(extra.iter().map(String::as_str));
    let step = Step::new("npx", &args);
    match identity {
        Some(i) => step.with_env("APPLE_SIGNING_IDENTITY", i),
        None => step,
    }
}

/// `cargo xtask build-app [tauri build arguments]`: a local release build; on macOS signed with the local identity when present.
pub fn build_app(args: &[String], root: &Path) -> Result<(), String> {
    let mut identity = None;
    if cfg!(target_os = "macos") && std::env::var_os("APPLE_SIGNING_IDENTITY").is_none_or(|v| v.is_empty()) {
        let shown = output_of(&Step::new("security", &["find-identity", "-v", "-p", "codesigning"])).unwrap_or_default();
        if has_identity(&shown) {
            eprintln!("Signing with \"{NAME}\".");
            identity = Some(NAME);
        }
    }
    run(&build_app_step(args, identity), &root.join("app"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_identity_is_found_by_its_quoted_name_only() {
        assert!(has_identity("  1) ABCD \"Debrief Local Dev\" (CSSMERR_TP_CERT_EXPIRED)\n"));
        assert!(!has_identity("  1) ABCD \"Debrief Local Dev 2\"\n"));
        assert!(!has_identity(""));
    }

    #[test]
    fn the_certificate_is_a_ten_year_code_signing_one_in_the_given_folder() {
        let s = req_step(Path::new("/t"));
        let line = s.args.join(" ");
        assert!(
            line.contains("-days 3650")
                && line.contains("-subj /CN=Debrief Local Dev")
                && line.contains("extendedKeyUsage=critical,codeSigning")
        );
        assert!(line.contains("-keyout /t/key.pem") && line.contains("-out /t/cert.pem"));
        let p = pkcs12_step(Path::new("/t"), "pw").args.join(" ");
        assert!(
            p.contains("-keypbe PBE-SHA1-3DES")
                && p.contains("-macalg sha1")
                && p.contains("-passout pass:pw")
                && p.contains("-out /t/debrief.p12")
        );
    }

    #[test]
    fn the_import_lets_codesign_use_the_key_and_trusts_the_certificate_for_code_signing() {
        let kc = keychain(Path::new("/h"));
        assert_eq!(kc, Path::new("/h/Library/Keychains/login.keychain-db"));
        let steps = import_steps(Path::new("/t"), &kc, "pw");
        assert_eq!(
            steps[0].args.join(" "),
            "import /t/debrief.p12 -k /h/Library/Keychains/login.keychain-db -P pw -T /usr/bin/codesign"
        );
        assert_eq!(
            steps[1].args.join(" "),
            "add-trusted-cert -r trustRoot -p codeSign -k /h/Library/Keychains/login.keychain-db /t/cert.pem"
        );
    }

    #[test]
    fn the_pkcs12_password_is_masked_in_the_echoed_commands() {
        let kc = keychain(Path::new("/h"));
        let shown = [
            pkcs12_step(Path::new("/t"), "s3cr3tpw").display(),
            import_steps(Path::new("/t"), &kc, "s3cr3tpw")[0].display(),
        ];
        for line in shown {
            assert!(!line.contains("s3cr3tpw") && line.contains("***"), "{line}");
        }
    }

    #[test]
    fn the_build_passes_its_arguments_on_and_signs_only_with_an_identity() {
        let a = vec!["--bundles".to_string(), "app".to_string()];
        let signed = build_app_step(&a, Some(NAME));
        assert_eq!(
            signed.display(),
            "APPLE_SIGNING_IDENTITY=Debrief Local Dev npx tauri build --bundles app"
        );
        assert!(build_app_step(&a, None).env.is_empty());
    }
}
