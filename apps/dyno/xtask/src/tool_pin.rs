//! `verify-tool-pin`: run once per pin bump (never by the app, never in a test). Checks a downloaded tool file against the
//! publisher's OpenPGP signature with the system's gpg, and prints the pack lines (size, sha256, date, fingerprint) for
//! `crates/dyno-agent/tools/tool-pins.toml`.
//!
//! The expected fingerprint is the one the user confirmed against smartmontools.org on 2026-11-03 (key id FF3AEFF5). The next
//! smartmontools release will be signed with a new key: then confirm the new fingerprint with a person, change `EXPECTED` here and
//! in the pack, and record where it was read.

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The confirmed signing-key fingerprint.
const EXPECTED: &str = "0C9577FD2C4CFCB4B9A599640A30812EFF3AEFF5";
const GPG_LIMIT: Duration = Duration::from_secs(60);

/// The first `fpr:` record's fingerprint of `gpg --with-colons --fingerprint` output.
pub fn first_fingerprint(colons: &str) -> Option<String> {
    colons
        .lines()
        .find_map(|l| l.strip_prefix("fpr:").and_then(|r| r.split(':').nth(8)).map(str::to_string))
}

/// The fingerprint in the grouped form the pack uses: groups of four, a space between.
pub fn grouped(fp: &str) -> String {
    fp.as_bytes()
        .chunks(4)
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The pack lines for a verified file.
pub fn pack_lines(size: u64, sha256: &str, date: &str) -> String {
    format!(
        "size = {size}\nsha256 = \"{sha256}\"\nkey_fingerprint = \"{}\"\nverified = \"{date}\"",
        grouped(EXPECTED)
    )
}

/// `YYYY-MM-DD` (UTC) of `secs` since the epoch (the civil-from-days algorithm, as `dev ci build-env` uses).
pub fn date_of(secs: u64) -> String {
    let z = i64::try_from(secs / 86_400).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn tempdir() -> Result<PathBuf, String> {
    let base = ["TMPDIR", "TEMP", "TMP"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .ok_or("::error::no temp folder is set (TMPDIR, TEMP or TMP); set TMPDIR to a scratch folder")?;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    // Short on purpose: gpg-agent's socket lives in this folder and a Unix socket path is limited to about 100 bytes.
    let dir = Path::new(&base).join(format!("pin{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("::error::cannot create {}: {e}", dir.display()))?;
    #[cfg(unix)]
    std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .map_err(|e| format!("::error::cannot protect {}: {e}", dir.display()))?;
    Ok(dir)
}

/// `cargo xtask verify-tool-pin <file> <file.asc> [key.asc]`.
pub fn run_task(args: &[String]) -> Result<(), String> {
    let usage = "usage: cargo xtask verify-tool-pin <file> <file.asc> [key.asc]";
    let (file, sig) = match args {
        [f, s] | [f, s, _] => (f, s),
        _ => return Err(usage.to_string()),
    };
    let default_key = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/tool-pins/smartmontools-signing-key.asc");
    let key = args.get(2).map_or(default_key, PathBuf::from);
    let home = tempdir()?;
    let result = verify(file, sig, &key, &home);
    // gpg starts a gpg-agent daemon for the private home; end it before the home goes, or it outlives the run.
    let h = home.to_string_lossy().into_owned();
    let _ = crate::run("gpgconf", &["--homedir", &h, "--kill", "gpg-agent"], &[], GPG_LIMIT);
    let _ = std::fs::remove_dir_all(&home);
    result
}

fn verify(file: &str, sig: &str, key: &Path, home: &Path) -> Result<(), String> {
    let h = home.to_string_lossy().into_owned();
    let env = [("GNUPGHOME", Some(h.as_str()))];
    let gpg = |a: &[&str]| crate::run("gpg", a, &env, GPG_LIMIT).map_err(|e| format!("::error::{e}; install GnuPG and run this again"));
    let key = key.to_string_lossy().into_owned();
    let imported = gpg(&["--batch", "--quiet", "--import", &key])?;
    if !imported.success {
        return Err(format!("::error::gpg could not import {key}: {}", imported.stderr.trim()));
    }
    let listed = gpg(&["--batch", "--with-colons", "--fingerprint"])?;
    let got = first_fingerprint(&listed.stdout).unwrap_or_default();
    if got != EXPECTED {
        return Err(format!(
            "::error::the key file has fingerprint {got}, not the confirmed {EXPECTED}; do not pin anything from it"
        ));
    }
    // The signature may be made by a key that has expired since (smartmontools' was valid when it signed); gpg says so in its output.
    let verified = gpg(&["--batch", "--verify", sig, file])?;
    let text = format!("{}{}", verified.stdout, verified.stderr);
    print!("{text}");
    if !text.contains("Good signature") {
        return Err(format!("::error::the signature of {file} does not verify"));
    }
    let bytes = std::fs::read(file).map_err(|e| format!("::error::cannot read {file}: {e}"))?;
    let sha: String = Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    println!("\n{}", pack_lines(bytes.len() as u64, &sha, &date_of(now)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_fingerprint_record_is_read_from_the_colon_listing() {
        let out = "tru::1:1\npub:-:4096:1:0A30812EFF3AEFF5:1:::::::scESC:::::::\nfpr:::::::::0C9577FD2C4CFCB4B9A599640A30812EFF3AEFF5:\nfpr:::::::::AAAA:\n";
        assert_eq!(first_fingerprint(out).as_deref(), Some(EXPECTED));
        assert_eq!(first_fingerprint("pub:x\n"), None);
    }

    #[test]
    fn the_pack_lines_have_the_grouped_fingerprint_and_the_script_s_four_keys() {
        let l = pack_lines(7, "ab", "2026-11-16");
        assert_eq!(
            l,
            "size = 7\nsha256 = \"ab\"\nkey_fingerprint = \"0C95 77FD 2C4C FCB4 B9A5 9964 0A30 812E FF3A EFF5\"\nverified = \"2026-11-16\""
        );
    }

    #[test]
    fn the_date_is_the_utc_day() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(1_794_787_200), "2026-11-16");
    }
}
