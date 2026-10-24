//! What a launch asks for, and where it goes: the command line (flags, paths, deep links), whether this process joins
//! the single-instance group, and the plan for files and links that arrive from outside (a second launch, the OS's
//! "Open with" or a URL scheme, the tray).
//!
//! Ported from Debrief's `instance.rs` with deep links added; parsing and planning are pure.

use serde::Serialize;
use std::path::{Path, PathBuf};

/// Starts a separate process even when the app is already running.
pub const NEW_INSTANCE_FLAG: &str = "--new-instance";
/// Opens the given files (or nothing) in a new window of the running instance.
pub const NEW_WINDOW_FLAG: &str = "--new-window";
/// Starts in Safe Mode (a separate process; ADR 0058's Restart in Safe Mode).
pub const SAFE_MODE_FLAG: &str = "--safe-mode";
/// Starts in the tray without a window (the log-in item passes it); a second launch with it changes nothing.
pub const HIDDEN_FLAG: &str = "--hidden";
/// Ends the flags: every later argument is a path (or a link), even one that looks like a flag.
pub const END_OF_FLAGS: &str = "--";
/// Command-line paths are capped (a hostile caller cannot make the app open millions of paths).
pub const MAX_PATHS: usize = 256;
/// Deep links per launch.
pub const MAX_LINKS: usize = 16;
/// The longest deep link, in bytes.
pub const MAX_LINK_LEN: usize = 2048;
/// Query parameters per deep link.
pub const MAX_LINK_PARAMS: usize = 32;

/// The arguments before [`END_OF_FLAGS`]: the only ones read as flags, so a path after it never switches on a mode.
pub fn flags<S: AsRef<str>>(args: &[S]) -> &[S] {
    let end = args
        .iter()
        .position(|a| a.as_ref() == END_OF_FLAGS)
        .unwrap_or(args.len());
    &args[..end]
}

/// A link into the app (`pitwall://models/llama?tab=files`), parsed once at the edge. Only the app's own schemes are
/// links; anything else on a command line is a path.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeepLink {
    /// The link as parsed (normalised).
    pub url: String,
    /// The scheme, lowercase (one of the app's).
    pub scheme: String,
    /// What the link points at: the host part (`models`), possibly empty.
    pub host: String,
    /// The path after the host (`/llama`), possibly empty.
    pub path: String,
    /// The query parameters, decoded, in order.
    pub query: Vec<(String, String)>,
}

/// A deep link for one of `schemes` (lowercase, without `://`), or `None`: another scheme, too long, control
/// characters, credentials in the URL, or not a URL at all.
pub fn parse_link(text: &str, schemes: &[&str]) -> Option<DeepLink> {
    if text.len() > MAX_LINK_LEN || text.chars().any(char::is_control) {
        return None;
    }
    let (scheme, _) = text.split_once(':')?;
    let scheme = scheme.to_ascii_lowercase();
    if !schemes.contains(&scheme.as_str()) {
        return None;
    }
    let url = url::Url::parse(text).ok()?;
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let query: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if query.len() > MAX_LINK_PARAMS {
        return None;
    }
    Some(DeepLink {
        scheme,
        host: url.host_str().unwrap_or_default().to_string(),
        path: url.path().to_string(),
        query,
        url: url.to_string(),
    })
}

/// What a command line asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaunchArgs {
    /// Absolute paths (relative ones resolved against the caller's folder), not yet checked.
    pub paths: Vec<PathBuf>,
    /// Links for the app's schemes.
    pub links: Vec<DeepLink>,
    pub new_window: bool,
    pub new_instance: bool,
    pub safe_mode: bool,
    pub hidden: bool,
}

impl LaunchArgs {
    fn take(&mut self, arg: &str, cwd: Option<&Path>, schemes: &[&str]) {
        if arg.is_empty() || arg.contains('\0') {
            return;
        }
        match parse_link(arg, schemes) {
            Some(link) if self.links.len() < MAX_LINKS => self.links.push(link),
            Some(_) => {}
            None if self.paths.len() < MAX_PATHS => self.paths.push(resolve(arg, cwd)),
            None => {}
        }
    }
}

