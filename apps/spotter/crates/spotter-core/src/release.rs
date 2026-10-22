//! Releases: what a source reports ([`Candidate`]) and what a watch keeps after reading its version ([`Release`]),
//! plus [`select`], which turns a check's candidates into the watch's releases.

use crate::checksum::Sha256Digest;
use crate::extract::{Extracted, Ignored};
use crate::limits::MAX_FOLDER_MATCHES;
use crate::pattern::NamePattern;
use crate::time::Timestamp;
use crate::version::{Version, VersionScheme};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One thing a source found, before its version is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The text the version is read from (file name, tag, title, version string).
    pub version_text: String,
    /// Display name.
    pub name: String,
    /// Where it is: a path or URL.
    pub location: String,
    /// When it was published or last changed.
    pub published_at: Option<Timestamp>,
    /// Size in bytes.
    pub size: Option<u64>,
    /// The publisher's SHA-256 of the file at `location`.
    pub sha256: Option<Sha256Digest>,
    /// Whether the source marks it as a pre-release.
    pub prerelease: bool,
    /// When it was created, where the source reports that (SMB on Windows does; many mounts do not).
    pub created_at: Option<Timestamp>,
}

impl Candidate {
    /// A candidate whose version is read from its name.
    pub fn named(name: &str, location: &str) -> Candidate {
        Candidate {
            version_text: name.into(),
            name: name.into(),
            location: location.into(),
            published_at: None,
            size: None,
            sha256: None,
            prerelease: false,
            created_at: None,
        }
    }
}

/// A checksum file next to an artifact (`setup.exe.sha256`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecksumFile {
    /// Its name.
    pub name: String,
    /// Where it is.
    pub location: String,
}

/// One file of a release: the same product and version can come as an ISO, an installer and a checksum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// File name.
    pub name: String,
    /// Where it is: a path or URL.
    pub location: String,
    /// Lower-case extension without the dot (`iso`, `exe`, `tar.gz`); empty when there is none.
    #[serde(default)]
    pub kind: String,
    /// The `variant` group (`x64`, `lite`); empty when none.
    #[serde(default)]
    pub variant: String,
    /// Size in bytes.
    #[serde(default)]
    pub size: Option<u64>,
    /// Last change.
    #[serde(default)]
    pub modified: Option<Timestamp>,
    /// Creation time, where reported.
    #[serde(default)]
    pub created: Option<Timestamp>,
    /// The checksum file that goes with it, when one is next to it.
    #[serde(default)]
    pub checksum_file: Option<ChecksumFile>,
}

/// What a release carries beyond its version (all optional, so older state files still load).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ReleaseDetail {
    /// The product (`id` group); `None` for a watch of one thing.
    #[serde(default)]
    pub product: Option<String>,
    /// The channel label as written (`beta`).
    #[serde(default)]
    pub channel: String,
    /// Other named groups of the pattern, for display.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
    /// Every file of this release, the primary one first.
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    /// The same file name came back with new contents (size or modified time changed).
    #[serde(default)]
    pub updated_in_place: bool,
}

/// A release a watch knows about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// Its version.
    pub version: Version,
    /// Display name.
    pub name: String,
    /// Where it is: a path or URL.
    pub location: String,
    /// When it was published or last changed.
    #[serde(default)]
    pub published_at: Option<Timestamp>,
    /// Size in bytes.
    #[serde(default)]
    pub size: Option<u64>,
    /// The publisher's SHA-256.
    #[serde(default)]
    pub sha256: Option<Sha256Digest>,
    /// Product, channel, fields and artifacts.
    #[serde(default)]
    pub detail: ReleaseDetail,
}

impl Release {
    /// The product, if the watch tracks several.
    pub fn product(&self) -> Option<&str> {
        self.detail.product.as_deref()
    }

