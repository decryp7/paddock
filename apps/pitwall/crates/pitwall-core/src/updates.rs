//! Model update notices (ADR 0017): what Pitwall knows about where an installed model came from, and the rules that
//! compare it with what Hugging Face lists now. Pure: the listings come in as parsed values, the machine's room and the
//! clock as arguments. Nothing here fetches, downloads or persists (see [`crate::update_state`] and the server's
//! `updates` module); a notice only ever *offers* a download, the user starts it.
//!
//! # Provenance
//!
//! At download time the server writes a small sidecar next to the model (`<file>.origin.json`, see [`sidecar_name`])
//! with the repository, the file, its size, the SHA-256 of every part and the repository's commit at that time.
//! Models downloaded before that (or copied in by hand under `owner/repo/`) have no sidecar: they are *inferred* from
//! their folder and size, without a hash. Models outside an `owner/repo` folder have no known origin and are never
//! looked up. Pitwall asks Hugging Face only about repository ids it downloaded from itself.
//!
//! # Rules
//!
//! For every installed model whose repository was answered:
//!
//! 1. **Newer revision of the same file.** The repository lists the same file and it differs from the copy on disk.
//!    With a recorded hash the comparison is the SHA-256 of every part. Without one (old entries) it is the total
//!    size, which differs whenever a file was re-quantised or re-uploaded. A new commit elsewhere in the repository
//!    that leaves this file alone is *not* a notice.
//! 2. **Better quantisation.** The repository offers the same base model (same file name without the quantisation
//!    label) in a higher-quality quantisation ([`quality_rank`]) than any copy of it you already have, up to `Q8_0`
//!    (16-bit and 32-bit weights are never suggested), and it fits this machine comfortably (`Verdict::Fits` for one
//!    slot at the default context). The offer is the *next step up* that fits, not the biggest.
//! 3. **Imatrix build.** The repository lists the same base and quantisation again as an importance-matrix build
//!    (`imatrix` in the name) while the copy you have is not one. Only offered when rule 2 found nothing.
//!
//! A model has at most one notice per rule. Notices are stable: the same finding gets the same id and fingerprint, so
//! a dismissal sticks until the repository offers something different.

use crate::fit::{self, ModelSize, Room, Verdict};
use crate::hf::{valid_file_path, RepoFiles, RepoId, Variant};
use crate::time::Millis;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Schema version of the provenance sidecar.
pub const PROVENANCE_VERSION: u32 = 1;
/// The largest sidecar read.
pub const MAX_PROVENANCE_BYTES: usize = 16 * 1024;
/// Most parts of one model whose hashes are kept.
pub const MAX_PARTS: usize = 256;

