//! What the UI shows about the renderer (`RendererStatus` in `api.types.ts`, camelCase JSON).

use crate::discover::{self, Browser};
use crate::manifest::{platform_key, Manifest};
use serde::Serialize;
use std::path::Path;

/// Which browser renders JavaScript pages (ADR 0002).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RendererKind {
    /// An installed Edge, Chrome or Chromium.
    ChromiumInstalled,
    /// The pinned Chromium Spotter downloaded.
    ChromiumDownloaded,
    /// The OS web view (last resort; no renderer is built on it yet).
    OsWebview,
    /// Nothing available.
    None,
}

/// The renderer status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RendererStatus {
    /// Which one.
    pub kind: RendererKind,
    /// Browser name and version, or why none is available and what to do.
    pub detail: String,
    /// A pinned Chromium can be downloaded on request.
    pub can_download: bool,
}

/// What was found; separated from the lookups so the wording is testable.
pub struct Found {
    /// An installed browser and its `--version` output.
    pub installed: Option<(Browser, Option<String>)>,
    /// A downloaded browser and its version output.
    pub downloaded: Option<(Browser, Option<String>)>,
    /// The OS web view could be used as a last resort.
    pub os_webview: bool,
    /// The manifest has a configured entry for this platform.
    pub can_download: bool,
}

/// Pure composition of the status.
pub fn compose(f: Found) -> RendererStatus {
    let name = |b: &Browser, v: &Option<String>| match v {
        Some(v) if !v.is_empty() => v.clone(),
        _ => b.flavor.name().to_string(),
    };
    if let Some((b, v)) = &f.installed {
        return RendererStatus {
            kind: RendererKind::ChromiumInstalled,
            detail: name(b, v),
            can_download: f.can_download,
        };
    }
    if let Some((b, v)) = &f.downloaded {
        return RendererStatus {
            kind: RendererKind::ChromiumDownloaded,
            detail: name(b, v),
            can_download: f.can_download,
        };
    }
    if f.os_webview {
        return RendererStatus {
            kind: RendererKind::OsWebview,
            detail: "Only the operating system web view is available: it cannot be supervised, so JavaScript pages are not rendered. Install Edge, Chrome or Chromium."
                .into(),
            can_download: f.can_download,
        };
    }
    let how = if f.can_download {
        "Install Edge, Chrome or Chromium, or download Chromium in Settings."
    } else {
        "Install Edge, Chrome or Chromium (a downloadable build is not available in this version)."
    };
    RendererStatus {
        kind: RendererKind::None,
        detail: format!("No Chromium-based browser found. {how}"),
        can_download: f.can_download,
    }
}

/// Looks the browsers up on this machine (runs `--version` on the one it finds) and composes the status.
/// `data_dir` is the app data folder where a downloaded Chromium lives. Blocking: call from `spawn_blocking`.
pub fn renderer_status(data_dir: &Path) -> RendererStatus {
    let version = |b: Browser| {
        let v = crate::install::self_test(&b.path).ok();
        (b, v)
    };
    compose(Found {
        installed: discover::find_installed().map(version),
        downloaded: discover::find_downloaded(data_dir).map(version),
        os_webview: false,
        can_download: Manifest::embedded().is_configured(&platform_key()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::Flavor;

    fn b() -> Browser {
        Browser {
            flavor: Flavor::Chrome,
            path: "/x".into(),
        }
    }
    fn none() -> Found {
        Found {
            installed: None,
            downloaded: None,
            os_webview: false,
            can_download: false,
        }
    }

    #[test]
    fn installed_wins_over_downloaded() {
        let s = compose(Found {
            installed: Some((b(), Some("Google Chrome 140".into()))),
            downloaded: Some((b(), None)),
            ..none()
        });
        assert_eq!(s.kind, RendererKind::ChromiumInstalled);
        assert_eq!(s.detail, "Google Chrome 140");
    }

    #[test]
    fn downloaded_and_name_fallback() {
        let s = compose(Found {
            downloaded: Some((b(), None)),
            can_download: true,
            ..none()
        });
        assert_eq!(s.kind, RendererKind::ChromiumDownloaded);
        assert_eq!(s.detail, "Google Chrome");
        assert!(s.can_download);
    }

    #[test]
    fn nothing_available_says_what_to_do() {
        let s = compose(none());
        assert_eq!(s.kind, RendererKind::None);
        assert!(s.detail.contains("Install Edge"));
        assert!(!s.can_download);
        assert!(compose(Found {
            can_download: true,
            ..none()
        })
        .detail
        .contains("download Chromium"));
        assert_eq!(
            compose(Found {
                os_webview: true,
                ..none()
            })
            .kind,
            RendererKind::OsWebview
        );
    }

    #[test]
    fn serialises_like_the_ui_contract() {
        let j = serde_json::to_value(compose(Found {
            installed: Some((b(), None)),
            ..none()
        }))
        .unwrap();
        assert_eq!(j["kind"], "chromium-installed");
        assert_eq!(j["canDownload"], false);
        assert_eq!(serde_json::to_value(RendererKind::OsWebview).unwrap(), "os-webview");
        assert_eq!(
            serde_json::to_value(RendererKind::ChromiumDownloaded).unwrap(),
            "chromium-downloaded"
        );
    }
}