/// Parses arguments (without the program name). Unknown flags are ignored; an argument that is a link for one of
/// `schemes` is a link, every other one a path.
pub fn parse_args<I: IntoIterator<Item = String>>(
    args: I,
    cwd: Option<&Path>,
    schemes: &[&str],
) -> LaunchArgs {
    let mut out = LaunchArgs::default();
    let mut paths_only = false;
    for arg in args {
        match arg.as_str() {
            a if paths_only => out.take(a, cwd, schemes),
            END_OF_FLAGS => paths_only = true,
            NEW_INSTANCE_FLAG => out.new_instance = true,
            NEW_WINDOW_FLAG => out.new_window = true,
            SAFE_MODE_FLAG => out.safe_mode = true,
            HIDDEN_FLAG => out.hidden = true,
            a if a.starts_with('-') => {}
            a => out.take(a, cwd, schemes),
        }
    }
    out
}

fn resolve(arg: &str, cwd: Option<&Path>) -> PathBuf {
    let p = PathBuf::from(arg);
    match cwd {
        Some(dir) if p.is_relative() => dir.join(p),
        _ => p,
    }
}

/// Whether this process joins the single-instance group (and hands its arguments over when another instance runs):
/// not with `--new-instance`, and not in Safe Mode (a flag or the app's environment variable).
pub fn wants_single_instance(args: &LaunchArgs, safe_mode_env: bool) -> bool {
    !args.new_instance && !args.safe_mode && !safe_mode_env
}

/// Existing paths only, canonicalized (the edge between the OS and the app).
pub fn existing(paths: &[PathBuf]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Files and links for a window, waiting until its UI takes them (`window_take_pending`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenRequest {
    /// Canonical paths of existing files and folders.
    pub paths: Vec<String>,
    pub links: Vec<DeepLink>,
}

impl OpenRequest {
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.links.is_empty()
    }

    /// `other` after this one, within the caps.
    pub fn extend(&mut self, other: OpenRequest) {
        let room = MAX_PATHS.saturating_sub(self.paths.len());
        self.paths.extend(other.paths.into_iter().take(room));
        let room = MAX_LINKS.saturating_sub(self.links.len());
        self.links.extend(other.links.into_iter().take(room));
    }
}

/// Where files from another app open when the window in front already shows something (a user setting).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OpenFilesIn {
    NewWindow,
    CurrentWindow,
    #[default]
    Ask,
}

/// The window files would go to: its label, and whether nothing is open there yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub label: String,
    pub pristine: bool,
}

/// What to do with files and links from outside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenPlan {
    /// Nothing to open: show and focus this window.
    Reveal(String),
    /// Open a new window with these.
    NewWindow(OpenRequest),
    /// Give these to this window.
    OpenIn(String, OpenRequest),
    /// Ask in this window whether the paths open here or in a new window; the links open here at once.
    Ask {
        label: String,
        paths: Vec<String>,
        links: Vec<DeepLink>,
    },
}

impl OpenPlan {
    /// For logs (paths and links stay out of them).
    pub fn kind(&self) -> &'static str {
        match self {
            OpenPlan::Reveal(_) => "reveal",
            OpenPlan::NewWindow(_) => "new-window",
            OpenPlan::OpenIn(..) => "current-window",
            OpenPlan::Ask { .. } => "ask",
        }
    }
}

/// Where files and links from outside go. A single-window app (`multi_window` false) always uses its one window; an
/// empty window is always reused; links never ask.
pub fn plan(
    request: OpenRequest,
    force_new_window: bool,
    setting: OpenFilesIn,
    target: Option<Target>,
    multi_window: bool,
) -> OpenPlan {
    let Some(t) = target else {
        return OpenPlan::NewWindow(request);
    };
    if force_new_window && multi_window {
        return OpenPlan::NewWindow(request);
    }
    if request.is_empty() {
        return OpenPlan::Reveal(t.label);
    }
    if t.pristine || !multi_window || request.paths.is_empty() {
        return OpenPlan::OpenIn(t.label, request);
    }
    match setting {
        OpenFilesIn::NewWindow => OpenPlan::NewWindow(request),
        OpenFilesIn::CurrentWindow => OpenPlan::OpenIn(t.label, request),
        OpenFilesIn::Ask => OpenPlan::Ask {
            label: t.label,
            paths: request.paths,
            links: request.links,
        },
    }
}