/// The sidecar's file name for a model file (`model.gguf` → `model.gguf.origin.json`).
pub fn sidecar_name(file_name: &str) -> String {
    format!("{file_name}.origin.json")
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_revision(s: &str) -> bool {
    (7..=64).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Where a model came from, recorded when it was downloaded (the sidecar's content).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    /// Schema version.
    pub version: u32,
    /// `owner/name`.
    pub repo: String,
    /// The file inside the repository (the first part of a split set).
    pub file: String,
    /// Bytes of all parts on disk after the download.
    pub size: u64,
    /// SHA-256 per part, empty when unknown.
    pub sha256: Vec<String>,
    /// The repository commit at download time, when it was known.
    pub revision: Option<String>,
    /// When it finished (Unix ms).
    pub downloaded_at: Millis,
}

impl Provenance {
    /// A record for a finished download. Hashes that are not 64 hex digits and revisions that are not a commit id
    /// are dropped rather than stored.
    pub fn new(repo: &str, file: &str, size: u64, sha256: Vec<String>, revision: Option<String>, at: Millis) -> Provenance {
        let sha256 = if sha256.len() <= MAX_PARTS && sha256.iter().all(|s| is_sha256(s)) {
            sha256.into_iter().map(|s| s.to_ascii_lowercase()).collect()
        } else {
            vec![]
        };
        Provenance {
            version: PROVENANCE_VERSION,
            repo: repo.to_string(),
            file: file.to_string(),
            size,
            sha256,
            revision: revision.filter(|r| is_revision(r)).map(|r| r.to_ascii_lowercase()),
            downloaded_at: at,
        }
    }

    /// The sidecar text.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Reads a sidecar. Damaged text, a newer schema or fields that do not parse give `None` (the model is then
    /// treated like one without a record).
    pub fn parse(text: &str) -> Option<Provenance> {
        if text.len() > MAX_PROVENANCE_BYTES {
            return None;
        }
        let p: Provenance = serde_json::from_str(text).ok()?;
        if p.version == 0 || p.version > PROVENANCE_VERSION {
            return None;
        }
        RepoId::parse(&p.repo).ok()?;
        valid_file_path(&p.file).then_some(())?;
        Some(Provenance::new(&p.repo, &p.file, p.size, p.sha256, p.revision, p.downloaded_at))
    }
}

/// An installed model as the rules see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The id clients use.
    pub model_id: String,
    /// The repository it came from.
    pub repo: RepoId,
    /// The file in the repository (first part).
    pub file: String,
    /// Its quantisation label, when known.
    pub quant: Option<String>,
    /// Total bytes on disk.
    pub size: u64,
    /// SHA-256 per part, empty when never recorded.
    pub sha256: Vec<String>,
    /// The repository commit when downloaded, when recorded.
    pub revision: Option<String>,
}

impl Installed {
    /// Builds the record for a model found on disk. `repo` and `file` come from where the file sits
    /// (`models/<owner>/<repo>/<file>`); `sidecar` is the content of its provenance file, if there is one. The sidecar
    /// is used only when it names the same repository and file and the size on disk still matches (a file replaced by
    /// hand is not what was recorded). `None` when the model did not come from a repository folder.
    pub fn from_disk(model_id: &str, repo: &str, file: &str, quant: Option<&str>, size: u64, sidecar: Option<&str>) -> Option<Installed> {
        let repo = RepoId::parse(repo).ok()?;
        valid_file_path(file).then_some(())?;
        let known = sidecar
            .and_then(Provenance::parse)
            .filter(|p| p.repo == repo.as_str() && p.file == file && p.size == size);
        Some(Installed {
            model_id: model_id.to_string(),
            repo,
            file: file.to_string(),
            quant: quant.map(str::to_string).or_else(|| crate::quant::label_from_file_name(file)),
            size,
            sha256: known.as_ref().map(|p| p.sha256.clone()).unwrap_or_default(),
            revision: known.and_then(|p| p.revision),
        })
    }
}

/// What Hugging Face lists for one repository now.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepoSnapshot {
    /// The repository's current commit, when the info answer had one.
    pub revision: Option<String>,
    /// Its GGUF files.
    pub files: RepoFiles,
}

/// The commit id in a model-info answer (`sha`), when it is one.
pub fn parse_revision(info: &Value) -> Option<String> {
    info.get("sha")
        .and_then(Value::as_str)
        .filter(|s| is_revision(s))
        .map(str::to_ascii_lowercase)
}

/// How good a quantisation is, higher is better, `None` when the label is not one Pitwall can order
/// (`Q4_K_M`, `IQ3_XS`, `Q8_0`, `F16`, …). Ordered by bits first, then by the variant within a bit width
/// (`XXS` < `XS` < `S` < `M` < legacy `_0`/`_1` < `K`/`K_S` < `K_M` < `K_L`).
pub fn quality_rank(label: &str) -> Option<u32> {
    let u = label.trim().to_ascii_uppercase();
    match u.as_str() {
        "F32" => return Some(320),
        "F16" | "BF16" => return Some(160),
        _ => {}
    }
    let rest = u.strip_prefix("IQ").or_else(|| u.strip_prefix('Q'))?;
    let split = rest.find(|c: char| !c.is_ascii_digit()).unwrap_or(rest.len());
    let (digits, suffix) = rest.split_at(split);
    let bits: u32 = digits.parse().ok().filter(|b| (1..=8).contains(b))?;
    let sub = match suffix.trim_start_matches('_') {
        "XXS" => 0,
        "XS" => 1,
        "S" => 2,
        "M" | "NL" => 3,
        "0" => 4,
        "1" => 5,
        "K" | "K_S" => 6,
        "K_M" => 7,
        "K_L" => 8,
        _ => return None,
    };
    Some(bits * 10 + sub)
}

/// The highest rank Pitwall ever suggests (`Q8_0`).
const MAX_SUGGESTED_RANK: u32 = 84;