    /// The artifact to open or download: the first with `variant` (when given) and `kind` (an extension, when given),
    /// else the primary one. `None` only for a release that has no artifacts (a source that is not a file list).
    pub fn pick(&self, variant: &str, kind: &str) -> Option<&Artifact> {
        let (variant, kind) = (variant.trim(), kind.trim().trim_start_matches('.').to_ascii_lowercase());
        self.detail
            .artifacts
            .iter()
            .find(|a| (variant.is_empty() || a.variant.eq_ignore_ascii_case(variant)) && (kind.is_empty() || a.kind == kind))
            .or_else(|| self.detail.artifacts.first())
    }

    /// The same release with its primary file replaced by the chosen artifact (what actions run on).
    pub fn with_artifact(&self, variant: &str, kind: &str) -> Release {
        match self.pick(variant, kind) {
            Some(a) => Release {
                name: a.name.clone(),
                location: a.location.clone(),
                size: a.size.or(self.size),
                ..self.clone()
            },
            None => self.clone(),
        }
    }
}

/// How the newest release is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LatestBy {
    /// The extracted version numbers, in the scheme's order. Ties go to the newer modified time, then the name.
    #[default]
    Version,
    /// The newest modified time (for names without usable versions). Re-copying an old build makes it look new.
    Modified,
    /// The newest creation time, where the source reports it.
    Created,
    /// The last name in natural order (`build-2026-10-23` after `build-2026-10-9`).
    Name,
}

impl LatestBy {
    /// From the stored value.
    pub fn parse(s: &str) -> Result<LatestBy, String> {
        match s {
            "version" => Ok(LatestBy::Version),
            "modified" => Ok(LatestBy::Modified),
            "created" => Ok(LatestBy::Created),
            "name" => Ok(LatestBy::Name),
            other => Err(format!(
                "\"{other}\" is not a way to pick the latest (version, modified, created or name)."
            )),
        }
    }
}

/// Which candidates a watch keeps.
#[derive(Debug, Clone, PartialEq)]
pub struct ReleaseFilter {
    /// Keep pre-releases (labelled versions, or marked so by the source).
    pub include_prereleases: bool,
    /// Keep only names matching this (applies to the candidate's name).
    pub name: NamePattern,
    /// How the newest release is decided.
    pub latest_by: LatestBy,
    /// Track only these products (the `id` group); empty for all.
    pub products: Vec<String>,
}

impl Default for ReleaseFilter {
    fn default() -> ReleaseFilter {
        ReleaseFilter {
            include_prereleases: false,
            name: NamePattern::any(),
            latest_by: LatestBy::Version,
            products: Vec::new(),
        }
    }
}

/// Why a candidate is not a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// Left out by the name filter.
    Name,
    /// Not one of the chosen file extensions.
    Extension,
    /// The scheme found nothing in the name.
    Unread(Ignored),
    /// A pre-release, and pre-releases are off.
    Prerelease,
    /// A product the watch does not track.
    Product,
    /// The chosen "latest by" needs a time the source did not report.
    NoTime(&'static str),
    /// A name without digits cannot be ordered by name.
    NoDigits,
    /// A checksum file that belongs to another file.
    Checksum,
}

impl Skip {
    /// The reason, for the user.
    pub fn reason(&self) -> String {
        match self {
            Skip::Name => "ignored: the name filter does not match".into(),
            Skip::Extension => "ignored: not one of the chosen file types".into(),
            Skip::Unread(i) => format!("ignored: {}", i.reason()),
            Skip::Prerelease => "ignored: a pre-release (turn on pre-releases to include it)".into(),
            Skip::Product => "ignored: not one of the chosen products".into(),
            Skip::NoTime(what) => format!("ignored: the source reports no {what} for it"),
            Skip::NoDigits => "ignored: the name has no number to order by".into(),
            Skip::Checksum => "attached to its file as a checksum".into(),
        }
    }
}

/// The result of [`select`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Selection {
    /// Releases: by product, then newest first, one per product and version, at most the limit per product.
    pub releases: Vec<Release>,
    /// Candidates whose name matched but whose text held no version (shown in the source test).
    pub without_version: Vec<String>,
    /// Candidates left out as pre-releases.
    pub prereleases_skipped: usize,
    /// Candidates left out by the name filter.
    pub names_skipped: usize,
}