/// The file names of `paths` for the "open here or in a new window?" question (the full paths stay in Rust).
pub fn names(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|p| {
            Path::new(p)
                .file_name()
                .map_or_else(|| p.clone(), |n| n.to_string_lossy().into_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMES: &[&str] = &["pitwall"];

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn req(paths: &[&str], links: usize) -> OpenRequest {
        OpenRequest {
            paths: paths.iter().map(|s| s.to_string()).collect(),
            links: (0..links)
                .map(|i| parse_link(&format!("pitwall://x/{i}"), SCHEMES).unwrap())
                .collect(),
        }
    }

    #[test]
    fn parses_flags_and_resolves_relative_paths() {
        let a = parse_args(
            args(&["logs", "/abs/x.proj", "--new-window", "--verbose", ""]),
            Some(Path::new("/home/u")),
            SCHEMES,
        );
        assert_eq!(
            a.paths,
            [PathBuf::from("/home/u/logs"), PathBuf::from("/abs/x.proj")]
        );
        assert!(a.new_window && !a.new_instance && !a.safe_mode && !a.hidden);
        assert!(wants_single_instance(&a, false));
        let b = parse_args(args(&["--new-instance", "rel"]), None, SCHEMES);
        assert_eq!(b.paths, [PathBuf::from("rel")]);
        assert!(!wants_single_instance(&b, false));
        let c = parse_args(args(&[SAFE_MODE_FLAG, HIDDEN_FLAG]), None, SCHEMES);
        assert!(c.safe_mode && c.hidden && !wants_single_instance(&c, false));
        assert!(
            !wants_single_instance(&LaunchArgs::default(), true),
            "the app's Safe Mode variable"
        );
        let many: Vec<String> = (0..1000).map(|i| format!("/p{i}")).collect();
        assert_eq!(parse_args(many, None, SCHEMES).paths.len(), MAX_PATHS);
        assert!(parse_args(args(&["a\0b"]), None, SCHEMES).paths.is_empty());
    }

    #[test]
    fn everything_after_the_end_of_flags_is_a_path_or_a_link() {
        let a = parse_args(
            args(&[
                "--new-window",
                "--",
                "--safe-mode",
                "/x.log",
                "",
                "a\0b",
                "pitwall://open",
            ]),
            None,
            SCHEMES,
        );
        assert!(a.new_window && !a.safe_mode);
        assert_eq!(
            a.paths,
            [PathBuf::from("--safe-mode"), PathBuf::from("/x.log")]
        );
        assert_eq!(a.links.len(), 1);
        assert_eq!(
            flags(&["--isolated", "--", "--export-diagnostics"]),
            ["--isolated"]
        );
        assert_eq!(flags(&["a", "b"]), ["a", "b"]);
        assert!(flags::<&str>(&[]).is_empty());
    }

    #[test]
    fn links_of_the_apps_schemes_are_parsed_and_everything_else_is_a_path() {
        let a = parse_args(
            args(&[
                "PITWALL://models/llama%20b?tab=files&x=1",
                "https://example.com",
                "C:\\logs\\a.log",
            ]),
            None,
            SCHEMES,
        );
        assert_eq!(
            a.paths,
            [
                PathBuf::from("https://example.com"),
                PathBuf::from("C:\\logs\\a.log")
            ]
        );
        let link = &a.links[0];
        assert_eq!(
            (link.scheme.as_str(), link.host.as_str(), link.path.as_str()),
            ("pitwall", "models", "/llama%20b")
        );
        assert_eq!(
            link.query,
            [
                ("tab".to_string(), "files".to_string()),
                ("x".to_string(), "1".to_string())
            ]
        );
        assert_eq!(link.url, "pitwall://models/llama%20b?tab=files&x=1");
        let many: Vec<String> = (0..40).map(|i| format!("pitwall://x/{i}")).collect();
        let b = parse_args(many, None, SCHEMES);
        assert_eq!((b.links.len(), b.paths.len()), (MAX_LINKS, 0));
    }

    #[test]
    fn hostile_links_are_refused() {
        let long = format!("pitwall://x/{}", "a".repeat(MAX_LINK_LEN));
        let params = format!(
            "pitwall://x?{}",
            (0..=MAX_LINK_PARAMS)
                .map(|i| format!("k{i}=v"))
                .collect::<Vec<_>>()
                .join("&")
        );
        for bad in [
            long.as_str(),
            "pitwall://x/\u{7}",
            "pitwall://user:secret@host/",
            "pitwall://user@host/",
            "pitwall://[bad",
            params.as_str(),
            "no-colon",
            "other://x",
        ] {
            assert_eq!(parse_link(bad, SCHEMES), None, "{bad}");
        }
        assert!(parse_link("pitwall:", SCHEMES).is_some_and(|l| l.host.is_empty()));
    }

    #[test]
    fn existing_keeps_real_paths_only() {
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let found = existing(&[here.join("src"), PathBuf::from("/does/not/exist")]);
        assert_eq!(found.len(), 1);
        assert!(Path::new(&found[0]).is_absolute());
    }

    #[test]
    fn plans_per_setting() {
        use OpenFilesIn::*;
        let busy = || {
            Some(Target {
                label: "w-2".into(),
                pristine: false,
            })
        };
        let empty = || {
            Some(Target {
                label: "main".into(),
                pristine: true,
            })
        };
        let p = || req(&["/a"], 0);
        assert_eq!(
            plan(p(), false, NewWindow, busy(), true),
            OpenPlan::NewWindow(p())
        );
        assert_eq!(
            plan(p(), false, CurrentWindow, busy(), true),
            OpenPlan::OpenIn("w-2".into(), p())
        );
        assert_eq!(
            plan(req(&["/a"], 1), false, Ask, busy(), true),
            OpenPlan::Ask {
                label: "w-2".into(),
                paths: vec!["/a".into()],
                links: req(&[], 1).links
            }
        );
        assert_eq!(
            plan(p(), false, NewWindow, empty(), true),
            OpenPlan::OpenIn("main".into(), p()),
            "an empty window is reused"
        );
        assert_eq!(
            plan(p(), true, CurrentWindow, empty(), true),
            OpenPlan::NewWindow(p()),
            "--new-window"
        );
        assert_eq!(
            plan(OpenRequest::default(), false, Ask, busy(), true),
            OpenPlan::Reveal("w-2".into())
        );
        assert_eq!(
            plan(OpenRequest::default(), true, Ask, busy(), true),
            OpenPlan::NewWindow(OpenRequest::default())
        );
        assert_eq!(
            plan(p(), false, CurrentWindow, None, true),
            OpenPlan::NewWindow(p()),
            "no window left (tray)"
        );
        assert_eq!(
            plan(req(&[], 2), false, NewWindow, busy(), true),
            OpenPlan::OpenIn("w-2".into(), req(&[], 2)),
            "links never ask and never open a window"
        );
        assert_eq!(
            plan(p(), true, NewWindow, busy(), false),
            OpenPlan::OpenIn("w-2".into(), p()),
            "a single-window app uses its window"
        );
        for k in [
            plan(p(), false, Ask, busy(), true),
            OpenPlan::Reveal("x".into()),
        ] {
            assert!(["ask", "reveal"].contains(&k.kind()));
        }
        assert_eq!(OpenPlan::NewWindow(p()).kind(), "new-window");
        assert_eq!(OpenPlan::OpenIn("x".into(), p()).kind(), "current-window");
    }

    #[test]
    fn requests_merge_within_the_caps_and_names_hide_folders() {
        let mut a = req(&["/a"], 1);
        a.extend(req(&["/b"], 1));
        assert_eq!((a.paths.len(), a.links.len()), (2, 2));
        let mut full = OpenRequest {
            paths: vec!["/p".into(); MAX_PATHS],
            links: req(&[], MAX_LINKS).links,
        };
        full.extend(req(&["/c"], 1));
        assert_eq!((full.paths.len(), full.links.len()), (MAX_PATHS, MAX_LINKS));
        assert_eq!(
            names(&["/logs/a.log".into(), "/".into()]),
            ["a.log".to_string(), "/".to_string()]
        );
        assert!(OpenRequest::default().is_empty() && !a.is_empty());
    }
}