/// A file name without directory, extension, split suffix, quantisation label and `imatrix` marker, lower-case: two
/// files with the same base are the same model in different builds.
pub fn base_name(file: &str) -> String {
    let name = file.rsplit('/').next().unwrap_or(file);
    let stem = crate::ids::strip_split_suffix(name.strip_suffix(".gguf").unwrap_or(name)).to_ascii_lowercase();
    let mut s = stem.clone();
    if let Some(label) = crate::quant::label_from_file_name(name) {
        if let Some(i) = stem.rfind(&label.to_ascii_lowercase()) {
            s.replace_range(i..i + label.len(), "");
        }
    }
    s = s.replace("imatrix", "");
    s.split(['-', '.', '_']).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("-")
}

fn is_imatrix(file: &str) -> bool {
    file.to_ascii_lowercase().contains("imatrix")
}

/// What kind of update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateKind {
    /// The same file changed in the repository.
    NewerRevision,
    /// A higher-quality quantisation that fits.
    BetterQuant,
    /// An importance-matrix build of the same quantisation.
    Imatrix,
}

impl UpdateKind {
    /// The kind in ids.
    pub fn as_str(self) -> &'static str {
        match self {
            UpdateKind::NewerRevision => "newerRevision",
            UpdateKind::BetterQuant => "betterQuant",
            UpdateKind::Imatrix => "imatrix",
        }
    }
}

/// One offered update (`UpdateNotice` in `api.types.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNotice {
    /// `<model id>#<kind>`; stable across checks.
    pub id: String,
    /// The installed model it is about.
    pub model_id: String,
    /// The repository.
    pub repo: String,
    /// What kind.
    pub kind: UpdateKind,
    /// The file to download (for a newer revision, the file you have).
    pub file: String,
    /// Its quantisation.
    pub quant: Option<String>,
    /// Bytes to download.
    pub size_bytes: u64,
    /// The file you have.
    pub installed_file: String,
    /// The quantisation you have.
    pub installed_quant: Option<String>,
    /// One sentence for the user.
    pub summary: String,
    /// When this finding was first made (Unix ms).
    pub found_at: Millis,
    /// SHA-256 of the offered file when Hugging Face listed one (the first part of a split set).
    pub target_sha256: Option<String>,
}

impl UpdateNotice {
    /// What the offer is, for dismissals: a dismissal applies to this fingerprint only, so a different offer for the
    /// same model (another file, a changed file) is shown again.
    pub fn fingerprint(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.kind.as_str(),
            self.file,
            self.size_bytes,
            self.target_sha256.as_deref().unwrap_or("")
        )
    }
}

/// What the rules need besides the listings.
#[derive(Debug, Clone, Copy)]
pub struct Basis<'a> {
    /// The context a model is judged at (the default context, one slot).
    pub context: u32,
    /// The machine's room now.
    pub room: &'a Room,
    /// The clock.
    pub now: Millis,
}

fn changed(have: &Installed, v: &Variant) -> bool {
    if !have.sha256.is_empty() && !v.sha256.is_empty() {
        have.sha256 != v.sha256
    } else {
        have.size != v.size
    }
}

fn notice(have: &Installed, kind: UpdateKind, v: &Variant, summary: String, now: Millis) -> UpdateNotice {
    UpdateNotice {
        id: format!("{}#{}", have.model_id, kind.as_str()),
        model_id: have.model_id.clone(),
        repo: have.repo.as_str().to_string(),
        kind,
        file: v.file.clone(),
        quant: v.quant.clone(),
        size_bytes: v.size,
        installed_file: have.file.clone(),
        installed_quant: have.quant.clone(),
        summary,
        found_at: now,
        target_sha256: v.sha256.first().cloned(),
    }
}

fn better_quant(have: &Installed, all: &[Installed], snap: &RepoSnapshot, basis: &Basis<'_>) -> Option<UpdateNotice> {
    let mine = have.quant.as_deref().and_then(quality_rank)?;
    let base = base_name(&have.file);
    // The best copy of this base you already have in this repository: only that one is offered an upgrade.
    let best = all
        .iter()
        .filter(|o| o.repo == have.repo && base_name(&o.file) == base)
        .filter_map(|o| o.quant.as_deref().and_then(quality_rank))
        .max()?;
    if mine < best {
        return None;
    }
    let next = snap
        .files
        .variants
        .iter()
        .filter(|v| base_name(&v.file) == base && is_imatrix(&v.file) == is_imatrix(&have.file))
        .filter_map(|v| Some((quality_rank(v.quant.as_deref()?)?, v)))
        .filter(|(r, v)| *r > best && *r <= MAX_SUGGESTED_RANK && v.size > 0)
        .filter(|(_, v)| {
            let size = ModelSize::from_file(v.size, v.quant.as_deref());
            fit::assess(Some(&size), basis.context, basis.room).verdict == Verdict::Fits
        })
        .min_by_key(|(r, _)| *r)?;
    let (_, v) = next;
    let label = v.quant.as_deref().unwrap_or("a higher quantisation");
    let summary = format!(
        "{label} ({}) is available and fits this machine; you have {}.",
        fit::gib(v.size),
        have.quant.as_deref().unwrap_or("a lower quantisation")
    );
    Some(notice(have, UpdateKind::BetterQuant, v, summary, basis.now))
}

