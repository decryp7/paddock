//! Keep-warm files (ADR 0019): a preset with a long fixed system prompt may have its processed prompt prefix saved
//! with llama-server's slot save (`--slot-save-path`, `<data>/slots/`) when the model unloads and restored when the
//! model loads again. The files are bounded (total size, age) and named so that changes to the model, the preset or the
//! engine invalidate them: `<model hash>.<alias>.<digest of fingerprint and prompt>.bin`.

use pitwall_core::time::Millis;
use sha2::{Digest, Sha256};
use std::path::Path;

/// The most bytes all saved prefixes may use; the oldest go first.
pub const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DAY_MS: Millis = 24 * 3600 * 1000;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The prefix every file of `model` starts with.
pub fn model_prefix(model: &str) -> String {
    format!("{}.", hex(&Sha256::digest(model.as_bytes())[..4]))
}

/// The prefix every file of one preset on `model` starts with.
pub fn preset_prefix(model: &str, alias: &str) -> String {
    format!("{}{alias}.", model_prefix(model))
}

/// The file for a preset's prefix: changes when the fingerprint (model file, engine, build) or the system prompt change.
pub fn file_name(model: &str, alias: &str, fingerprint: &str, system_prompt: &str) -> String {
    let mut h = Sha256::new();
    h.update(fingerprint.as_bytes());
    h.update([0]);
    h.update(system_prompt.as_bytes());
    format!("{}{}.bin", preset_prefix(model, alias), hex(&h.finalize()[..8]))
}

/// Deletes the files whose names start with `prefix` except `keep`; returns how many went.
pub fn drop_prefix(dir: &Path, prefix: &str, keep: Option<&str>) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut n = 0;
    for e in rd.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with(prefix) && Some(name.as_str()) != keep && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

/// Deletes every file of a preset, whatever the model (the preset was changed or deleted).
pub fn drop_alias(dir: &Path, alias: &str) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut n = 0;
    for e in rd.filter_map(Result::ok) {
        let name = e.file_name().to_string_lossy().into_owned();
        let of_alias = name
            .get(9..)
            .and_then(|rest| rest.strip_prefix(alias))
            .is_some_and(|rest| rest.starts_with('.') && rest.len() == ".".len() + 16 + ".bin".len());
        if of_alias && std::fs::remove_file(e.path()).is_ok() {
            n += 1;
        }
    }
    n
}

/// Deletes files unused for `max_days`, then the oldest until the total is within [`MAX_TOTAL_BYTES`]. Returns how many
/// went. At most `max` files are examined.
pub fn sweep(dir: &Path, max_days: u32, now: Millis, max: usize) -> u64 {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let mut files: Vec<(Millis, u64, std::path::PathBuf)> = rd
        .filter_map(Result::ok)
        .take(max)
        .filter_map(|e| {
            let m = e.metadata().ok().filter(std::fs::Metadata::is_file)?;
            let t = m
                .modified()
                .ok()?
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as Millis);
            Some((t, m.len(), e.path()))
        })
        .collect();
    files.sort();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    let mut n = 0;
    for (t, size, path) in files {
        let old = now.saturating_sub(t) > u64::from(max_days) * DAY_MS;
        if (old || total > MAX_TOTAL_BYTES) && std::fs::remove_file(&path).is_ok() {
            total = total.saturating_sub(size);
            n += 1;
        }
    }
    n
}

/// Files and bytes held.
pub fn usage(dir: &Path) -> (u64, u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return (0, 0) };
    rd.filter_map(Result::ok)
        .filter_map(|e| e.metadata().ok().filter(std::fs::Metadata::is_file))
        .fold((0, 0), |a, m| (a.0 + 1, a.1 + m.len()))
}

/// Marks a file as used now (so "unused for N days" counts from the last restore).
pub fn touch(path: &Path) {
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_change_with_model_preset_fingerprint_and_prompt() {
        let a = file_name("o/r:q", "fast", "fp1", "be brief");
        assert!(a.starts_with(&preset_prefix("o/r:q", "fast")) && a.ends_with(".bin") && !a.contains('/'));
        assert_ne!(a, file_name("o/r:q", "fast", "fp2", "be brief"));
        assert_ne!(a, file_name("o/r:q", "fast", "fp1", "be long"));
        assert_ne!(a, file_name("o/r:q", "slow", "fp1", "be brief"));
        assert!(!file_name("other", "fast", "fp1", "be brief").starts_with(&model_prefix("o/r:q")));
    }

    #[test]
    fn invalidation_keeps_only_the_current_file() {
        let d = tempfile::tempdir().unwrap();
        let (old, new) = (file_name("m", "fast", "fp1", "p"), file_name("m", "fast", "fp2", "p"));
        let other = file_name("m", "slow", "fp1", "p");
        for f in [&old, &new, &other] {
            std::fs::write(d.path().join(f), b"x").unwrap();
        }
        assert_eq!(drop_prefix(d.path(), &preset_prefix("m", "fast"), Some(&new)), 1);
        assert!(!d.path().join(&old).exists() && d.path().join(&new).exists() && d.path().join(&other).exists());
        let aliased = file_name("zz", "fast", "fp", "p");
        std::fs::write(d.path().join(&aliased), b"x").unwrap();
        std::fs::write(d.path().join("notes.txt"), b"x").unwrap();
        assert_eq!(drop_alias(d.path(), "fast"), 2, "the alias on any model, and only it");
        assert!(d.path().join(&other).exists() && d.path().join("notes.txt").exists());
        assert_eq!(drop_alias(&d.path().join("missing"), "fast"), 0);
        assert_eq!(drop_prefix(d.path(), &model_prefix("m"), None), 1);
        assert_eq!(usage(d.path()), (1, 1), "only the unrelated file is left");
        assert_eq!(drop_prefix(&d.path().join("missing"), "x", None), 0);
    }

    #[test]
    fn the_sweep_drops_files_unused_for_too_long_and_stays_within_the_cap() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a.bin"), d.path().join("b.bin"));
        std::fs::write(&a, vec![0; 10]).unwrap();
        std::fs::write(&b, vec![0; 20]).unwrap();
        assert_eq!(usage(d.path()), (2, 30));
        let now = crate::paths::now_ms();
        assert_eq!(sweep(d.path(), 30, now, 100), 0, "fresh files stay");
        let later = now + 31 * DAY_MS;
        touch(&b);
        assert_eq!(sweep(d.path(), 30, later, 100), 2, "unused for more than 30 days");
        assert_eq!(sweep(&d.path().join("missing"), 30, now, 100), 0);
        std::fs::write(&a, b"x").unwrap();
        assert_eq!(sweep(d.path(), 30, now, 0), 0, "bounded per pass");
    }
}
