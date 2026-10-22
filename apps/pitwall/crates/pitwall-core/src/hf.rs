//! Hugging Face: the URLs Pitwall fetches (search, file list, model info, file download) and the parsing of the
//! answers (untrusted JSON, parsed once into typed values with limits). Only GGUF files are offered; split models
//! are grouped; vision projectors (`mmproj`) are listed apart. Fetching is the server's egress module's job.

use crate::ids::split_part;
use crate::privacy::{DownloadUrl, EgressError};
use crate::quant::label_from_file_name;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Most search results shown.
pub const SEARCH_LIMIT: usize = 40;
/// Most files read from a repository listing.
pub const MAX_FILES: usize = 5000;
/// Longest search query.
pub const MAX_QUERY: usize = 200;

/// A repository id `owner/name`, validated.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RepoId(String);

impl RepoId {
    /// Parses `owner/name` (letters, digits, `-`, `_`, `.`; no `..`).
    pub fn parse(s: &str) -> Result<RepoId, String> {
        let s = s.trim();
        let ok_part = |p: &str| {
            !p.is_empty()
                && p.len() <= 96
                && !p.starts_with('.')
                && !p.contains("..")
                && p.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        };
        match s.split_once('/') {
            Some((a, b)) if ok_part(a) && ok_part(b) => Ok(RepoId(s.to_string())),
            _ => Err(format!(
                "{:?} is not a Hugging Face repository (owner/name)",
                s.chars().take(100).collect::<String>()
            )),
        }
    }

    /// As text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for RepoId {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        RepoId::parse(&s)
    }
}

impl From<RepoId> for String {
    fn from(r: RepoId) -> String {
        r.0
    }
}

/// Whether a path inside a repository is a safe GGUF file path (`dir/x.gguf`, no `..`, no backslashes).
pub fn valid_file_path(p: &str) -> bool {
    p.len() <= 300
        && p.ends_with(".gguf")
        && !p.starts_with('/')
        && !p.contains("..")
        && !p.contains('\\')
        && p.split('/').all(|seg| !seg.is_empty() && !seg.starts_with('.'))
        && p.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | '+'))
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// The search URL for GGUF models, most downloaded first.
pub fn search_url(query: &str) -> Result<DownloadUrl, EgressError> {
    let q: String = query.trim().chars().take(MAX_QUERY).collect();
    DownloadUrl::parse(&format!(
        "https://huggingface.co/api/models?search={}&filter=gguf&sort=downloads&direction=-1&limit={SEARCH_LIMIT}",
        encode(&q)
    ))
}

/// The recursive file list of a repository's main branch.
pub fn tree_url(repo: &RepoId) -> Result<DownloadUrl, EgressError> {
    DownloadUrl::parse(&format!(
        "https://huggingface.co/api/models/{}/tree/main?recursive=true",
        repo.as_str()
    ))
}

/// The model info (licence, gating).
pub fn info_url(repo: &RepoId) -> Result<DownloadUrl, EgressError> {
    DownloadUrl::parse(&format!("https://huggingface.co/api/models/{}", repo.as_str()))
}

/// A file's download URL.
pub fn file_url(repo: &RepoId, path: &str) -> Result<DownloadUrl, String> {
    if !valid_file_path(path) {
        return Err(format!("{path:?} is not a GGUF file of the repository"));
    }
    DownloadUrl::parse(&format!("https://huggingface.co/{}/resolve/main/{path}", repo.as_str())).map_err(|e| e.to_string())
}

/// A search result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HfModel {
    /// `owner/name`.
    pub repo: String,
    /// The owner.
    pub author: String,
    /// Downloads (last month).
    pub downloads: u64,
    /// Likes.
    pub likes: u64,
    /// Last change (Unix ms), when given.
    pub updated_at: Option<u64>,
    /// Tags (at most 12).
    pub tags: Vec<String>,
    /// `text-generation`, `feature-extraction`, …
    pub pipeline: Option<String>,
}

/// Parses an ISO date's day to Unix ms (`2025-03-01T…` → midnight UTC); `None` when it isn't one.
fn iso_day_ms(s: &str) -> Option<u64> {
    let d = s.get(..10)?;
    let mut it = d.split('-');
    let (y, m, day): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    if !(1970..=9999).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // Days from civil (Howard Hinnant).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days).ok().map(|d| d * 86_400_000)
}

