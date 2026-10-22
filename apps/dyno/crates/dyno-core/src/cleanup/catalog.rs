//! The cleanup categories Dyno knows. Each names symbolic roots the agent resolves per OS, an age filter and a risk.

use serde::{Deserialize, Serialize};

/// How a category is presented and grouped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupGroup {
    /// OS temp files, caches, dumps, logs, update leftovers.
    System,
    /// Browser caches.
    Browser,
    /// Developer build output and package caches.
    Developer,
    /// The Downloads folder.
    Downloads,
    /// Docker images.
    Docker,
    /// From a plugin.
    Plugin,
}

/// How careful the user must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupRisk {
    /// Regenerated automatically; pre-selected.
    Safe,
    /// Regenerated, but costs time or a download (build output, package caches); the user selects it.
    Review,
    /// The user's own files (Downloads); never pre-selected, never scheduled without asking.
    Personal,
}

/// An OS a category applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Os {
    /// Windows only.
    Windows,
    /// macOS only.
    Macos,
    /// Linux (XDG folders).
    Linux,
}

/// A symbolic place the agent resolves to folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Root {
    /// The user's temp folder (%TEMP%, $TMPDIR).
    UserTemp,
    /// C:\Windows\Temp (mostly needs the helper; what can't be deleted is reported).
    SystemTemp,
    /// Windows Update's download cache (C:\Windows\SoftwareDistribution\Download).
    UpdateDownloads,
    /// Crash dumps (%LOCALAPPDATA%\CrashDumps, ~/Library/Logs/DiagnosticReports).
    CrashDumps,
    /// Windows Error Reporting archives (%LOCALAPPDATA%\Microsoft\Windows\WER).
    ErrorReports,
    /// Thumbnail and icon caches.
    ThumbnailCache,
    /// Per-user app caches (~/Library/Caches).
    UserCaches,
    /// The user's log folders (~/Library/Logs).
    UserLogs,
    /// Chrome's caches (every profile).
    ChromeCache,
    /// Edge's caches.
    EdgeCache,
    /// Firefox's caches.
    FirefoxCache,
    /// Safari's caches.
    SafariCache,
    /// The Downloads folder.
    Downloads,
    /// Project folders found under the user's profile (see [`super::policy::dev_artifact`]).
    Projects,
    /// ~/.cargo/registry/{cache,src} and ~/.cargo/git/checkouts.
    CargoCache,
    /// npm's cache.
    NpmCache,
    /// NuGet's global packages and HTTP cache.
    NugetCache,
    /// pip's cache.
    PipCache,
    /// ~/.rustup/toolchains (only toolchains no project uses, see [`super::policy::stale_toolchains`]).
    RustToolchains,
    /// Docker's dangling images (removed with the Docker CLI, not as files).
    DockerImages,
}

/// What to pick inside a root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Selector {
    /// Every entry directly inside the root (files and folders, each as one item).
    Children,
    /// Installer files (.exe, .msi, .msix, .dmg, .pkg) directly inside the root.
    Installers,
    /// Build output folders (`target`, `node_modules`) of projects untouched for the dev-project age.
    DevArtifacts,
    /// Toolchains no default or override uses.
    StaleToolchains,
    /// Images reported by the Docker CLI.
    DockerImages,
}

/// Where a category looks: a named place or a path template from a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootRef {
    /// A place Dyno resolves per OS.
    Named(Root),
    /// A pack's path template (`{home}/.cache/app`), expanded by the agent (see `pack::valid_template`).
    Template(String),
}

/// A category, from the built-in pack or a user's pack.
#[derive(Debug, Clone, PartialEq)]
pub struct CategorySpec {
    /// Stable id.
    pub id: String,
    /// Name.
    pub name: String,
    /// One sentence: what it is and what deleting it costs.
    pub description: String,
    /// Group.
    pub group: CleanupGroup,
    /// Risk.
    pub risk: CleanupRisk,
    /// Which OSes (empty: all).
    pub os: Vec<Os>,
    /// Where.
    pub roots: Vec<RootRef>,
    /// What inside.
    pub selector: Selector,
    /// Only items not modified for this many days (0: any age).
    pub min_age_days: u32,
    /// From the app (true) or a user's pack (false).
    pub builtin: bool,
}

impl CategorySpec {
    /// Whether the category applies to `os`.
    pub fn applies_to(&self, os: Os) -> bool {
        self.os.is_empty() || self.os.contains(&os)
    }
}

/// Every built-in category (the embedded pack, `builtin.toml`).
pub fn catalog() -> Vec<CategorySpec> {
    super::pack::builtin()
}

/// Categories for one OS.
pub fn for_os(os: Os) -> Vec<CategorySpec> {
    catalog().into_iter().filter(|c| c.applies_to(os)).collect()
}

/// The UI's view of a category.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupCategory {
    /// Id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Description.
    pub description: String,
    /// Group.
    pub group: CleanupGroup,
    /// Risk.
    pub risk: CleanupRisk,
    /// Whether scheduled runs include it.
    pub scheduled: bool,
}

impl CategorySpec {
    /// The UI's view, `scheduled` from the user's settings (never for personal categories).
    pub fn view(&self, scheduled: bool) -> CleanupCategory {
        CleanupCategory {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            group: self.group,
            risk: self.risk,
            scheduled: scheduled && self.risk != CleanupRisk::Personal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_consistent() {
        let all = catalog();
        let mut ids: Vec<_> = all.iter().map(|c| c.id.as_str()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "ids are unique");
        assert!(all.iter().all(|c| !c.roots.is_empty() && !c.description.is_empty() && c.builtin));
        assert!(for_os(Os::Windows)
            .iter()
            .all(|c| c.applies_to(Os::Windows) && c.id != "safariCache"));
        assert!(for_os(Os::Macos).iter().any(|c| c.id == "safariCache"));
        assert!(for_os(Os::Linux).iter().any(|c| c.id == "appCaches"), "Linux has categories");
        assert!(for_os(Os::Linux).iter().all(|c| c.id != "systemTemp" && c.id != "safariCache"));
        let d = all.iter().find(|c| c.id == "downloadInstallers").unwrap();
        assert_eq!(d.risk, CleanupRisk::Personal);
        assert!(!d.view(true).scheduled, "personal files are never scheduled");
        assert!(all[0].view(true).scheduled);
        assert!(!all[0].view(false).scheduled);
    }
}
