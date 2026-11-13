//! `--scope apps/<app>` (C99, the release path, ER-31): what one app's release is judged on. The app's folder, the shared `crates/`,
//! and the UI projects it ships (`ui/projects/<app>` and the shared `ui/projects/paddock-ui`). The port of `util/scope.mjs`.

/// The scope as given, without trailing slashes, or `None` (none given, or only slashes).
pub fn normalize(scope: Option<&str>) -> Option<String> {
    let s = scope?.trim_end_matches('/');
    (!s.is_empty()).then(|| s.to_string())
}

/// Whether a repository path (a file, or a waiver path with a `#key` suffix) is inside the scope. No scope: everything.
pub fn in_scope(scope: Option<&str>, path: &str) -> bool {
    let Some(s) = normalize(scope) else { return true };
    let f = path.split('#').next().unwrap_or("");
    let app = s.rsplit('/').next().unwrap_or("");
    f == s
        || f.starts_with(&format!("{s}/"))
        || f.starts_with("crates/")
        || f.starts_with("ui/projects/paddock-ui/")
        || f.starts_with(&format!("ui/projects/{app}/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_scope_is_everything_and_slashes_are_dropped() {
        assert!(in_scope(None, "anything"));
        assert!(in_scope(Some("/"), "anything"));
        assert_eq!(normalize(Some("apps/dyno//")).as_deref(), Some("apps/dyno"));
    }

    #[test]
    fn an_app_scope_holds_its_folder_the_shared_crates_and_its_ui_projects() {
        let s = Some("apps/dyno");
        for p in [
            "apps/dyno",
            "apps/dyno/app/x.ts",
            "crates/a/src/lib.rs",
            "ui/projects/paddock-ui/x",
            "ui/projects/dyno/y",
            "apps/dyno/Cargo.lock#key",
        ] {
            assert!(in_scope(s, p), "{p}");
        }
        for p in ["apps/dynox/a", "apps/debrief/a", "ui/projects/debrief/y", "docs/a.md"] {
            assert!(!in_scope(s, p), "{p}");
        }
    }
}
