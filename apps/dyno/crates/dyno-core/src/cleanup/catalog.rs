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

/// The OS a category applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Os {
    /// Windows only.
    Windows,
    /// macOS only.
    Macos,
    /// Both.
    Any,
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

/// A built-in category.
#[derive(Debug, Clone, PartialEq)]
pub struct CategorySpec {
    /// Stable id.
    pub id: &'static str,
    /// Name.
    pub name: &'static str,
    /// One sentence: what it is and what deleting it costs.
    pub description: &'static str,
    /// Group.
    pub group: CleanupGroup,
    /// Risk.
    pub risk: CleanupRisk,
    /// Which OS.
    pub os: Os,
    /// Where.
    pub roots: &'static [Root],
    /// What inside.
    pub selector: Selector,
    /// Only items not modified for this many days (0: any age).
    pub min_age_days: u32,
}

/// Every built-in category.
pub fn catalog() -> Vec<CategorySpec> {
    use CleanupGroup as G;
    use CleanupRisk as R;
    let c = |id, name, description, group, risk, os, roots, selector, min_age_days| CategorySpec {
        id,
        name,
        description,
        group,
        risk,
        os,
        roots,
        selector,
        min_age_days,
    };
    vec![
        c(
            "userTemp",
            "Temporary files",
            "Files apps left in your temp folder. Only files older than two days are offered.",
            G::System,
            R::Safe,
            Os::Any,
            &[Root::UserTemp],
            Selector::Children,
            2,
        ),
        c(
            "systemTemp",
            "System temporary files",
            "Windows' own temp folder. Items Windows still uses are skipped and listed.",
            G::System,
            R::Safe,
            Os::Windows,
            &[Root::SystemTemp],
            Selector::Children,
            2,
        ),
        c(
            "updateLeftovers",
            "Windows Update leftovers",
            "Downloaded update packages already installed. Windows downloads them again if needed.",
            G::System,
            R::Safe,
            Os::Windows,
            &[Root::UpdateDownloads],
            Selector::Children,
            7,
        ),
        c(
            "crashDumps",
            "Crash dumps and reports",
            "Memory dumps and error reports from apps that crashed. Useful only to send to a developer.",
            G::System,
            R::Safe,
            Os::Any,
            &[Root::CrashDumps, Root::ErrorReports],
            Selector::Children,
            3,
        ),
        c(
            "thumbnails",
            "Thumbnail cache",
            "Preview images Explorer rebuilds as you browse folders.",
            G::System,
            R::Safe,
            Os::Windows,
            &[Root::ThumbnailCache],
            Selector::Children,
            0,
        ),
        c(
            "appCaches",
            "App caches",
            "Caches apps rebuild on their own. Apps may be a little slower the first time.",
            G::System,
            R::Review,
            Os::Macos,
            &[Root::UserCaches],
            Selector::Children,
            7,
        ),
        c(
            "oldLogs",
            "Old logs",
            "Log files older than 30 days.",
            G::System,
            R::Safe,
            Os::Macos,
            &[Root::UserLogs],
            Selector::Children,
            30,
        ),
        c(
            "chromeCache",
            "Chrome cache",
            "Pages and images Chrome keeps to load sites faster. Sign-ins and history are kept.",
            G::Browser,
            R::Safe,
            Os::Any,
            &[Root::ChromeCache],
            Selector::Children,
            0,
        ),
        c(
            "edgeCache",
            "Edge cache",
            "Pages and images Edge keeps to load sites faster. Sign-ins and history are kept.",
            G::Browser,
            R::Safe,
            Os::Any,
            &[Root::EdgeCache],
            Selector::Children,
            0,
        ),
        c(
            "firefoxCache",
            "Firefox cache",
            "Firefox's page cache. Sign-ins and history are kept.",
            G::Browser,
            R::Safe,
            Os::Any,
            &[Root::FirefoxCache],
            Selector::Children,
            0,
        ),
        c(
            "safariCache",
            "Safari cache",
            "Safari's page cache. Sign-ins and history are kept.",
            G::Browser,
            R::Safe,
            Os::Macos,
            &[Root::SafariCache],
            Selector::Children,
            0,
        ),
        c(
            "downloadInstallers",
            "Installers in Downloads",
            "Setup files you downloaded more than 30 days ago. These are your files: Dyno never selects them for you.",
            G::Downloads,
            R::Personal,
            Os::Any,
            &[Root::Downloads],
            Selector::Installers,
            30,
        ),
        c(
            "devArtifacts",
            "Build output of idle projects",
            "Rust target/ and node_modules folders of projects untouched for a while. The next build recreates them.",
            G::Developer,
            R::Review,
            Os::Any,
            &[Root::Projects],
            Selector::DevArtifacts,
            0,
        ),
        c(
            "cargoCache",
            "Cargo cache",
            "Downloaded crates. Cargo downloads them again when a build needs them.",
            G::Developer,
            R::Review,
            Os::Any,
            &[Root::CargoCache],
            Selector::Children,
            30,
        ),
        c(
            "npmCache",
            "npm cache",
            "Downloaded npm packages. npm downloads them again when needed.",
            G::Developer,
            R::Review,
            Os::Any,
            &[Root::NpmCache],
            Selector::Children,
            30,
        ),
        c(
            "nugetCache",
            "NuGet cache",
            "Downloaded NuGet packages. Restores download them again.",
            G::Developer,
            R::Review,
            Os::Any,
            &[Root::NugetCache],
            Selector::Children,
            30,
        ),
        c(
            "pipCache",
            "pip cache",
            "Downloaded Python packages and wheels.",
            G::Developer,
            R::Safe,
            Os::Any,
            &[Root::PipCache],
            Selector::Children,
            14,
        ),
        c(
            "rustToolchains",
            "Unused Rust toolchains",
            "Rust toolchains that are neither the default nor used by a project override.",
            G::Developer,
            R::Review,
            Os::Any,
            &[Root::RustToolchains],
            Selector::StaleToolchains,
            0,
        ),
        c(
            "dockerImages",
            "Unused Docker images",
            "Dangling images no container or tag uses. Removed through Docker, not as files.",
            G::Docker,
            R::Review,
            Os::Any,
            &[Root::DockerImages],
            Selector::DockerImages,
            0,
        ),
    ]
}

/// Categories for one OS.
pub fn for_os(os: Os) -> Vec<CategorySpec> {
    catalog().into_iter().filter(|c| c.os == Os::Any || c.os == os).collect()
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
            id: self.id.into(),
            name: self.name.into(),
            description: self.description.into(),
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
        let mut ids: Vec<_> = all.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), all.len(), "ids are unique");
        assert!(all.iter().all(|c| !c.roots.is_empty() && !c.description.is_empty()));
        assert!(for_os(Os::Windows).iter().all(|c| c.os != Os::Macos));
        assert!(for_os(Os::Macos).iter().any(|c| c.id == "safariCache"));
        let d = all.iter().find(|c| c.id == "downloadInstallers").unwrap();
        assert_eq!(d.risk, CleanupRisk::Personal);
        assert!(!d.view(true).scheduled, "personal files are never scheduled");
        assert!(all[0].view(true).scheduled);
        assert!(!all[0].view(false).scheduled);
    }
}