/// One candidate read: what to group and order by.
struct Read {
    extracted: Extracted,
    /// The text that identifies the release before its time is known (the version text, else the name).
    identity: String,
    candidate: Candidate,
}

/// Lower-case extension (`iso`, `tar.gz`), empty when none.
pub fn extension_of(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for double in [".tar.gz", ".tar.xz", ".tar.bz2", ".tar.zst"] {
        if lower.ends_with(double) {
            return double[1..].to_string();
        }
    }
    match lower.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() && ext.len() <= 12 && !ext.contains(['/', '\\', ' ']) => ext.to_string(),
        _ => String::new(),
    }
}

/// The checksum-file extensions (`sha256`, `sha512`, `sha1`, `md5`, and the `*sum` spellings).
pub const CHECKSUM_EXTENSIONS: [&str; 7] = ["sha256", "sha512", "sha1", "md5", "sha256sum", "sha1sum", "md5sum"];

/// Whether `name` looks like a checksum file for another file; the file it belongs to.
pub fn checksum_target(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && CHECKSUM_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())).then_some(stem)
}

fn display_time(t: Timestamp) -> String {
    time::OffsetDateTime::from_unix_timestamp(t.0.div_euclid(1000))
        .ok()
        .and_then(|d| {
            let f = time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]");
            d.format(&f).ok()
        })
        .unwrap_or_default()
}

fn digit_runs(s: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in s.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            out.push(cur.parse::<u64>().unwrap_or(u64::MAX));
            cur.clear();
        }
    }
    out.truncate(crate::version::MAX_PARTS);
    out
}

fn read(c: Candidate, scheme: &VersionScheme, filter: &ReleaseFilter) -> Result<Read, Skip> {
    if !filter.name.matches(&c.name) {
        return Err(Skip::Name);
    }
    let extracted = scheme.extract_full(&c.version_text).map_err(Skip::Unread)?;
    if filter.latest_by == LatestBy::Version && extracted.version.parts.is_empty() {
        return Err(Skip::Unread(Ignored::NoVersion));
    }
    if (extracted.version.is_prerelease() || c.prerelease) && !filter.include_prereleases {
        return Err(Skip::Prerelease);
    }
    if let Some(id) = &extracted.id {
        if !filter.products.is_empty() && !filter.products.iter().any(|p| p == id) {
            return Err(Skip::Product);
        }
    }
    match filter.latest_by {
        LatestBy::Modified if c.published_at.is_none() => return Err(Skip::NoTime("modified time")),
        LatestBy::Created if c.created_at.is_none() => return Err(Skip::NoTime("creation time")),
        LatestBy::Name if digit_runs(&c.name).is_empty() => return Err(Skip::NoDigits),
        _ => {}
    }
    let identity = if extracted.version.text.is_empty() {
        c.name.clone()
    } else {
        extracted.version.text.clone()
    };
    Ok(Read {
        extracted,
        identity,
        candidate: c,
    })
}

/// What makes files one release: the same numbers (so `2.05` and `2.5` are one), else the same text.
fn group_key(r: &Read, latest_by: LatestBy) -> String {
    if latest_by == LatestBy::Version {
        let v = &r.extracted.version;
        let end = v.parts.iter().rposition(|&p| p != 0).map_or(0, |i| i + 1);
        format!("{:?}|{}", &v.parts[..end], v.pre.to_ascii_lowercase())
    } else {
        r.identity.clone()
    }
}

fn artifact_of(r: &Read, checksums: &BTreeMap<String, ChecksumFile>) -> Artifact {
    let c = &r.candidate;
    Artifact {
        name: c.name.clone(),
        location: c.location.clone(),
        kind: extension_of(&c.name),
        variant: r.extracted.variant.clone(),
        size: c.size,
        modified: c.published_at,
        created: c.created_at,
        checksum_file: checksums.get(&c.name.to_ascii_lowercase()).cloned(),
    }
}