/// Parses the search answer.
pub fn parse_search(v: &Value) -> Result<Vec<HfModel>, String> {
    let list = v.as_array().ok_or("Hugging Face's search answer is not a list")?;
    Ok(list
        .iter()
        .take(SEARCH_LIMIT)
        .filter_map(|m| {
            let repo = m.get("id").or_else(|| m.get("modelId")).and_then(Value::as_str)?;
            let repo = RepoId::parse(repo).ok()?;
            let author = repo.as_str().split('/').next().unwrap_or_default().to_string();
            Some(HfModel {
                author,
                downloads: m.get("downloads").and_then(Value::as_u64).unwrap_or(0),
                likes: m.get("likes").and_then(Value::as_u64).unwrap_or(0),
                updated_at: m.get("lastModified").and_then(Value::as_str).and_then(iso_day_ms),
                tags: m
                    .get("tags")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .take(12)
                    .map(str::to_string)
                    .collect(),
                pipeline: m.get("pipeline_tag").and_then(Value::as_str).map(str::to_string),
                repo: repo.0,
            })
        })
        .collect())
}

/// A downloadable GGUF (all parts of a split model together).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    /// The file (the first part of a split set).
    pub file: String,
    /// Every part, in order.
    pub parts: Vec<String>,
    /// e.g. `Q4_K_M`.
    pub quant: Option<String>,
    /// Bytes of all parts.
    pub size: u64,
    /// SHA-256 per part (from the LFS pointers), when every part has one.
    pub sha256: Vec<String>,
}

/// A repository's GGUF files.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoFiles {
    /// Model files.
    pub variants: Vec<Variant>,
    /// Vision projectors.
    pub projectors: Vec<Variant>,
}

fn sha_of(f: &Value) -> Option<String> {
    let oid = f.pointer("/lfs/oid").or_else(|| f.pointer("/lfs/sha256")).and_then(Value::as_str)?;
    let oid = oid.strip_prefix("sha256:").unwrap_or(oid);
    (oid.len() == 64 && oid.bytes().all(|b| b.is_ascii_hexdigit())).then(|| oid.to_ascii_lowercase())
}

/// Parses a tree listing into variants (sorted by size) and projectors.
pub fn parse_tree(v: &Value) -> Result<RepoFiles, String> {
    let list = v.as_array().ok_or("Hugging Face's file list is not a list")?;
    struct F {
        path: String,
        size: u64,
        sha: Option<String>,
    }
    let files: Vec<F> = list
        .iter()
        .take(MAX_FILES)
        .filter(|f| f.get("type").and_then(Value::as_str) == Some("file"))
        .filter_map(|f| {
            let path = f.get("path").and_then(Value::as_str).filter(|p| valid_file_path(p))?;
            let size = f.pointer("/lfs/size").or_else(|| f.get("size")).and_then(Value::as_u64)?;
            Some(F {
                path: path.to_string(),
                size,
                sha: sha_of(f),
            })
        })
        .collect();
    let mut out = RepoFiles::default();
    let mut groups: std::collections::BTreeMap<String, Vec<(u32, u32, &F)>> = Default::default();
    for f in &files {
        let stem = f.path.trim_end_matches(".gguf");
        match split_part(stem) {
            Some((base, part, total)) => groups.entry(format!("{base}#{total}")).or_default().push((part, total, f)),
            None => groups.entry(f.path.clone()).or_default().push((1, 1, f)),
        }
    }
    for (_, mut parts) in groups {
        parts.sort_by_key(|(p, _, _)| *p);
        let total = parts[0].1;
        let complete = parts.len() as u32 == total && parts.iter().enumerate().all(|(i, (p, _, _))| *p == i as u32 + 1);
        if !complete {
            continue;
        }
        let names: Vec<String> = parts.iter().map(|(_, _, f)| f.path.clone()).collect();
        let shas: Vec<String> = parts.iter().filter_map(|(_, _, f)| f.sha.clone()).collect();
        let v = Variant {
            file: names[0].clone(),
            quant: label_from_file_name(&names[0]),
            size: parts.iter().map(|(_, _, f)| f.size).sum(),
            sha256: if shas.len() == names.len() { shas } else { vec![] },
            parts: names,
        };
        let lower = v.file.to_ascii_lowercase();
        if lower.contains("mmproj") || lower.contains("projector") {
            out.projectors.push(v);
        } else {
            out.variants.push(v);
        }
    }
    out.variants.sort_by_key(|v| v.size);
    out.projectors.sort_by_key(|v| v.size);
    Ok(out)
}