fn imatrix_build(have: &Installed, snap: &RepoSnapshot, basis: &Basis<'_>) -> Option<UpdateNotice> {
    if is_imatrix(&have.file) {
        return None;
    }
    let base = base_name(&have.file);
    let label = have.quant.as_deref()?.to_ascii_uppercase();
    let v = snap.files.variants.iter().find(|v| {
        is_imatrix(&v.file)
            && base_name(&v.file) == base
            && v.quant.as_deref().map(str::to_ascii_uppercase).as_deref() == Some(label.as_str())
    })?;
    let summary = format!(
        "An importance-matrix build of {label} ({}) is available; it usually keeps more quality at the same size.",
        fit::gib(v.size)
    );
    Some(notice(have, UpdateKind::Imatrix, v, summary, basis.now))
}

/// Which repositories one check asks about: at most `max`, taken as a window over the sorted list that starts at a
/// different place each day (`seed`), so every repository gets its turn when there are more than `max`. Returns the
/// chosen ones and how many were left for another day.
pub fn pick_repos(mut repos: Vec<String>, seed: u64, max: usize) -> (Vec<String>, usize) {
    repos.sort();
    repos.dedup();
    if repos.len() <= max {
        return (repos, 0);
    }
    let start = (seed % repos.len() as u64) as usize;
    repos.rotate_left(start);
    let left = repos.len() - max;
    repos.truncate(max);
    (repos, left)
}

