//! Importers: a winget export, a Brewfile or an OPML list becomes watch drafts, plus the entries that could not be
//! turned into one (with the reason). Drafts have an empty id; the backend assigns one when the user saves them.
use crate::limits::{MAX_NAME_CHARS, MAX_WATCHES};
use crate::source::SourceConfig;
use crate::watch::{Watch, WatchSpec};
use quick_xml::events::Event;
use quick_xml::Reader;
use quick_xml::XmlVersion;

/// Largest list read (2 MiB).
pub const MAX_IMPORT_BYTES: usize = 2 << 20;

/// A list format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    /// `winget export` JSON.
    Winget,
    /// A Homebrew `Brewfile`.
    Brewfile,
    /// An OPML list of feeds.
    Opml,
}

impl ImportFormat {
    /// From the UI's id (`winget`, `brewfile`, `opml`).
    pub fn parse(s: &str) -> Option<ImportFormat> {
        match s {
            "winget" => Some(ImportFormat::Winget),
            "brewfile" => Some(ImportFormat::Brewfile),
            "opml" => Some(ImportFormat::Opml),
            _ => None,
        }
    }
}

/// What an import produced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Imported {
    /// Drafts, not yet saved.
    pub drafts: Vec<WatchSpec>,
    /// Entries left out, each with the reason.
    pub skipped: Vec<String>,
}

/// Reads a list. `Err` only when the text cannot be read at all (not JSON, not XML, too large).
pub fn import(format: ImportFormat, text: &str) -> Result<Imported, String> {
    if text.len() > MAX_IMPORT_BYTES {
        return Err("The list is larger than 2 MiB.".into());
    }
    let text = text.trim_start_matches('\u{feff}');
    let mut out = match format {
        ImportFormat::Winget => winget(text)?,
        ImportFormat::Brewfile => brewfile(text),
        ImportFormat::Opml => opml(text)?,
    };
    if out.drafts.len() > MAX_WATCHES {
        out.skipped.push(format!("Only the first {MAX_WATCHES} entries are imported."));
        out.drafts.truncate(MAX_WATCHES);
    }
    Ok(out)
}

fn draft(name: &str, source: SourceConfig, scheme: &str, interval_secs: u32) -> WatchSpec {
    WatchSpec {
        id: String::new(),
        name: name.chars().take(MAX_NAME_CHARS).collect(),
        source,
        versions: scheme.into(),
        version_pattern: None,
        include_prereleases: false,
        name_filter: String::new(),
        interval_secs,
        notify: Default::default(),
        actions: Vec::new(),
        channels: Vec::new(),
        version_filter: Default::default(),
        allow_local: false,
        paused: false,
    }
}

fn valid(spec: WatchSpec, skipped: &mut Vec<String>, out: &mut Vec<WatchSpec>) {
    let probe = WatchSpec {
        id: "draft".into(),
        ..spec.clone()
    };
    match Watch::parse(&probe) {
        Ok(_) => out.push(spec),
        Err(p) => skipped.push(format!("{}: {}", spec.name, p.join(" "))),
    }
}

/// Winget package ids are `Publisher.Package[.Variant]`; the community manifest repository keeps one folder per
/// segment, and the folders of the last one are the versions.
fn winget(text: &str) -> Result<Imported, String> {
    let doc: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("This is not a winget export (JSON): {e}"))?;
    let sources = doc
        .get("Sources")
        .and_then(|s| s.as_array())
        .ok_or("This is not a winget export: it has no \"Sources\" list.")?;
    let mut out = Imported::default();
    for src in sources {
        let source_name = src.pointer("/SourceDetails/Name").and_then(|n| n.as_str()).unwrap_or("winget");
        for pkg in src.get("Packages").and_then(|p| p.as_array()).into_iter().flatten() {
            let Some(id) = pkg.get("PackageIdentifier").and_then(|i| i.as_str()) else {
                continue;
            };
            if !source_name.eq_ignore_ascii_case("winget") {
                out.skipped
                    .push(format!("{id}: from the \"{source_name}\" source, which Spotter cannot read."));
                continue;
            }
            let segments: Vec<&str> = id.split('.').collect();
            let safe = |s: &&str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
            if segments.len() < 2 || !segments.iter().all(safe) {
                out.skipped.push(format!("{id}: not a Publisher.Package identifier."));
                continue;
            }
            let initial = segments[0].chars().next().unwrap_or('_').to_ascii_lowercase();
            let url = format!(
                "https://api.github.com/repos/microsoft/winget-pkgs/contents/manifests/{initial}/{}",
                segments.join("/")
            );
            let source = SourceConfig::new(
                "json-api",
                &[
                    ("url", &url),
                    ("items", "$[*]"),
                    ("version", "$.name"),
                    (
                        "location",
                        &format!(
                            "https://github.com/microsoft/winget-pkgs/tree/master/manifests/{initial}/{}/{{version}}",
                            segments.join("/")
                        ),
                    ),
                ],
            );
            valid(draft(id, source, "numeric", 24 * 3600), &mut out.skipped, &mut out.drafts);
        }
    }
    Ok(out)
}