/// Reads each candidate with `scheme`, applies `filter`, joins the files of one product and version into one release
/// (several artifacts) and returns at most `limit` per product, newest first according to `filter.latest_by`. Looks
/// at no more than [`MAX_FOLDER_MATCHES`] candidates. Checksum files next to another candidate are attached to it.
pub fn select(candidates: Vec<Candidate>, scheme: &VersionScheme, filter: &ReleaseFilter, limit: usize) -> Selection {
    let mut out = Selection::default();
    let candidates: Vec<Candidate> = candidates.into_iter().take(MAX_FOLDER_MATCHES).collect();
    let (files, checksums) = split_checksums(candidates);
    let mut groups: BTreeMap<(Option<String>, String), Vec<Read>> = BTreeMap::new();
    for c in files {
        let name = c.name.clone();
        match read(c, scheme, filter) {
            Ok(r) => groups
                .entry((r.extracted.id.clone(), group_key(&r, filter.latest_by)))
                .or_default()
                .push(r),
            Err(Skip::Name) => out.names_skipped += 1,
            Err(Skip::Prerelease) => out.prereleases_skipped += 1,
            Err(Skip::Unread(_) | Skip::NoDigits | Skip::NoTime(_)) => {
                if out.without_version.len() < 20 {
                    out.without_version.push(name);
                }
            }
            Err(Skip::Product | Skip::Checksum | Skip::Extension) => {}
        }
    }
    let mut releases: Vec<Release> = groups.into_values().map(|g| release_of(g, &checksums, filter.latest_by)).collect();
    releases.sort_by(|a, b| {
        a.product()
            .cmp(&b.product())
            .then_with(|| b.version.cmp(&a.version))
            .then_with(|| b.published_at.cmp(&a.published_at))
            .then_with(|| a.name.cmp(&b.name))
    });
    // One release per product and version (the numbers can be equal for differently written texts).
    releases.dedup_by(|later, first| later.product() == first.product() && later.version == first.version);
    let mut kept: BTreeMap<Option<String>, usize> = BTreeMap::new();
    releases.retain(|r| {
        let n = kept.entry(r.detail.product.clone()).or_insert(0);
        *n += 1;
        *n <= limit
    });
    out.releases = releases;
    out
}

fn split_checksums(candidates: Vec<Candidate>) -> (Vec<Candidate>, BTreeMap<String, ChecksumFile>) {
    let names: std::collections::BTreeSet<String> = candidates.iter().map(|c| c.name.to_ascii_lowercase()).collect();
    let mut sums = BTreeMap::new();
    let mut files = Vec::new();
    for c in candidates {
        match checksum_target(&c.name) {
            Some(target) if names.contains(&target.to_ascii_lowercase()) => {
                sums.insert(
                    target.to_ascii_lowercase(),
                    ChecksumFile {
                        name: c.name.clone(),
                        location: c.location.clone(),
                    },
                );
            }
            _ => files.push(c),
        }
    }
    (files, sums)
}

