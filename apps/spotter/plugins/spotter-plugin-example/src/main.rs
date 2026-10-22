//! An example Spotter plugin: releases listed in a manifest file that a build system writes, one release per line.
//!
//! ```text
//! # version    location                                  published (optional, RFC 3339)
//! 1.4.2        \\builds\drops\MyApp-1.4.2.msi           2026-10-22T08:00:00Z
//! 1.5.0-rc.1   https://dl.example.org/MyApp-1.5.0-rc.1.msi
//! ```
//!
//! Columns are separated by whitespace or commas (`format = csv`). It shows the whole plugin contract: a manifest
//! with a configuration form (a folder-ish path, a choice, a toggle), typed errors, and limits on what it reads.

use spotter_plugin_api::{
    serve, ChoiceOption, Config, ErrorKind, FieldKind, FieldSpec, Manifest, Plugin, PluginError, SourceKindSpec, WireRelease,
};
use std::process::ExitCode;

/// Largest manifest read (1 MiB).
const MAX_MANIFEST: u64 = 1 << 20;

struct ManifestFile;

impl Plugin for ManifestFile {
    fn manifest(&self) -> Manifest {
        Manifest::new("manifest-file", "Manifest file", env!("CARGO_PKG_VERSION")).with_kind(
            SourceKindSpec::new(
                "lines",
                "Release manifest",
                "A text file a build system writes, one release per line.",
            )
            .with_field(
                FieldSpec::new("path", "Manifest file", FieldKind::Text)
                    .required()
                    .placeholder(r"\\builds\drops\releases.txt")
                    .help("A local path or a path on a share."),
            )
            .with_field(
                FieldSpec::new(
                    "format",
                    "Format",
                    FieldKind::Choice(vec![
                        ChoiceOption::new("lines", "Columns separated by spaces"),
                        ChoiceOption::new("csv", "Comma-separated"),
                    ]),
                )
                .default_value("lines"),
            )
            .with_field(FieldSpec::new(
                "prerelease_marker",
                "Lines starting with ! are pre-releases",
                FieldKind::Toggle,
            )),
        )
    }

    fn list(&self, kind: &str, config: &Config, limit: usize) -> Result<Vec<WireRelease>, PluginError> {
        if kind != "lines" {
            return Err(PluginError::new(
                ErrorKind::Unsupported,
                format!("This plugin has no source kind \"{kind}\"."),
            ));
        }
        let path = config.require("path").map_err(|e| PluginError::new(ErrorKind::InvalidConfig, e))?;
        let meta = std::fs::metadata(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => PluginError::new(ErrorKind::Unreachable, format!("{path} is not there (yet).")),
            std::io::ErrorKind::PermissionDenied => PluginError::new(ErrorKind::Unauthorized, format!("No access to {path}.")),
            _ => PluginError::new(ErrorKind::Unreachable, format!("{path} could not be read ({e}).")),
        })?;
        if meta.len() > MAX_MANIFEST {
            return Err(PluginError::new(ErrorKind::Failed, format!("{path} is larger than 1 MiB.")));
        }
        let text = std::fs::read_to_string(path).map_err(|e| PluginError::new(ErrorKind::Failed, format!("{path} is not text ({e}).")))?;
        Ok(parse(
            &text,
            config.get("format") == Some("csv"),
            config.flag("prerelease_marker"),
            limit,
        ))
    }
}

/// The releases in a manifest's text.
fn parse(text: &str, csv: bool, marker: bool, limit: usize) -> Vec<WireRelease> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let (pre, line) = match line.strip_prefix('!') {
                Some(rest) if marker => (true, rest.trim_start()),
                _ => (false, line),
            };
            let cols: Vec<&str> = if csv {
                line.split(',').map(str::trim).collect()
            } else {
                line.split_whitespace().collect()
            };
            let version = *cols.first()?;
            let location = cols.get(1).copied().unwrap_or("");
            let mut r = WireRelease::new(version, version, location);
            r.published_at_ms = cols.get(2).and_then(|d| parse_date(d));
            r.prerelease = pre;
            Some(r)
        })
        .take(limit)
        .collect()
}

/// `YYYY-MM-DDTHH:MM:SSZ` (UTC) as milliseconds; anything else is ignored.
fn parse_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[19] != b'Z' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d, hh, mm, ss) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000)
}

fn main() -> ExitCode {
    serve(ManifestFile)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn manifests_parse() {
        let text = "# header\n1.4.2  \\\\b\\MyApp-1.4.2.msi  2026-10-22T08:00:00Z\n\n! 1.5.0-rc.1 https://dl/x\nbare\n";
        let r = parse(text, false, true, 10);
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].version.as_str(), r[0].location.as_str()), ("1.4.2", "\\\\b\\MyApp-1.4.2.msi"));
        assert_eq!(r[0].published_at_ms, Some(1_792_656_000_000));
        assert!(r[1].prerelease && r[1].version == "1.5.0-rc.1");
        assert_eq!(r[2].location, "");
        assert_eq!(parse(text, false, true, 1).len(), 1);
        assert_eq!(parse("!x", false, false, 5)[0].version, "!x");
        let csv = parse("2.0, https://dl/2 , 2026-01-01T00:00:00Z", true, false, 5);
        assert_eq!(
            (csv[0].location.as_str(), csv[0].published_at_ms),
            ("https://dl/2", Some(1_767_225_600_000))
        );
        for bad in ["2026-13-01T00:00:00Z", "2026-01-01 00:00:00Z", "x"] {
            assert_eq!(parse_date(bad), None);
        }
    }

    #[test]
    fn listing_reads_the_file_and_reports_typed_errors() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("releases.txt");
        let cfg = Config::new().with("path", &path.to_string_lossy());
        assert_eq!(ManifestFile.list("lines", &cfg, 5).unwrap_err().kind, ErrorKind::Unreachable);
        std::fs::write(&path, "1.0 a\n1.1 b\n").unwrap();
        assert_eq!(ManifestFile.list("lines", &cfg, 5).unwrap().len(), 2);
        assert_eq!(ManifestFile.list("other", &cfg, 5).unwrap_err().kind, ErrorKind::Unsupported);
        assert_eq!(
            ManifestFile.list("lines", &Config::new(), 5).unwrap_err().kind,
            ErrorKind::InvalidConfig
        );
        std::fs::write(&path, vec![b'x'; (MAX_MANIFEST + 1) as usize]).unwrap();
        assert_eq!(ManifestFile.list("lines", &cfg, 5).unwrap_err().kind, ErrorKind::Failed);
        std::fs::write(&path, [0xff, 0xfe]).unwrap();
        assert_eq!(ManifestFile.list("lines", &cfg, 5).unwrap_err().kind, ErrorKind::Failed);
        let m = ManifestFile.manifest();
        assert_eq!((m.id.as_str(), m.kinds[0].fields.len()), ("manifest-file", 3));
    }
}
