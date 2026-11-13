//! Finds developer project folders in the user's profile (folders holding `Cargo.toml`, `package.json` or a `*.sln`),
//! bounded in depth, folders visited, results and time.

use dyno_core::facts::DevFolder;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Limits of one scan.
#[derive(Debug, Clone, Copy)]
pub struct ScanLimits {
    /// Levels below the root (the root is depth 0).
    pub max_depth: usize,
    /// Folders read at most.
    pub max_visited: usize,
    /// Projects reported at most.
    pub max_results: usize,
    /// Wall-clock budget.
    pub budget: Duration,
}

impl Default for ScanLimits {
    fn default() -> Self {
        ScanLimits {
            max_depth: 3,
            max_visited: 20_000,
            max_results: 200,
            budget: Duration::from_secs(3),
        }
    }
}

/// Folders not worth descending into: hidden ones, OS data, build output and cloud-sync folders (OneDrive).
fn skipped(name: &str) -> bool {
    // OneDrive folders hold placeholders: listing one makes the cloud client fetch the directory listing (Windows "Files On-Demand");
    // the file contents stay in the cloud. Only "OneDrive" and "OneDrive - <account>" are cloud folders, not "onedrive-cli".
    let lower = name.to_ascii_lowercase();
    name.starts_with('.')
        || lower == "onedrive"
        || lower.starts_with("onedrive - ")
        || matches!(
            lower.as_str(),
            "appdata" | "library" | "node_modules" | "target" | "bin" | "obj" | "dist" | "build" | "applications" | "$recycle.bin"
        )
}

fn is_project(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().take(5_000).any(|e| {
        let name = e.file_name();
        let name = name.to_string_lossy();
        name == "Cargo.toml" || name == "package.json" || name.to_ascii_lowercase().ends_with(".sln")
    })
}

/// Project folders under `root`, outermost first: a project's own subfolders are not reported again.
pub fn scan(root: &Path, limits: ScanLimits) -> Vec<DevFolder> {
    let started = Instant::now();
    let mut out = Vec::new();
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = [(root.to_path_buf(), 0)].into();
    let mut visited = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        if visited >= limits.max_visited || out.len() >= limits.max_results || started.elapsed() > limits.budget {
            break;
        }
        visited += 1;
        if depth > 0 && is_project(&dir) {
            out.push(DevFolder {
                path: dir.to_string_lossy().into_owned(),
                indexed: None,
                av_excluded: None,
            });
            continue;
        }
        if depth >= limits.max_depth {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            // Never follow links (junctions / symlinks) out of the profile.
            let Ok(ft) = e.file_type() else { continue };
            if !ft.is_dir() || ft.is_symlink() || skipped(&e.file_name().to_string_lossy()) {
                continue;
            }
            queue.push_back((e.path(), depth + 1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_sync_folders_are_not_walked() {
        let dir = paddock_testkit::private_tempdir().unwrap();
        for rel in ["OneDrive/proj", "OneDrive - Contoso/proj", "onedrive/proj", "real/proj"] {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("Cargo.toml"), "").unwrap();
        }
        let found = scan(dir.path(), ScanLimits::default());
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].path.replace('\\', "/").ends_with("/real/proj"));
        assert!(skipped("OneDrive - Fabrikam") && !skipped("my-onedrive-notes"));
        assert!(skipped("OneDrive") && skipped("ONEDRIVE") && !skipped("onedrive-cli") && !skipped("OneDriveSync"));
    }

    #[test]
    fn finds_projects_within_limits() {
        let dir = paddock_testkit::private_tempdir().unwrap();
        let root = dir.path();
        let mk = |rel: &str, file: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join(file), "").unwrap();
        };
        mk("dev/rusty", "Cargo.toml");
        mk("dev/rusty/sub", "Cargo.toml");
        mk("dev/web", "package.json");
        mk("src/app", "App.SLN");
        mk("a/b/c/d/deep", "Cargo.toml");
        mk(".hidden/proj", "Cargo.toml");
        mk("dev/node_modules/x", "package.json");
        mk("notes", "readme.txt");
        let mut found: Vec<String> = scan(root, ScanLimits::default())
            .into_iter()
            .map(|f| f.path.strip_prefix(&*root.to_string_lossy()).unwrap_or(&f.path).replace('\\', "/"))
            .collect();
        found.sort();
        assert_eq!(found, ["/dev/rusty", "/dev/web", "/src/app"]);
        let one = scan(
            root,
            ScanLimits {
                max_results: 1,
                ..ScanLimits::default()
            },
        );
        assert_eq!(one.len(), 1);
        let none = scan(
            root,
            ScanLimits {
                max_visited: 1,
                ..ScanLimits::default()
            },
        );
        assert!(none.is_empty());
        assert!(scan(&root.join("missing"), ScanLimits::default()).is_empty());
    }
}
