//! An example Dyno plugin, and the reference for writing one (see docs/plugins.md):
//!
//! - a sensor, "Dyno example: seconds since start";
//! - a cleanup category, "Example: old .tmp files": `*.tmp` files older than 7 days in the folder named by the
//!   environment variable `DYNO_EXAMPLE_TMP_DIR` (none by default, so it proposes nothing until configured).
//!
//! The plugin only proposes candidates; Dyno checks each against its cleanup safety policy and deletes them itself.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use dyno_plugin_api::{
    Candidate, CategoryInfo, CleanupProvider, Contribution, Manifest, Plugin, PluginError, Risk, ScanLimits, SensorProvider, SensorReading,
};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The cleanup category's id.
const CATEGORY: &str = "old-tmp";
/// Files older than this are proposed.
const MAX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

struct Example {
    started: Instant,
    /// Where to look for old `.tmp` files, if configured.
    tmp_dir: Option<PathBuf>,
}

impl Plugin for Example {
    fn manifest(&self) -> Manifest {
        Manifest::new("example", "Dyno example", env!("CARGO_PKG_VERSION"))
            .contributing(Contribution::Sensors)
            .contributing(Contribution::Cleanup)
    }

    fn as_sensors(&self) -> Option<&dyn SensorProvider> {
        Some(self)
    }

    fn as_cleanup(&self) -> Option<&dyn CleanupProvider> {
        Some(self)
    }
}

impl SensorProvider for Example {
    fn sensors(&self) -> Vec<SensorReading> {
        vec![SensorReading::new(
            "uptime",
            "Dyno example: seconds since start",
            self.started.elapsed().as_secs_f64().floor(),
            "s",
        )]
    }
}

impl CleanupProvider for Example {
    fn categories(&self) -> Vec<CategoryInfo> {
        vec![CategoryInfo {
            id: CATEGORY.into(),
            name: "Example: old .tmp files".into(),
            description: "*.tmp files older than 7 days in the folder set in DYNO_EXAMPLE_TMP_DIR".into(),
            risk: Risk::Safe,
        }]
    }

    fn scan(&self, _category: &str, limits: ScanLimits) -> Result<Vec<Candidate>, PluginError> {
        let Some(dir) = &self.tmp_dir else {
            return Ok(Vec::new());
        };
        if !dir.is_dir() {
            return Err(PluginError::new(format!(
                "{} is not a folder: fix DYNO_EXAMPLE_TMP_DIR",
                dir.display()
            )));
        }
        let mut found = Vec::new();
        walk(dir, limits.max_depth, SystemTime::now(), limits.max_candidates as usize, &mut found);
        Ok(found)
    }
}

/// Collects old `.tmp` files below `dir`, at most `depth` levels down and `max` in all. Links are not followed;
/// unreadable entries are skipped.
fn walk(dir: &Path, depth: u32, now: SystemTime, max: usize, out: &mut Vec<Candidate>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        if out.len() >= max {
            return;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let path = entry.path();
        if meta.is_dir() && !meta.file_type().is_symlink() {
            if depth > 0 {
                walk(&path, depth - 1, now, max, out);
            }
            continue;
        }
        let is_tmp = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("tmp"));
        let modified = meta.modified().unwrap_or(now);
        let age = now.duration_since(modified).unwrap_or_default();
        if meta.is_file() && is_tmp && age > MAX_AGE {
            out.push(Candidate {
                path: path.display().to_string(),
                bytes: meta.len(),
                modified_ms: modified.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0),
                reason: format!("a temporary file untouched for {} days", age.as_secs() / 86_400),
            });
        }
    }
}

fn main() -> std::process::ExitCode {
    dyno_plugin_api::serve(Example {
        started: Instant::now(),
        tmp_dir: std::env::var_os("DYNO_EXAMPLE_TMP_DIR").map(PathBuf::from),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn example(dir: Option<PathBuf>) -> Example {
        Example {
            started: Instant::now(),
            tmp_dir: dir,
        }
    }

    fn old_file(path: &Path) {
        let f = File::create(path).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(30 * 24 * 3600)).unwrap();
    }

    #[test]
    fn it_describes_itself_and_reports_its_uptime() {
        let e = example(None);
        let m = e.manifest();
        assert_eq!(m.id, "example");
        assert!(m.contributes(Contribution::Sensors) && m.contributes(Contribution::Cleanup));
        let s = e.sensors();
        assert_eq!(s[0].id, "uptime");
        assert!(s[0].value >= 0.0);
        assert_eq!(e.categories()[0].id, CATEGORY);
        assert!(e.as_sensors().is_some() && e.as_cleanup().is_some() && e.as_actions().is_none());
    }

    #[test]
    fn without_a_folder_it_proposes_nothing() {
        assert!(example(None).scan(CATEGORY, ScanLimits::default()).unwrap().is_empty());
        let missing = example(Some(PathBuf::from("/no/such/dyno/folder")));
        assert!(missing
            .scan(CATEGORY, ScanLimits::default())
            .unwrap_err()
            .message
            .contains("DYNO_EXAMPLE_TMP_DIR"));
    }

    #[test]
    fn only_old_tmp_files_within_the_limits_are_proposed() {
        let t = tempfile::tempdir().unwrap();
        old_file(&t.path().join("old.tmp"));
        old_file(&t.path().join("OLD2.TMP"));
        old_file(&t.path().join("keep.txt"));
        File::create(t.path().join("new.tmp")).unwrap();
        std::fs::create_dir_all(t.path().join("a").join("b")).unwrap();
        old_file(&t.path().join("a").join("nested.tmp"));
        old_file(&t.path().join("a").join("b").join("deep.tmp"));
        let e = example(Some(t.path().to_path_buf()));
        let scan = |max_depth, max_candidates| {
            let mut names: Vec<String> = e
                .scan(CATEGORY, ScanLimits { max_candidates, max_depth })
                .unwrap()
                .into_iter()
                .map(|c| Path::new(&c.path).file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            names.sort();
            names
        };
        assert_eq!(scan(0, 100), ["OLD2.TMP", "old.tmp"]);
        assert_eq!(scan(1, 100), ["OLD2.TMP", "nested.tmp", "old.tmp"]);
        assert_eq!(scan(8, 100).len(), 4);
        assert_eq!(scan(8, 2).len(), 2);
        let c = &e.scan(CATEGORY, ScanLimits::default()).unwrap()[0];
        assert!(c.reason.contains("days") && c.modified_ms > 0);
    }
}