/// One release from the files of one product and version.
fn release_of(mut group: Vec<Read>, checksums: &BTreeMap<String, ChecksumFile>, latest_by: LatestBy) -> Release {
    // The primary file: the newest, then the first by name, so the choice is stable.
    group.sort_by(|a, b| {
        b.candidate
            .published_at
            .cmp(&a.candidate.published_at)
            .then_with(|| a.candidate.name.cmp(&b.candidate.name))
    });
    let artifacts: Vec<Artifact> = group.iter().map(|r| artifact_of(r, checksums)).collect();
    let newest_modified = group.iter().filter_map(|r| r.candidate.published_at).max();
    let newest_created = group.iter().filter_map(|r| r.candidate.created_at).max();
    let first = group.remove(0);
    let from_name = first.extracted.version.text.is_empty();
    let mut version = first.extracted.version.clone();
    let stamp = |t: Option<Timestamp>| t.map_or(0, |t| u64::try_from(t.0.div_euclid(1000)).unwrap_or(0));
    match latest_by {
        LatestBy::Version => {}
        LatestBy::Modified => version.parts = vec![stamp(newest_modified)],
        LatestBy::Created => version.parts = vec![stamp(newest_created)],
        LatestBy::Name => version.parts = digit_runs(&first.candidate.name),
    }
    if latest_by != LatestBy::Version {
        let when = if latest_by == LatestBy::Created {
            newest_created
        } else {
            newest_modified
        };
        version.text = match (from_name, latest_by, when) {
            (true, LatestBy::Modified | LatestBy::Created, Some(t)) => format!("{} · {}", first.candidate.name, display_time(t)),
            (true, _, _) => first.candidate.name.clone(),
            (false, ..) => version.text,
        };
    }
    Release {
        version,
        name: first.candidate.name.clone(),
        location: first.candidate.location.clone(),
        published_at: newest_modified,
        size: first.candidate.size,
        sha256: first.candidate.sha256,
        detail: ReleaseDetail {
            product: first.extracted.id.clone(),
            channel: first.extracted.channel.clone(),
            fields: first.extracted.fields.clone(),
            artifacts,
            updated_in_place: false,
        },
    }
}

/// One row of the wizard's name table.
#[derive(Debug, Clone, PartialEq)]
pub struct PreviewRow {
    /// The text read (the file name, folder name or relative path).
    pub name: String,
    /// What was read, or why not.
    pub read: Result<Extracted, Skip>,
}