/// The notices for `installed` given the repositories' current listings (`snapshots` by repository id). A repository
/// with no snapshot (it could not be asked, or has no listing) produces nothing for its models.
pub fn evaluate(installed: &[Installed], snapshots: &BTreeMap<String, RepoSnapshot>, basis: &Basis<'_>) -> Vec<UpdateNotice> {
    let mut out = vec![];
    for have in installed {
        let Some(snap) = snapshots.get(have.repo.as_str()) else { continue };
        if let Some(v) = snap.files.variants.iter().find(|v| v.file == have.file) {
            if changed(have, v) {
                let short = |r: &str| r.chars().take(7).collect::<String>();
                let revisions = match (&have.revision, &snap.revision) {
                    (Some(a), Some(b)) if a != b => format!(" (repository revision {}, now {})", short(a), short(b)),
                    _ => String::new(),
                };
                let summary = format!(
                    "{} changed on Hugging Face since you downloaded it{revisions}.",
                    v.file.rsplit('/').next().unwrap_or(&v.file)
                );
                out.push(notice(have, UpdateKind::NewerRevision, v, summary, basis.now));
            }
        }
        match better_quant(have, installed, snap, basis) {
            Some(n) => out.push(n),
            None => out.extend(imatrix_build(have, snap, basis)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GIB;

    fn big_room() -> Room {
        Room {
            ram_total: 64 * GIB,
            ram_available: 48 * GIB,
            ram_releasable: 0,
            vram_free: None,
            unified: false,
        }
    }

    fn small_room() -> Room {
        Room {
            ram_total: 8 * GIB,
            ram_available: 6 * GIB,
            ram_releasable: 0,
            vram_free: None,
            unified: false,
        }
    }

    fn sha(c: char) -> String {
        c.to_string().repeat(64)
    }

    fn var(file: &str, size: u64, h: Option<char>) -> Variant {
        Variant {
            file: file.into(),
            parts: vec![file.into()],
            quant: crate::quant::label_from_file_name(file),
            size,
            sha256: h.map(|c| vec![sha(c)]).unwrap_or_default(),
        }
    }

    fn snap(vs: Vec<Variant>) -> BTreeMap<String, RepoSnapshot> {
        BTreeMap::from([(
            "acme/Tiny-GGUF".to_string(),
            RepoSnapshot {
                revision: None,
                files: RepoFiles {
                    variants: vs,
                    projectors: vec![],
                },
            },
        )])
    }

    fn have(file: &str, size: u64, h: Option<char>) -> Installed {
        Installed {
            model_id: file.trim_end_matches(".gguf").to_ascii_lowercase(),
            repo: RepoId::parse("acme/Tiny-GGUF").unwrap(),
            file: file.into(),
            quant: crate::quant::label_from_file_name(file),
            size,
            sha256: h.map(|c| vec![sha(c)]).unwrap_or_default(),
            revision: None,
        }
    }

    fn basis(room: &Room) -> Basis<'_> {
        Basis {
            context: 8192,
            room,
            now: 1_000,
        }
    }

    #[test]
    fn provenance_round_trips_and_sanitises() {
        let p = Provenance::new(
            "acme/Tiny-GGUF",
            "tiny-Q4_K_M.gguf",
            5,
            vec![sha('A')],
            Some("ABCDEF1234567".into()),
            9,
        );
        assert_eq!(p.sha256, vec![sha('a')]);
        assert_eq!(p.revision.as_deref(), Some("abcdef1234567"));
        assert_eq!(Provenance::parse(&p.to_json()), Some(p));
        let bad = Provenance::new("acme/x", "x.gguf", 1, vec!["nothex".into()], Some("zz".into()), 0);
        assert!(bad.sha256.is_empty() && bad.revision.is_none(), "invalid values are dropped");
        assert_eq!(sidecar_name("m.gguf"), "m.gguf.origin.json");
    }

    #[test]
    fn damaged_or_foreign_sidecars_are_ignored() {
        let ok = Provenance::new("acme/Tiny-GGUF", "t.gguf", 1, vec![], None, 0);
        let mut newer = ok.clone();
        newer.version = 99;
        let mut zero = ok.clone();
        zero.version = 0;
        let mut bad_repo = ok.clone();
        bad_repo.repo = "../x".into();
        let mut bad_file = ok.clone();
        bad_file.file = "../evil.gguf".into();
        for text in [
            "".to_string(),
            "{".into(),
            "[]".into(),
            "{\"version\":1}".into(),
            newer.to_json(),
            zero.to_json(),
            bad_repo.to_json(),
            bad_file.to_json(),
            "x".repeat(MAX_PROVENANCE_BYTES + 1),
        ] {
            assert!(Provenance::parse(&text).is_none(), "{text:.40}");
        }
    }

    #[test]
    fn installed_models_use_the_sidecar_only_when_it_describes_the_file() {
        let p = Provenance::new("acme/Tiny-GGUF", "t-Q4_K_M.gguf", 7, vec![sha('a')], Some("abc1234".into()), 0).to_json();
        let i = Installed::from_disk("t", "acme/Tiny-GGUF", "t-Q4_K_M.gguf", None, 7, Some(&p)).unwrap();
        assert_eq!(
            (i.sha256.len(), i.revision.as_deref(), i.quant.as_deref()),
            (1, Some("abc1234"), Some("Q4_K_M"))
        );
        // Replaced by hand (size differs), or another file's sidecar: treated as an old entry without a record.
        for (file, size) in [("t-Q4_K_M.gguf", 8), ("other-Q4_K_M.gguf", 7)] {
            let i = Installed::from_disk("t", "acme/Tiny-GGUF", file, Some("Q4_K_M"), size, Some(&p)).unwrap();
            assert!(i.sha256.is_empty() && i.revision.is_none(), "{file} {size}");
        }
        let old = Installed::from_disk("t", "acme/Tiny-GGUF", "t.gguf", None, 7, None).unwrap();
        assert!(old.quant.is_none() && old.sha256.is_empty());
        // No repository folder, or a file path that cannot be a repository file: nothing to ask.
        assert!(Installed::from_disk("t", "", "t.gguf", None, 7, None).is_none());
        assert!(Installed::from_disk("t", "acme/Tiny-GGUF", "../t.gguf", None, 7, None).is_none());
    }

    #[test]
    fn revisions_come_from_the_info_answer() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(parse_revision(&serde_json::json!({"sha": sha})).as_deref(), Some(sha));
        for v in [
            serde_json::json!({}),
            serde_json::json!({"sha": 5}),
            serde_json::json!({"sha": "main"}),
        ] {
            assert!(parse_revision(&v).is_none());
        }
    }

    #[test]
    fn quantisations_are_ordered_by_quality() {
        let order = [
            "IQ2_XXS", "Q2_K", "IQ3_XS", "Q3_K_M", "IQ4_XS", "Q4_0", "Q4_K_S", "Q4_K_M", "Q5_K_M", "Q6_K", "Q8_0", "BF16", "F32",
        ];
        let ranks: Vec<u32> = order.iter().map(|l| quality_rank(l).unwrap()).collect();
        assert!(ranks.windows(2).all(|w| w[0] < w[1]), "{ranks:?}");
        assert_eq!(quality_rank("q4_k_m"), quality_rank("Q4_K_M"));
        assert_eq!(quality_rank("Q4_1"), Some(45));
        assert_eq!(quality_rank("Q4_K_L"), Some(48));
        for bad in ["", "Q", "Q9_0", "Q0_0", "Q4_K_XL", "weird", "IQ", "Q4_X"] {
            assert_eq!(quality_rank(bad), None, "{bad}");
        }
    }

    #[test]
    fn base_names_ignore_quantisation_splits_and_imatrix() {
        assert_eq!(base_name("Tiny-8B-Q4_K_M.gguf"), base_name("dir/Tiny-8B-Q8_0.gguf"));
        assert_eq!(base_name("Tiny-8B-Q4_K_M.gguf"), base_name("Tiny-8B-imatrix-Q4_K_M.gguf"));
        assert_eq!(base_name("big/Tiny-8B-Q8_0-00001-of-00002.gguf"), base_name("Tiny-8B-Q4_K_M.gguf"));
        assert_ne!(base_name("Tiny-8B-Q4_K_M.gguf"), base_name("Tiny-14B-Q4_K_M.gguf"));
        assert_eq!(base_name("plain.gguf"), "plain");
    }

    #[test]
    fn repositories_are_picked_in_a_rotating_window() {
        let repos: Vec<String> = ["d/x", "a/x", "c/x", "b/x", "a/x"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            pick_repos(repos.clone(), 7, 10),
            (vec!["a/x".into(), "b/x".into(), "c/x".into(), "d/x".into()], 0)
        );
        assert_eq!(pick_repos(repos.clone(), 0, 2), (vec!["a/x".into(), "b/x".into()], 2));
        assert_eq!(pick_repos(repos.clone(), 1, 2), (vec!["b/x".into(), "c/x".into()], 2));
        assert_eq!(
            pick_repos(repos.clone(), 3, 2),
            (vec!["d/x".into(), "a/x".into()], 2),
            "wraps around"
        );
        assert_eq!(pick_repos(vec![], 5, 2), (vec![], 0));
    }

    #[test]
    fn the_summary_names_the_revisions_when_both_are_known() {
        let room = big_room();
        let mut h = have("Tiny-Q4_K_M.gguf", 100, Some('a'));
        h.revision = Some("1111111aaaa".into());
        let mut s = snap(vec![var("Tiny-Q4_K_M.gguf", 100, Some('b'))]);
        s.get_mut("acme/Tiny-GGUF").unwrap().revision = Some("2222222bbbb".into());
        let n = evaluate(&[h], &s, &basis(&room));
        assert!(n[0].summary.contains("revision 1111111, now 2222222"), "{}", n[0].summary);
    }

    #[test]
    fn a_changed_file_is_a_newer_revision_by_hash() {
        let room = big_room();
        let installed = [have("Tiny-Q4_K_M.gguf", 100, Some('a'))];
        let same = snap(vec![var("Tiny-Q4_K_M.gguf", 100, Some('a'))]);
        assert!(evaluate(&installed, &same, &basis(&room)).is_empty());
        let changed = snap(vec![var("Tiny-Q4_K_M.gguf", 100, Some('b'))]);
        let n = evaluate(&installed, &changed, &basis(&room));
        assert_eq!(n.len(), 1);
        assert_eq!(
            (n[0].kind, n[0].id.as_str(), n[0].file.as_str()),
            (UpdateKind::NewerRevision, "tiny-q4_k_m#newerRevision", "Tiny-Q4_K_M.gguf")
        );
        assert_eq!(n[0].target_sha256, Some(sha('b')));
        assert!(n[0].summary.contains("changed on Hugging Face"));
    }

    #[test]
    fn old_entries_without_a_hash_compare_by_size() {
        let room = big_room();
        let old = [have("Tiny-Q4_K_M.gguf", 100, None)];
        assert!(evaluate(&old, &snap(vec![var("Tiny-Q4_K_M.gguf", 100, Some('b'))]), &basis(&room)).is_empty());
        assert_eq!(
            evaluate(&old, &snap(vec![var("Tiny-Q4_K_M.gguf", 120, Some('b'))]), &basis(&room)).len(),
            1
        );
        // The listing has no hash either: same rule.
        let known = [have("Tiny-Q4_K_M.gguf", 100, Some('a'))];
        assert_eq!(
            evaluate(&known, &snap(vec![var("Tiny-Q4_K_M.gguf", 90, None)]), &basis(&room)).len(),
            1
        );
    }

    #[test]
    fn other_files_changing_or_the_file_going_away_is_not_a_notice() {
        let room = big_room();
        let installed = [have("Tiny-Q8_0.gguf", 100, Some('a'))];
        let s = snap(vec![var("Tiny-Q4_K_M.gguf", 50, Some('z')), var("Other-Q8_0.gguf", 100, Some('q'))]);
        assert!(evaluate(&installed, &s, &basis(&room)).is_empty());
        assert!(
            evaluate(&installed, &BTreeMap::new(), &basis(&room)).is_empty(),
            "no snapshot, no notice"
        );
        assert!(evaluate(&[], &s, &basis(&room)).is_empty());
    }

    #[test]
    fn a_better_quantisation_is_the_next_step_up_that_fits() {
        let room = big_room();
        let installed = [have("Tiny-Q4_K_M.gguf", 4 * GIB, Some('a'))];
        let s = snap(vec![
            var("Tiny-Q4_K_M.gguf", 4 * GIB, Some('a')),
            var("Tiny-Q3_K_M.gguf", 3 * GIB, Some('c')),
            var("Tiny-Q5_K_M.gguf", 5 * GIB, Some('d')),
            var("Tiny-Q8_0.gguf", 8 * GIB, Some('e')),
            var("Tiny-BF16.gguf", 16 * GIB, Some('f')),
            var("Tiny-IQ9_X.gguf", GIB, Some('g')),
        ]);
        let n = evaluate(&installed, &s, &basis(&room));
        assert_eq!(n.len(), 1, "{n:?}");
        assert_eq!(
            (n[0].kind, n[0].file.as_str(), n[0].quant.as_deref()),
            (UpdateKind::BetterQuant, "Tiny-Q5_K_M.gguf", Some("Q5_K_M"))
        );
        assert_eq!(n[0].installed_quant.as_deref(), Some("Q4_K_M"));
        assert!(n[0].summary.contains("Q5_K_M") && n[0].summary.contains("fits"));
        assert_eq!(n[0].id, "tiny-q4_k_m#betterQuant");
    }

    #[test]
    fn a_quantisation_that_does_not_fit_is_not_offered() {
        let room = small_room();
        let installed = [have("Tiny-Q4_K_M.gguf", 5 * GIB / 2, Some('a'))];
        let s = snap(vec![
            var("Tiny-Q4_K_M.gguf", 5 * GIB / 2, Some('a')),
            var("Tiny-Q8_0.gguf", 9 * GIB, Some('e')),
        ]);
        assert!(evaluate(&installed, &s, &basis(&room)).is_empty());
        // The next step up that fits is offered even when a bigger one does not.
        let s = snap(vec![
            var("Tiny-Q4_K_M.gguf", 5 * GIB / 2, Some('a')),
            var("Tiny-Q5_K_M.gguf", 3 * GIB, Some('d')),
            var("Tiny-Q8_0.gguf", 9 * GIB, Some('e')),
        ]);
        let n = evaluate(&installed, &s, &basis(&room));
        assert_eq!(n[0].file, "Tiny-Q5_K_M.gguf");
    }

    #[test]
    fn only_the_best_copy_you_have_is_offered_an_upgrade_and_other_models_are_left_alone() {
        let room = big_room();
        let installed = [
            have("Tiny-Q4_K_M.gguf", 4 * GIB, Some('a')),
            have("Tiny-Q6_K.gguf", 6 * GIB, Some('b')),
        ];
        let s = snap(vec![
            var("Tiny-Q4_K_M.gguf", 4 * GIB, Some('a')),
            var("Tiny-Q5_K_M.gguf", 5 * GIB, Some('d')),
            var("Tiny-Q6_K.gguf", 6 * GIB, Some('b')),
            var("Tiny-Q8_0.gguf", 8 * GIB, Some('e')),
            var("Huge-Q8_0.gguf", 8 * GIB, Some('h')),
        ]);
        let n = evaluate(&installed, &s, &basis(&room));
        assert_eq!(n.len(), 1, "{n:?}");
        assert_eq!((n[0].model_id.as_str(), n[0].file.as_str()), ("tiny-q6_k", "Tiny-Q8_0.gguf"));
        // Already the best that is ever suggested, or a label Pitwall cannot order: nothing.
        let top = [have("Tiny-Q8_0.gguf", 8 * GIB, Some('e'))];
        assert!(evaluate(&top, &s, &basis(&room)).is_empty());
        let odd = [have("Tiny-weird.gguf", 8 * GIB, Some('e'))];
        assert!(evaluate(&odd, &s, &basis(&room)).is_empty());
    }

    #[test]
    fn an_imatrix_build_of_the_same_quantisation_is_offered_when_nothing_better_is() {
        let room = big_room();
        let installed = [have("Tiny-Q8_0.gguf", 8 * GIB, Some('a'))];
        let s = snap(vec![
            var("Tiny-Q8_0.gguf", 8 * GIB, Some('a')),
            var("Tiny-imatrix-Q8_0.gguf", 8 * GIB, Some('b')),
        ]);
        let n = evaluate(&installed, &s, &basis(&room));
        assert_eq!(n.len(), 1);
        assert_eq!((n[0].kind, n[0].file.as_str()), (UpdateKind::Imatrix, "Tiny-imatrix-Q8_0.gguf"));
        // Already an imatrix build: no offer; a different quantisation of the imatrix build is not "the same".
        let have_i = [have("Tiny-imatrix-Q8_0.gguf", 8 * GIB, Some('b'))];
        assert!(evaluate(&have_i, &s, &basis(&room)).is_empty());
        let q4 = [have("Tiny-Q4_K_M.gguf", 4 * GIB, Some('a'))];
        let s4 = snap(vec![
            var("Tiny-Q4_K_M.gguf", 4 * GIB, Some('a')),
            var("Tiny-imatrix-Q8_0.gguf", 8 * GIB, Some('b')),
        ]);
        assert!(
            evaluate(&q4, &s4, &basis(&room)).is_empty(),
            "an imatrix build of another quantisation is not an upgrade of this one"
        );
        let both = snap(vec![
            var("Tiny-Q4_K_M.gguf", 4 * GIB, Some('a')),
            var("Tiny-imatrix-Q4_K_M.gguf", 4 * GIB, Some('b')),
            var("Tiny-Q5_K_M.gguf", 5 * GIB, Some('d')),
        ]);
        let n = evaluate(&q4, &both, &basis(&room));
        assert_eq!(
            (n.len(), n[0].kind),
            (1, UpdateKind::BetterQuant),
            "a better quantisation wins over the imatrix offer"
        );
        // An imatrix copy is only upgraded to imatrix builds of a higher quantisation.
        let have_i4 = [have("Tiny-imatrix-Q4_K_M.gguf", 4 * GIB, Some('a'))];
        let mixed = snap(vec![
            var("Tiny-imatrix-Q4_K_M.gguf", 4 * GIB, Some('a')),
            var("Tiny-Q5_K_M.gguf", 5 * GIB, Some('d')),
            var("Tiny-imatrix-Q6_K.gguf", 6 * GIB, Some('e')),
        ]);
        let n = evaluate(&have_i4, &mixed, &basis(&room));
        assert_eq!(n[0].file, "Tiny-imatrix-Q6_K.gguf");
    }

    #[test]
    fn a_changed_file_and_a_better_quantisation_are_two_notices_with_stable_fingerprints() {
        let room = big_room();
        let installed = [have("Tiny-Q4_K_M.gguf", 4 * GIB, Some('a'))];
        let s = snap(vec![
            var("Tiny-Q4_K_M.gguf", 4 * GIB, Some('z')),
            var("Tiny-Q5_K_M.gguf", 5 * GIB, Some('d')),
        ]);
        let a = evaluate(&installed, &s, &basis(&room));
        let kinds: Vec<_> = a.iter().map(|n| n.kind).collect();
        assert_eq!(kinds, vec![UpdateKind::NewerRevision, UpdateKind::BetterQuant]);
        let later = Basis { now: 99, ..basis(&room) };
        let b = evaluate(&installed, &s, &later);
        assert_eq!(a[0].fingerprint(), b[0].fingerprint(), "the fingerprint ignores when it was found");
        assert_ne!(a[0].fingerprint(), a[1].fingerprint());
    }
}