fn brewfile(text: &str) -> Imported {
    let mut out = Imported::default();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let Some((kind, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let name = rest
            .trim()
            .trim_start_matches(['"', '\''])
            .split(['"', '\''])
            .next()
            .unwrap_or("")
            .trim();
        match kind {
            "brew" | "cask" => {
                let ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "@+._-".contains(c));
                if !ok {
                    out.skipped.push(format!(
                        "{kind} \"{name}\": only plain formula and cask names are supported (not taps)."
                    ));
                    continue;
                }
                let (url, version) = if kind == "brew" {
                    (format!("https://formulae.brew.sh/api/formula/{name}.json"), "$.versions.stable")
                } else {
                    (format!("https://formulae.brew.sh/api/cask/{name}.json"), "$.version")
                };
                let source = SourceConfig::new(
                    "json-api",
                    &[
                        ("url", &url),
                        ("items", "$"),
                        ("version", version),
                        ("location", &format!("https://formulae.brew.sh/{kind}/{name}")),
                    ],
                );
                valid(draft(name, source, "numeric", 24 * 3600), &mut out.skipped, &mut out.drafts);
            }
            "tap" | "mas" | "vscode" | "whalebrew" | "cargo" | "go" | "npm" | "uv" | "flatpak" => {
                out.skipped
                    .push(format!("{kind} \"{name}\": Spotter reads formulae and casks only."));
            }
            _ => {}
        }
    }
    out
}