/// Licence and gating from the model info.
pub fn parse_info(v: &Value) -> (Option<String>, bool) {
    let license = v.pointer("/cardData/license").and_then(Value::as_str).map(str::to_string);
    let gated = match v.get("gated") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty() && s != "false",
        _ => false,
    };
    (license, gated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repos_and_paths() {
        let r = RepoId::parse(" Qwen/Qwen3-8B-GGUF ").unwrap();
        assert_eq!(r.as_str(), "Qwen/Qwen3-8B-GGUF");
        for bad in ["noslash", "a/b/c", "../x", "a/..", "a/.x", "a b/c", "/x"] {
            assert!(RepoId::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(serde_json::from_str::<RepoId>("\"a/b\"").unwrap(), RepoId::parse("a/b").unwrap());
        assert!(serde_json::from_str::<RepoId>("\"ab\"").is_err());
        assert_eq!(String::from(r.clone()), "Qwen/Qwen3-8B-GGUF");
        assert!(valid_file_path("Q4/x-Q4_K_M.gguf"));
        for bad in ["x.bin", "../x.gguf", "/x.gguf", "a\\b.gguf", "a//b.gguf", ".x/y.gguf", "a b.gguf"] {
            assert!(!valid_file_path(bad), "{bad}");
        }
        assert!(file_url(&r, "../evil.gguf").is_err());
        assert_eq!(
            file_url(&r, "x.gguf").unwrap().as_str(),
            "https://huggingface.co/Qwen/Qwen3-8B-GGUF/resolve/main/x.gguf"
        );
        assert!(search_url("qwen 3 & co/é")
            .unwrap()
            .as_str()
            .contains("search=qwen+3+%26+co%2F%C3%A9&filter=gguf"));
        assert!(tree_url(&r).unwrap().as_str().ends_with("/tree/main?recursive=true"));
        assert!(info_url(&r).unwrap().as_str().ends_with("/api/models/Qwen/Qwen3-8B-GGUF"));
    }

    #[test]
    fn search_results() {
        let v = json!([
            {"id": "Qwen/Qwen3-8B-GGUF", "downloads": 100, "likes": 5, "lastModified": "2025-03-01T10:00:00.000Z", "tags": ["gguf", "chat"], "pipeline_tag": "text-generation"},
            {"modelId": "x/y"},
            {"id": "not a repo"},
            {"nothing": true}
        ]);
        let r = parse_search(&v).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].author, "Qwen");
        assert_eq!(r[0].updated_at, Some(1_740_787_200_000));
        assert_eq!(r[1].downloads, 0);
        assert!(parse_search(&json!({})).is_err());
        assert_eq!(iso_day_ms("1970-01-01"), Some(0));
        assert_eq!(iso_day_ms("2024-02-29"), Some(1_709_164_800_000));
        assert_eq!(iso_day_ms("nope"), None);
        assert_eq!(iso_day_ms("2024-13-01"), None);
        assert_eq!(iso_day_ms("1969-12-31"), None);
    }

    #[test]
    fn tree_listing_groups_and_filters() {
        let sha = "a".repeat(64);
        let v = json!([
            {"type": "file", "path": "Qwen3-8B-Q4_K_M.gguf", "size": 1, "lfs": {"oid": sha, "size": 5000}},
            {"type": "file", "path": "Qwen3-8B-Q8_0.gguf", "size": 9000},
            {"type": "file", "path": "big/Big-Q4_K_M-00002-of-00002.gguf", "lfs": {"oid": format!("sha256:{}", "b".repeat(64)), "size": 20}},
            {"type": "file", "path": "big/Big-Q4_K_M-00001-of-00002.gguf", "lfs": {"oid": "c".repeat(64), "size": 30}},
            {"type": "file", "path": "half/H-00001-of-00003.gguf", "size": 1},
            {"type": "file", "path": "mmproj-Qwen3-F16.gguf", "size": 700},
            {"type": "file", "path": "README.md", "size": 1},
            {"type": "directory", "path": "big"},
            {"type": "file", "path": "../evil.gguf", "size": 1},
            {"type": "file", "path": "nosize.gguf"}
        ]);
        let r = parse_tree(&v).unwrap();
        assert_eq!(r.variants.len(), 3, "{:?}", r.variants);
        assert_eq!(r.variants[0].file, "big/Big-Q4_K_M-00001-of-00002.gguf");
        assert_eq!((r.variants[0].size, r.variants[0].parts.len()), (50, 2));
        assert_eq!(r.variants[0].sha256, vec!["c".repeat(64), "b".repeat(64)]);
        assert_eq!(r.variants[1].quant.as_deref(), Some("Q4_K_M"));
        assert_eq!(r.variants[1].sha256, vec![sha]);
        assert!(r.variants[2].sha256.is_empty());
        assert_eq!(r.projectors.len(), 1);
        assert!(parse_tree(&json!("x")).is_err());
    }

    #[test]
    fn info() {
        assert_eq!(
            parse_info(&json!({"cardData": {"license": "apache-2.0"}, "gated": false})),
            (Some("apache-2.0".into()), false)
        );
        assert_eq!(parse_info(&json!({"gated": "manual"})), (None, true));
        assert_eq!(parse_info(&json!({"gated": "false"})), (None, false));
        assert!(parse_info(&json!({"gated": true})).1);
        assert!(!parse_info(&json!({})).1);
    }
}