/// Reads every candidate for the wizard's preview: one row each, in the order given (bounded by
/// [`MAX_FOLDER_MATCHES`]). Pure and cheap, so the wizard can ask on every keystroke.
pub fn preview_rows(candidates: Vec<Candidate>, scheme: &VersionScheme, filter: &ReleaseFilter) -> Vec<PreviewRow> {
    let candidates: Vec<Candidate> = candidates.into_iter().take(MAX_FOLDER_MATCHES).collect();
    let (files, _) = split_checksums(candidates.clone());
    let attached: std::collections::BTreeSet<&str> = candidates
        .iter()
        .filter(|c| !files.iter().any(|f| f.name == c.name))
        .map(|c| c.name.as_str())
        .collect();
    candidates
        .iter()
        .map(|c| PreviewRow {
            name: c.version_text.clone(),
            read: if attached.contains(c.name.as_str()) {
                Err(Skip::Checksum)
            } else {
                read(c.clone(), scheme, filter).map(|r| r.extracted)
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern::PatternSyntax;

    fn names(s: &Selection) -> Vec<&str> {
        s.releases.iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn selection_orders_dedupes_filters_and_bounds() {
        let cands = [
            "b-R1.01.00T10.zip",
            "a-R1.01.00T10.zip",
            "R1.02.00T01.zip",
            "readme.txt",
            "R0.09.00T99.zip",
            "R1.00.00T01.log",
        ]
        .map(|n| Candidate::named(n, &format!("/s/{n}")))
        .to_vec();
        let filter = ReleaseFilter {
            include_prereleases: false,
            name: NamePattern::new(PatternSyntax::Glob, "*.zip").unwrap(),
            ..ReleaseFilter::default()
        };
        let s = select(cands.clone(), &VersionScheme::BuildNumber, &filter, 10);
        assert_eq!(names(&s), ["R1.02.00T01.zip", "a-R1.01.00T10.zip", "R0.09.00T99.zip"]);
        assert_eq!(s.names_skipped, 2);
        assert!(s.without_version.is_empty());
        assert_eq!(select(cands.clone(), &VersionScheme::BuildNumber, &filter, 1).releases.len(), 1);
        let all = select(cands, &VersionScheme::BuildNumber, &ReleaseFilter::default(), 10);
        assert_eq!(all.without_version, ["readme.txt"]);
    }

    #[test]
    fn prereleases_are_skipped_unless_wanted() {
        let mut marked = Candidate::named("2.0.0", "l");
        marked.prerelease = true;
        let cands = vec![Candidate::named("1.0.0", "l"), Candidate::named("1.1.0-rc.1", "l"), marked];
        let s = select(cands.clone(), &VersionScheme::Semver, &ReleaseFilter::default(), 10);
        assert_eq!(names(&s), ["1.0.0"]);
        assert_eq!(s.prereleases_skipped, 2);
        let with = ReleaseFilter {
            include_prereleases: true,
            ..ReleaseFilter::default()
        };
        assert_eq!(
            names(&select(cands, &VersionScheme::Semver, &with, 10)),
            ["2.0.0", "1.1.0-rc.1", "1.0.0"]
        );
    }

    #[test]
    fn unversioned_names_are_reported_up_to_a_bound() {
        let cands = (0..30).map(|i| Candidate::named(&format!("file{i}"), "l")).collect();
        assert_eq!(
            select(cands, &VersionScheme::Semver, &ReleaseFilter::default(), 10)
                .without_version
                .len(),
            20
        );
        let many = (0..MAX_FOLDER_MATCHES + 10).map(|i| Candidate::named(&format!("1.{i}"), "l")).collect();
        let s = select(many, &VersionScheme::Semver, &ReleaseFilter::default(), usize::MAX);
        assert_eq!(s.releases.len(), MAX_FOLDER_MATCHES);
    }

    fn file(name: &str, mtime: Option<i64>) -> Candidate {
        let mut c = Candidate::named(name, &format!("/drops/{name}"));
        c.published_at = mtime.map(Timestamp::from_secs);
        c.created_at = mtime.map(|t| Timestamp::from_secs(t + 5));
        c.size = Some(100);
        c
    }

    fn acme() -> VersionScheme {
        VersionScheme::custom(
            r"Acme-(?<id>[A-Z]+)-R(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)T(?<build>\d+)(?:_(?<variant>x64|arm64))?(?:-(?<channel>beta|rc))?",
        )
        .unwrap()
    }

    #[test]
    fn products_are_tracked_separately_and_files_join_into_releases() {
        let cands = vec![
            file("Acme-PRO-R3.10.00T32.iso", Some(100)),
            file("Acme-PRO-R3.10.00T32_x64.exe", Some(120)),
            file("Acme-PRO-R3.10.00T32_x64.exe.sha256", Some(121)),
            file("Acme-PRO-R3.9.00T40.iso", Some(300)),
            file("Acme-LITE-R1.00.00T05.zip", Some(50)),
            file("Acme-LITE-R1.01.00T01-beta.zip", Some(60)),
            file("readme.txt", Some(1)),
        ];
        let s = select(cands.clone(), &acme(), &ReleaseFilter::default(), 10);
        let shown: Vec<_> = s.releases.iter().map(|r| (r.product().unwrap(), r.version.text.as_str())).collect();
        assert_eq!(shown, [("LITE", "R1.00.00T05"), ("PRO", "R3.10.00T32"), ("PRO", "R3.9.00T40")]);
        assert_eq!(s.without_version, ["readme.txt"]);
        assert_eq!(s.prereleases_skipped, 1);
        let pro = &s.releases[1];
        let kinds: Vec<_> = pro
            .detail
            .artifacts
            .iter()
            .map(|a| (a.name.as_str(), a.kind.as_str(), a.variant.as_str()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("Acme-PRO-R3.10.00T32_x64.exe", "exe", "x64"),
                ("Acme-PRO-R3.10.00T32.iso", "iso", "")
            ]
        );
        assert_eq!(
            pro.detail.artifacts[0].checksum_file.as_ref().map(|c| c.name.as_str()),
            Some("Acme-PRO-R3.10.00T32_x64.exe.sha256")
        );
        assert_eq!(pro.name, "Acme-PRO-R3.10.00T32_x64.exe", "the newest file is the primary one");
        assert_eq!(pro.published_at, Some(Timestamp::from_secs(120)));
        // A chosen artifact or variant.
        assert_eq!(pro.pick("x64", "").unwrap().kind, "exe");
        assert_eq!(pro.pick("", "ISO").unwrap().kind, "iso");
        assert_eq!(pro.pick("arm64", "").unwrap().kind, "exe", "falls back to the primary");
        assert_eq!(pro.with_artifact("", ".iso").location, "/drops/Acme-PRO-R3.10.00T32.iso");
        // Pre-releases, per-product limits and restricting to products.
        let with = ReleaseFilter {
            include_prereleases: true,
            products: vec!["LITE".into()],
            ..ReleaseFilter::default()
        };
        let s = select(cands.clone(), &acme(), &with, 1);
        assert_eq!(s.releases.len(), 1);
        assert_eq!(s.releases[0].version.text, "R1.01.00T01-beta");
        assert_eq!(s.releases[0].detail.channel, "beta");
        let limited = select(cands, &acme(), &ReleaseFilter::default(), 1);
        assert_eq!(limited.releases.len(), 2, "one per product");
    }

    #[test]
    fn zeros_do_not_change_order_and_old_builds_with_new_mtimes_stay_old() {
        let cands = vec![
            file("Acme-PRO-R3.9.00T40.iso", Some(9_000)),
            file("Acme-PRO-R3.10.00T01.iso", Some(1_000)),
        ];
        let s = select(cands, &acme(), &ReleaseFilter::default(), 10);
        assert_eq!(s.releases[0].version.text, "R3.10.00T01", "the version decides, not the mtime");
    }

    #[test]
    fn equal_versions_join_whatever_the_zeros() {
        let v = VersionScheme::custom(r"v(?<major>\d+)\.(?<minor>\d+)").unwrap();
        let s = select(
            vec![file("v2.05.txt", Some(1)), file("v2.5.zip", Some(2))],
            &v,
            &ReleaseFilter::default(),
            10,
        );
        assert_eq!(s.releases.len(), 1);
        assert_eq!(s.releases[0].detail.artifacts.len(), 2);
        assert_eq!(s.releases[0].name, "v2.5.zip", "ties go to the newer modified time");
        let tie = select(
            vec![file("v2.5.b", Some(1)), file("v2.5.a", Some(1))],
            &v,
            &ReleaseFilter::default(),
            10,
        );
        assert_eq!(tie.releases[0].name, "v2.5.a", "then to the name");
    }

    #[test]
    fn latest_by_modified_created_and_name() {
        let any = VersionScheme::custom(r"(?<id>[a-z]+)").unwrap();
        let by = |latest_by| ReleaseFilter {
            latest_by,
            ..ReleaseFilter::default()
        };
        let cands = vec![
            file("alpha.zip", Some(500)),
            file("beta.zip", Some(900)),
            file("gamma.zip", Some(100)),
        ];
        let m = select(cands.clone(), &any, &by(LatestBy::Modified), 10);
        let order: Vec<_> = m.releases.iter().map(|r| r.product().unwrap()).collect();
        assert_eq!(order, ["alpha", "beta", "gamma"], "products are separate");
        // One product, several files: the newest is the latest.
        let one = VersionScheme::custom(r"^(?<id>build)").unwrap();
        let builds = vec![file("build-a.zip", Some(500)), file("build-b.zip", Some(900))];
        let m = select(builds.clone(), &one, &by(LatestBy::Modified), 10);
        assert_eq!(m.releases.len(), 2);
        assert_eq!(m.releases[0].name, "build-b.zip");
        assert!(m.releases[0].version > m.releases[1].version);
        assert!(
            m.releases[0].version.text.starts_with("build-b.zip · 1970-01-01 00:15"),
            "{}",
            m.releases[0].version.text
        );
        let c = select(builds, &one, &by(LatestBy::Created), 10);
        assert_eq!(c.releases[0].name, "build-b.zip");
        // Names without a time are reported, not guessed.
        let none = select(vec![file("build-x.zip", None)], &one, &by(LatestBy::Modified), 10);
        assert!(none.releases.is_empty() && none.without_version == ["build-x.zip"]);
        // Natural name order.
        let names = vec![
            Candidate::named("build-2026-10-9", "l"),
            Candidate::named("build-2026-10-23", "l"),
            Candidate::named("notes", "l"),
        ];
        let any_name = VersionScheme::custom(r"^(?<id>build)").unwrap();
        let n = select(names, &any_name, &by(LatestBy::Name), 10);
        assert_eq!(n.releases.len(), 2);
        assert_eq!(n.releases[0].name, "build-2026-10-23");
        assert_eq!(n.releases[0].version.parts, vec![2026, 10, 23]);
    }

    #[test]
    fn a_file_overwritten_in_place_is_a_new_release_with_the_same_name() {
        let any = VersionScheme::custom(r"^(?<id>setup)").unwrap();
        let f = ReleaseFilter {
            latest_by: LatestBy::Modified,
            ..ReleaseFilter::default()
        };
        let before = select(vec![file("setup.exe", Some(1_000))], &any, &f, 10).releases;
        let after = select(vec![file("setup.exe", Some(2_000))], &any, &f, 10).releases;
        assert_eq!(before[0].name, after[0].name);
        assert!(after[0].version > before[0].version);
        assert_ne!(before[0].version.text, after[0].version.text);
    }

    #[test]
    fn extensions_and_checksum_names() {
        assert_eq!(extension_of("a.ISO"), "iso");
        assert_eq!(extension_of("a.tar.gz"), "tar.gz");
        assert_eq!(extension_of("noext"), "");
        assert_eq!(extension_of(".hidden"), "");
        assert_eq!(checksum_target("setup.exe.sha256"), Some("setup.exe"));
        assert_eq!(checksum_target("setup.exe.SHA512"), Some("setup.exe"));
        assert_eq!(checksum_target("setup.exe"), None);
        assert_eq!(checksum_target(".sha256"), None);
        // A checksum file without its file is a candidate like any other.
        let s = select(
            vec![file("app-1.0.0.zip.sha256", Some(1))],
            &VersionScheme::Semver,
            &ReleaseFilter::default(),
            3,
        );
        assert_eq!(s.releases.len(), 1);
    }

    #[test]
    fn preview_rows_say_why_names_are_ignored() {
        let cands = vec![
            file("Acme-PRO-R3.10.00T32.iso", Some(1)),
            file("Acme-PRO-R3.10.00T32.iso.sha256", Some(1)),
            file("readme.txt", Some(1)),
            file("Acme-QA-R1.00.00T01-beta.zip", Some(1)),
        ];
        let f = ReleaseFilter::default();
        let rows = preview_rows(cands.clone(), &acme(), &f);
        assert!(rows[0].read.is_ok());
        assert_eq!(rows[1].read, Err(Skip::Checksum));
        assert_eq!(rows[2].read, Err(Skip::Unread(Ignored::NoMatch)));
        assert_eq!(rows[3].read, Err(Skip::Prerelease));
        for r in &rows {
            if let Err(s) = &r.read {
                assert!(!s.reason().is_empty());
            }
        }
        let only = ReleaseFilter {
            products: vec!["PRO".into()],
            include_prereleases: true,
            ..ReleaseFilter::default()
        };
        assert_eq!(preview_rows(cands, &acme(), &only)[3].read, Err(Skip::Product));
        for s in [
            Skip::Name,
            Skip::NoTime("modified time"),
            Skip::NoDigits,
            Skip::Product,
            Skip::Prerelease,
        ] {
            assert!(s.reason().starts_with("ignored"));
        }
    }

    #[test]
    fn latest_by_parses() {
        for (s, v) in [
            ("version", LatestBy::Version),
            ("modified", LatestBy::Modified),
            ("created", LatestBy::Created),
            ("name", LatestBy::Name),
        ] {
            assert_eq!(LatestBy::parse(s), Ok(v));
        }
        assert!(LatestBy::parse("x").unwrap_err().contains("not a way"));
    }
}