fn opml(text: &str) -> Result<Imported, String> {
    let mut reader = Reader::from_str(text);
    let mut out = Imported::default();
    let mut seen_opml = false;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let name = e.local_name().as_ref().to_ascii_lowercase();
                if name == "opml" {
                    seen_opml = true;
                } else if name == "outline" {
                    let attr = |key: &str| {
                        e.attributes()
                            .flatten()
                            .find(|a| a.key.local_name().as_ref().eq_ignore_ascii_case(key))
                            .and_then(|a| a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.trim().to_string()))
                    };
                    let title = attr("title").or_else(|| attr("text")).unwrap_or_default();
                    let Some(url) = attr("xmlUrl") else { continue };
                    if !(url.starts_with("http://") || url.starts_with("https://")) {
                        out.skipped.push(format!(
                            "{}: only http(s) feeds are supported.",
                            if title.is_empty() { &url } else { &title }
                        ));
                        continue;
                    }
                    let name = if title.is_empty() { url.clone() } else { title };
                    valid(
                        draft(&name, SourceConfig::new("feed", &[("url", &url)]), "semver", 6 * 3600),
                        &mut out.skipped,
                        &mut out.drafts,
                    );
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("This is not a valid OPML file: {e}")),
            _ => {}
        }
        if out.drafts.len() + out.skipped.len() > 4 * MAX_WATCHES {
            break;
        }
    }
    if !seen_opml {
        return Err("This is not an OPML file: it has no <opml> element.".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn winget_exports_become_manifest_watches() {
        let json = "\u{feff}".to_string()
            + r#"{"Sources":[
              {"SourceDetails":{"Name":"winget"},"Packages":[{"PackageIdentifier":"BurntSushi.ripgrep.MSVC"},{"PackageIdentifier":"Single"},{"PackageIdentifier":"a b.c"},{"Other":1}]},
              {"SourceDetails":{"Name":"msstore"},"Packages":[{"PackageIdentifier":"9NBLGGH4NNS1"}]}]}"#;
        let out = import(ImportFormat::Winget, &json).unwrap();
        assert_eq!(out.drafts.len(), 1);
        let d = &out.drafts[0];
        assert_eq!(d.name, "BurntSushi.ripgrep.MSVC");
        assert_eq!(
            d.source.values["url"],
            "https://api.github.com/repos/microsoft/winget-pkgs/contents/manifests/b/BurntSushi/ripgrep/MSVC"
        );
        assert_eq!((d.id.as_str(), d.versions.as_str()), ("", "numeric"));
        assert_eq!(out.skipped.len(), 3, "{:?}", out.skipped);
        assert!(out.skipped.iter().any(|s| s.contains("msstore")));
        assert!(import(ImportFormat::Winget, "[]").unwrap_err().contains("Sources"));
        assert!(import(ImportFormat::Winget, "nope").unwrap_err().contains("not a winget"));
    }

    #[test]
    fn brewfiles_become_formula_and_cask_watches() {
        let text = "tap \"homebrew/cask\"\n# comment\nbrew \"ripgrep\"\nbrew 'fd' # trailing\ncask \"visual-studio-code\"\nbrew \"user/tap/thing\"\nmas \"Xcode\", id: 497799835\n\nbogus line\nbrew";
        let out = import(ImportFormat::Brewfile, text).unwrap();
        let names: Vec<&str> = out.drafts.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["ripgrep", "fd", "visual-studio-code"]);
        assert_eq!(out.drafts[0].source.values["version"], "$.versions.stable");
        assert_eq!(
            out.drafts[2].source.values["url"],
            "https://formulae.brew.sh/api/cask/visual-studio-code.json"
        );
        assert_eq!(out.skipped.len(), 3, "{:?}", out.skipped);
    }

    #[test]
    fn opml_lists_become_feed_watches() {
        let text = r#"<?xml version="1.0"?><opml version="2.0"><body>
            <outline text="Dev"><outline type="rss" text="Rust Blog" xmlUrl="https://blog.rust-lang.org/feed.xml"/>
            <outline title="Mine" text="x" xmlUrl="http://example.org/f?a=1&amp;b=2"/>
            <outline text="Local" xmlUrl="file:///etc/passwd"/><outline xmlUrl="https://a.example/rss"></outline>
            <outline text="No url"/></outline></body></opml>"#;
        let out = import(ImportFormat::Opml, text).unwrap();
        let names: Vec<&str> = out.drafts.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, ["Rust Blog", "Mine", "https://a.example/rss"]);
        assert_eq!(out.drafts[1].source.values["url"], "http://example.org/f?a=1&b=2");
        assert_eq!(out.skipped.len(), 1);
        assert!(import(ImportFormat::Opml, "<html/>").unwrap_err().contains("<opml>"));
        assert!(import(ImportFormat::Opml, "<opml><body></opml>").unwrap_err().contains("OPML"));
    }

    #[test]
    fn imports_are_bounded_and_formats_parse() {
        assert!(import(ImportFormat::Brewfile, &" ".repeat(MAX_IMPORT_BYTES + 1))
            .unwrap_err()
            .contains("2 MiB"));
        let many: String = (0..MAX_WATCHES + 5).map(|i| format!("brew \"pkg{i}\"\n")).collect();
        let out = import(ImportFormat::Brewfile, &many).unwrap();
        assert_eq!((out.drafts.len(), out.skipped.len()), (MAX_WATCHES, 1));
        assert_eq!(ImportFormat::parse("opml"), Some(ImportFormat::Opml));
        assert_eq!(ImportFormat::parse("winget"), Some(ImportFormat::Winget));
        assert_eq!(ImportFormat::parse("brewfile"), Some(ImportFormat::Brewfile));
        assert_eq!(ImportFormat::parse("csv"), None);
    }
}
