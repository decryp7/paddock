//! The diagnostics bundle: a zip of the service's redacted sections (configuration, status, history, the tail of
//! each runner's log, the service log) plus the app's own facts. No tokens, no `.runner` file (Pitcrew never reads
//! it), and every text is redacted once more here before it is written.

use pitcrew_core::protocol::DiagnosticsSection;
use pitcrew_core::redact::redact;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Writes the bundle into `dir` and returns its path.
pub fn write_bundle(dir: &Path, stamp: &str, sections: &[DiagnosticsSection]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("pitcrew-diagnostics-{stamp}.zip"));
    let file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for s in sections {
        // A name is a plain file name; anything else is flattened.
        let name: String = s
            .name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' })
            .collect();
        let name = name.trim_start_matches('.');
        zip.start_file(name, options).map_err(|e| e.to_string())?;
        zip.write_all(redact(&s.text).as_bytes()).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    #[allow(
        clippy::disallowed_methods,
        reason = "a test builds its scratch path from the OS temp folder and removes it"
    )]
    fn bundles_are_redacted_zips_with_safe_names() {
        let dir = std::env::temp_dir().join(format!("pitcrew-it-diag-{}", std::process::id()));
        let sections = vec![
            DiagnosticsSection {
                name: "../evil/config.toml".into(),
                text: "token=supersecret\nname=x".into(),
            },
            DiagnosticsSection {
                name: "app.txt".into(),
                text: "ok".into(),
            },
        ];
        let p = write_bundle(&dir, "20261022", &sections).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&p).unwrap()).unwrap();
        let mut names: Vec<String> = (0..z.len()).map(|i| z.by_index(i).unwrap().name().unwrap().to_string()).collect();
        names.sort();
        assert_eq!(names, vec!["_evil_config.toml", "app.txt"]);
        let mut text = String::new();
        z.by_name("_evil_config.toml").unwrap().read_to_string(&mut text).unwrap();
        assert!(!text.contains("supersecret"), "{text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
