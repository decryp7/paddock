//! Declarative packs (ADR 0011): TOML files that extend Pitwall without code.
//!
//! - **Engine packs** describe any local engine that serves an OpenAI-compatible API (llamafile, KoboldCpp,
//!   `mlx_lm.server`, vLLM, ...): how to launch it, its health endpoint, capabilities, the pinned and checksummed
//!   download per platform and memory hints.
//! - **Model catalog packs** list curated models (Hugging Face repo, quantisation variants, context, capabilities,
//!   tags) for the library's "will this fit?" and recommendations.
//!
//! Packs are parsed once, here, into typed values. Unknown keys are warnings; an invalid pack is skipped with a
//! message that says what to fix; one bad pack never stops the others. Downloads can only name hosts on the
//! privacy allow-list, so a pack cannot add an outbound destination.

use crate::fit::{self, Fit, ModelSize, Room, Verdict};
use crate::privacy::DownloadUrl;
use crate::select::Capability;
use serde::{Deserialize, Serialize};

/// The pack format this version reads.
pub const FORMAT: u32 = 1;
/// Largest pack file accepted.
pub const MAX_PACK_BYTES: usize = 256 * 1024;
const MAX_ARGS: usize = 64;
const MAX_MODELS: usize = 500;
const MAX_VARIANTS: usize = 32;

/// The placeholders an argument template may use.
pub const PLACEHOLDERS: &[&str] = &["host", "port", "model", "mmproj", "context", "alias", "threads", "engine_dir", "binary"];

/// A problem with a pack, attributed to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// The pack's file name.
    pub file: String,
    /// What to fix.
    pub message: String,
}

/// How a download is unpacked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Archive {
    /// The download is the executable itself.
    None,
    /// A zip archive.
    Zip,
    /// A gzip-compressed tar archive.
    TarGz,
}

/// One pinned download for a platform.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// `windows-x64`, `macos-arm64`, `linux-x64`, ...
    pub platform: String,
    /// The checked URL.
    pub url: DownloadUrl,
    /// SHA-256, lowercase hex.
    pub sha256: String,
    /// Size in bytes.
    pub size: u64,
    /// How to unpack it.
    pub archive: Archive,
    /// The executable's file name inside the install folder.
    pub binary: String,
}

/// What an engine can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineCaps {
    /// Chat completions.
    pub chat: bool,
    /// Embeddings.
    pub embeddings: bool,
    /// Image input.
    pub vision: bool,
    /// Tool calls.
    pub tools: bool,
}

/// A validated engine pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnginePack {
    /// Its id (`llamafile`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// One sentence.
    pub description: String,
    /// The pinned engine version.
    pub version: String,
    /// What it can do.
    pub caps: EngineCaps,
    /// Command template (placeholders in braces).
    pub command: String,
    /// Argument templates.
    pub args: Vec<String>,
    /// Environment variable that receives a private API key, when the engine supports one.
    pub api_key_env: Option<String>,
    /// Health check path.
    pub health_path: String,
    /// Extra memory the engine needs on top of the model.
    pub overhead_bytes: u64,
    /// Seconds to wait for the engine to become healthy.
    pub start_timeout_secs: u64,
    /// Downloads per platform.
    pub downloads: Vec<Download>,
}

/// One quantisation of a catalog model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogVariant {
    /// `Q4_K_M`.
    pub quant: String,
    /// File name in the repo.
    pub file: String,
    /// Size in bytes.
    pub size_bytes: u64,
}

/// A curated model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogModel {
    /// Id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// `owner/repo` on Hugging Face.
    pub repo: String,
    /// Maximum context.
    pub context: u32,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// Tags (`code`, `small`, ...).
    pub tags: Vec<String>,
    /// Quantisations, smallest first.
    pub variants: Vec<CatalogVariant>,
}

/// A recommendation: a model and the variant that fits best.
#[derive(Debug, Clone, PartialEq)]
pub struct Recommendation {
    /// The model.
    pub model: CatalogModel,
    /// The largest variant that fits comfortably (or the smallest, with its verdict, when none does).
    pub variant: CatalogVariant,
    /// Whether it fits.
    pub fit: Fit,
}

/// Everything loaded from a set of pack files.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Packs {
    /// Engine packs.
    pub engines: Vec<EnginePack>,
    /// Catalog models (later packs replace earlier ones with the same model id).
    pub models: Vec<CatalogModel>,
    /// Packs that were skipped.
    pub problems: Vec<Problem>,
    /// Unknown keys and other non-fatal notes.
    pub warnings: Vec<Problem>,
}

fn is_slug(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')) && s.starts_with(|c: char| c.is_ascii_alphanumeric())
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_platform(s: &str) -> bool {
    matches!(s, "windows-x64" | "windows-arm64" | "macos-arm64" | "macos-x64" | "linux-x64" | "linux-arm64")
}

fn is_file_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 200 && !s.starts_with('.') && !s.contains("..") && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+'))
}

/// The placeholders used in a template, or the first malformed one.
pub fn placeholders(template: &str) -> Result<Vec<&str>, String> {
    let mut out = vec![];
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        if rest.as_bytes()[i] == b'}' {
            return Err(format!("a stray \"}}\" in \"{template}\""));
        }
        let after = &rest[i + 1..];
        let j = after.find('}').ok_or_else(|| format!("an unclosed \"{{\" in \"{template}\""))?;
        out.push(&after[..j]);
        rest = &after[j + 1..];
    }
    Ok(out)
}

/// Fill a template; every placeholder must be in `values`.
pub fn fill(template: &str, values: &[(&str, String)]) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let j = after.find('}').ok_or("unclosed placeholder")?;
        let name = &after[..j];
        let v = values.iter().find(|(k, _)| *k == name).ok_or_else(|| format!("no value for {{{name}}}"))?;
        out.push_str(&v.1);
        rest = &after[j + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

type Table = toml::map::Map<String, toml::Value>;

/// Known keys per table, for the unknown-key warnings.
fn warn_unknown(t: &Table, known: &[&str], path: &str, file: &str, out: &mut Vec<Problem>) {
    for k in t.keys().filter(|k| !known.contains(&k.as_str())) {
        out.push(Problem { file: file.into(), message: format!("unknown key \"{path}{k}\" is ignored") });
    }
}

fn s(t: &Table, k: &str) -> Option<String> {
    t.get(k).and_then(toml::Value::as_str).map(str::to_string)
}

fn u(t: &Table, k: &str) -> Option<u64> {
    t.get(k).and_then(toml::Value::as_integer).and_then(|n| u64::try_from(n).ok())
}

fn b(t: &Table, k: &str) -> bool {
    t.get(k).and_then(toml::Value::as_bool).unwrap_or(false)
}

fn sub<'a>(t: &'a Table, k: &str) -> Option<&'a Table> {
    t.get(k).and_then(toml::Value::as_table)
}

/// Parse an engine pack.
pub fn parse_engine(text: &str, file: &str, warnings: &mut Vec<Problem>) -> Result<EnginePack, String> {
    if text.len() > MAX_PACK_BYTES {
        return Err("the pack file is too large".into());
    }
    let t: Table = toml::from_str(text).map_err(|e| format!("not valid TOML: {}", e.message()))?;
    if u(&t, "format") != Some(u64::from(FORMAT)) {
        return Err(format!("\"format\" must be {FORMAT}"));
    }
    warn_unknown(&t, &["format", "id", "name", "description", "version", "capabilities", "launch", "health", "memory", "download"], "", file, warnings);
    let id = s(&t, "id").filter(|i| is_slug(i)).ok_or("\"id\" must be 1 to 64 letters, digits, - _ . ")?;
    if id == "llama.cpp" || id == "llamacpp" {
        return Err("\"id\" clashes with the built-in llama.cpp engine".into());
    }
    let name = s(&t, "name").filter(|n| !n.trim().is_empty() && n.len() <= 80).ok_or("\"name\" is required (at most 80 characters)")?;
    let version = s(&t, "version").filter(|v| is_file_name(v)).ok_or("\"version\" is required: pin the engine version")?;
    let c = sub(&t, "capabilities").ok_or("[capabilities] is required")?;
    warn_unknown(c, &["chat", "embeddings", "vision", "tools"], "capabilities.", file, warnings);
    let caps = EngineCaps { chat: b(c, "chat"), embeddings: b(c, "embeddings"), vision: b(c, "vision"), tools: b(c, "tools") };
    if !caps.chat && !caps.embeddings {
        return Err("[capabilities] must enable chat or embeddings".into());
    }
    let l = sub(&t, "launch").ok_or("[launch] is required")?;
    warn_unknown(l, &["command", "args", "api_key_env", "start_timeout_secs"], "launch.", file, warnings);
    let command = s(l, "command").filter(|c| !c.is_empty() && c.len() <= 300).ok_or("launch.command is required")?;
    let args: Vec<String> = l.get("args").and_then(toml::Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();
    if args.len() > MAX_ARGS || args.iter().any(|a| a.len() > 300 || a.chars().any(char::is_control)) {
        return Err(format!("launch.args must have at most {MAX_ARGS} entries without control characters"));
    }
    let mut used: Vec<String> = vec![];
    for tpl in std::iter::once(&command).chain(args.iter()) {
        for p in placeholders(tpl)? {
            if !PLACEHOLDERS.contains(&p) {
                return Err(format!("unknown placeholder {{{p}}}; use one of {}", PLACEHOLDERS.iter().map(|p| format!("{{{p}}}")).collect::<Vec<_>>().join(", ")));
            }
            used.push(p.to_string());
        }
    }
    for need in ["host", "port", "model"] {
        if !used.iter().any(|p| p == need) {
            return Err(format!("launch.args must use {{{need}}}: the engine has to bind 127.0.0.1 on the port Pitwall picks and load the model it is given"));
        }
    }
    if args.iter().any(|a| a == "0.0.0.0" || a == "::" || a.starts_with("--host=0")) {
        return Err("launch.args must not bind all interfaces; use {host}".into());
    }
    let api_key_env = s(l, "api_key_env");
    if api_key_env.as_deref().is_some_and(|e| e.is_empty() || !e.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')) {
        return Err("launch.api_key_env must be an UPPER_CASE variable name".into());
    }
    let start_timeout_secs = u(l, "start_timeout_secs").unwrap_or(300).clamp(5, 1800);
    let h = sub(&t, "health").ok_or("[health] is required")?;
    warn_unknown(h, &["path"], "health.", file, warnings);
    let health_path = s(h, "path").filter(|p| p.starts_with('/') && !p.contains(' ') && p.len() <= 100).ok_or("health.path must start with /")?;
    let overhead_bytes = sub(&t, "memory").and_then(|m| u(m, "overhead_mib")).unwrap_or(512).min(1 << 20) << 20;
    if let Some(m) = sub(&t, "memory") {
        warn_unknown(m, &["overhead_mib"], "memory.", file, warnings);
    }
    let mut downloads = vec![];
    for (i, d) in t.get("download").and_then(toml::Value::as_array).map(Vec::as_slice).unwrap_or_default().iter().enumerate() {
        let d = d.as_table().ok_or_else(|| format!("download #{} is not a table", i + 1))?;
        warn_unknown(d, &["platform", "url", "sha256", "size", "archive", "binary"], "download.", file, warnings);
        let platform = s(d, "platform").filter(|p| is_platform(p)).ok_or_else(|| format!("download #{}: \"platform\" must be windows-x64, windows-arm64, macos-arm64, macos-x64, linux-x64 or linux-arm64", i + 1))?;
        let url = s(d, "url").ok_or_else(|| format!("download #{}: \"url\" is required", i + 1)).and_then(|u| DownloadUrl::parse(&u).map_err(|e| format!("download #{}: {e}", i + 1)))?;
        let sha256 = s(d, "sha256").map(|h| h.to_ascii_lowercase()).filter(|h| is_sha256(h)).ok_or_else(|| format!("download #{}: \"sha256\" must be 64 hex characters: downloads are always verified", i + 1))?;
        let size = u(d, "size").filter(|n| *n > 0).ok_or_else(|| format!("download #{}: \"size\" is required", i + 1))?;
        let archive: Archive = d.get("archive").cloned().unwrap_or_else(|| toml::Value::String("none".into())).try_into().map_err(|_| format!("download #{}: \"archive\" must be none, zip or tar-gz", i + 1))?;
        let binary = s(d, "binary").filter(|b| is_file_name(b)).ok_or_else(|| format!("download #{}: \"binary\" must be a plain file name", i + 1))?;
        if downloads.iter().any(|x: &Download| x.platform == platform) {
            return Err(format!("download #{}: platform {platform} is listed twice", i + 1));
        }
        downloads.push(Download { platform, url, sha256, size, archive, binary });
    }
    Ok(EnginePack { id, name, description: s(&t, "description").unwrap_or_default(), version, caps, command, args, api_key_env, health_path, overhead_bytes, start_timeout_secs, downloads })
}

/// Parse a model catalog pack.
pub fn parse_models(text: &str, file: &str, warnings: &mut Vec<Problem>) -> Result<Vec<CatalogModel>, String> {
    if text.len() > MAX_PACK_BYTES {
        return Err("the pack file is too large".into());
    }
    let t: Table = toml::from_str(text).map_err(|e| format!("not valid TOML: {}", e.message()))?;
    if u(&t, "format") != Some(u64::from(FORMAT)) {
        return Err(format!("\"format\" must be {FORMAT}"));
    }
    warn_unknown(&t, &["format", "id", "name", "model"], "", file, warnings);
    s(&t, "id").filter(|i| is_slug(i)).ok_or("\"id\" must be 1 to 64 letters, digits, - _ .")?;
    let mut out: Vec<CatalogModel> = vec![];
    let list = t.get("model").and_then(toml::Value::as_array).map(Vec::as_slice).unwrap_or_default();
    if list.len() > MAX_MODELS {
        return Err(format!("at most {MAX_MODELS} models per pack"));
    }
    for (i, m) in list.iter().enumerate() {
        let n = i + 1;
        let m = m.as_table().ok_or_else(|| format!("model #{n} is not a table"))?;
        warn_unknown(m, &["id", "name", "repo", "context", "capabilities", "tags", "variants"], "model.", file, warnings);
        let id = s(m, "id").filter(|i| is_slug(i)).ok_or_else(|| format!("model #{n}: \"id\" must be 1 to 64 letters, digits, - _ ."))?;
        let repo = s(m, "repo").ok_or_else(|| format!("model {id}: \"repo\" is required"))?;
        crate::hf::RepoId::parse(&repo).map_err(|e| format!("model {id}: {e}"))?;
        let context = u(m, "context").filter(|c| (256..=10_000_000).contains(c)).ok_or_else(|| format!("model {id}: \"context\" must be 256 to 10000000"))? as u32;
        let strings = |k: &str| -> Vec<String> { m.get(k).and_then(toml::Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default() };
        let mut capabilities = vec![];
        for c in strings("capabilities") {
            capabilities.push(Capability::parse(&c).ok_or_else(|| format!("model {id}: unknown capability \"{c}\""))?);
        }
        if capabilities.is_empty() {
            return Err(format!("model {id}: \"capabilities\" must not be empty"));
        }
        let tags: Vec<String> = strings("tags").into_iter().map(|t| t.to_ascii_lowercase()).collect();
        if tags.iter().any(|t| !is_slug(t)) {
            return Err(format!("model {id}: tags are lowercase words"));
        }
        let vs = m.get("variants").and_then(toml::Value::as_array).map(Vec::as_slice).unwrap_or_default();
        if vs.is_empty() || vs.len() > MAX_VARIANTS {
            return Err(format!("model {id}: \"variants\" needs 1 to {MAX_VARIANTS} entries"));
        }
        let mut variants = vec![];
        for v in vs {
            let v = v.as_table().ok_or_else(|| format!("model {id}: a variant is not a table"))?;
            warn_unknown(v, &["quant", "file", "size_bytes"], "model.variants.", file, warnings);
            let file_name = s(v, "file").filter(|f| crate::hf::valid_file_path(f) && f.ends_with(".gguf")).ok_or_else(|| format!("model {id}: a variant needs a \"file\" ending in .gguf"))?;
            variants.push(CatalogVariant {
                quant: s(v, "quant").unwrap_or_default(),
                file: file_name,
                size_bytes: u(v, "size_bytes").filter(|n| *n > 0).ok_or_else(|| format!("model {id}: a variant needs \"size_bytes\""))?,
            });
        }
        variants.sort_by_key(|v| v.size_bytes);
        if out.iter().any(|x| x.id == id) {
            return Err(format!("model {id} is listed twice"));
        }
        out.push(CatalogModel { name: s(m, "name").unwrap_or_else(|| id.clone()), id, repo, context, capabilities, tags, variants });
    }
    Ok(out)
}

impl Packs {
    /// Load packs from `(file name, text)` pairs; files named `*.engine.toml` are engine packs, `*.models.toml`
    /// catalog packs, anything else is reported. Later files win on a clashing id (user packs after built-ins).
    pub fn load<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Packs {
        let mut p = Packs::default();
        for (name, text) in files {
            let mut w = vec![];
            if name.ends_with(".engine.toml") {
                match parse_engine(text, name, &mut w) {
                    Ok(e) => {
                        p.engines.retain(|x| x.id != e.id);
                        p.engines.push(e);
                    }
                    Err(m) => p.problems.push(Problem { file: name.into(), message: m }),
                }
            } else if name.ends_with(".models.toml") {
                match parse_models(text, name, &mut w) {
                    Ok(ms) => {
                        for m in ms {
                            p.models.retain(|x| x.id != m.id);
                            p.models.push(m);
                        }
                    }
                    Err(m) => p.problems.push(Problem { file: name.into(), message: m }),
                }
            } else {
                p.problems.push(Problem { file: name.into(), message: "pack files are named *.engine.toml or *.models.toml".into() });
            }
            p.warnings.extend(w);
        }
        p
    }

    /// The built-in packs.
    pub fn builtin() -> Packs {
        Packs::load([("llamafile.engine.toml", include_str!("packs/llamafile.engine.toml")), ("starter.models.toml", include_str!("packs/starter.models.toml"))])
    }
}

/// Models matching `tag` (all when `None`) that fit `room`, best first: the largest variant that fits comfortably,
/// models that only fit tightly after, and nothing that will not fit.
pub fn recommend(models: &[CatalogModel], tag: Option<&str>, room: &Room) -> Vec<Recommendation> {
    let mut out: Vec<(u8, u64, Recommendation)> = vec![];
    for m in models.iter().filter(|m| tag.is_none_or(|t| m.tags.iter().any(|x| x == t))) {
        let ctx = m.context.min(8192);
        let assess = |v: &CatalogVariant| fit::assess(Some(&ModelSize::from_file(v.size_bytes, Some(&v.quant))), ctx, room);
        let fits = m.variants.iter().filter(|v| assess(v).verdict == Verdict::Fits).max_by_key(|v| v.size_bytes);
        let tight = m.variants.iter().filter(|v| assess(v).verdict == Verdict::Tight).max_by_key(|v| v.size_bytes);
        if let Some((v, rank)) = fits.map(|v| (v, 0u8)).or(tight.map(|v| (v, 1))) {
            out.push((rank, u64::MAX - v.size_bytes, Recommendation { model: m.clone(), variant: v.clone(), fit: assess(v) }));
        }
    }
    out.sort_by_key(|(r, s, _)| (*r, *s));
    out.into_iter().map(|(_, _, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GIB;

    fn engine(extra: &str) -> String {
        format!(
            r#"format = 1
id = "kobold"
name = "KoboldCpp"
version = "1.2.3"
[capabilities]
chat = true
[launch]
command = "{{engine_dir}}/{{binary}}"
args = ["--host", "{{host}}", "--port", "{{port}}", "-m", "{{model}}"]
[health]
path = "/health"
{extra}"#
        )
    }

    const DL: &str = r#"[[download]]
platform = "linux-x64"
url = "https://github.com/x/y/releases/download/1/k.tar.gz"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
size = 10
archive = "tar-gz"
binary = "kobold"
"#;

    fn err(text: &str) -> String {
        parse_engine(text, "t.engine.toml", &mut vec![]).unwrap_err()
    }

    #[test]
    fn a_good_engine_pack_parses() {
        let mut w = vec![];
        let e = parse_engine(&engine(&format!("{DL}\n[memory]\noverhead_mib = 100\n")), "k.engine.toml", &mut w).unwrap();
        assert_eq!((e.id.as_str(), e.version.as_str(), e.overhead_bytes, e.downloads[0].archive), ("kobold", "1.2.3", 100 << 20, Archive::TarGz));
        assert_eq!(e.downloads[0].url.host(), "github.com");
        assert!(w.is_empty());
    }

    #[test]
    fn unknown_keys_are_warnings() {
        let mut w = vec![];
        parse_engine(&engine("mystery = 1\n"), "k.engine.toml", &mut w).unwrap();
        assert!(w[0].message.contains("mystery"));
    }

    #[test]
    fn invalid_engine_packs_say_what_to_fix() {
        assert!(err("not toml [").contains("not valid TOML"));
        assert!(err(&engine("").replace("format = 1", "format = 2")).contains("format"));
        assert!(err(&engine("").replace("id = \"kobold\"", "id = \"llama.cpp\"")).contains("clashes"));
        assert!(err(&engine("").replace("version = \"1.2.3\"\n", "")).contains("pin"));
        assert!(err(&engine("").replace("chat = true", "chat = false")).contains("chat or embeddings"));
        assert!(err(&engine("").replace("\"{host}\"", "\"0.0.0.0\"")).contains("{host}"));
        assert!(err(&engine("").replace("{port}", "{nope}")).contains("unknown placeholder"));
        assert!(err(&engine("").replace("\"-m\", \"{model}\"", "\"-m\", \"{model\"")).contains("unclosed"));
        assert!(err(&engine("").replace("path = \"/health\"", "path = \"health\"")).contains("health.path"));
        let bad_url = DL.replace("https://github.com/x/y", "https://evil.example/x");
        assert!(err(&engine(&bad_url)).contains("only downloads from"));
        let no_sha = DL.replace("0000000000000000000000000000000000000000000000000000000000000000", "abc");
        assert!(err(&engine(&no_sha)).contains("sha256"));
        assert!(err(&engine(&DL.replace("linux-x64", "plan9"))).contains("platform"));
        assert!(err(&engine(&DL.replace("tar-gz", "rar"))).contains("archive"));
        assert!(err(&engine(&format!("{DL}{DL}"))).contains("twice"));
        assert!(err(&engine(&DL.replace("\"kobold\"", "\"../x\""))).contains("binary"));
        assert!(err(&engine("").replace("[launch]", "[launch]\napi_key_env = \"lower\"")).contains("UPPER_CASE"));
        assert!(err(&"x".repeat(MAX_PACK_BYTES + 1)).contains("too large"));
    }

    #[test]
    fn templates_are_filled_or_refused() {
        assert_eq!(placeholders("{a} x {b}").unwrap(), ["a", "b"]);
        assert!(placeholders("a}").is_err());
        let v = [("port", "1234".to_string())];
        assert_eq!(fill("--port={port}", &v).unwrap(), "--port=1234");
        assert!(fill("{model}", &v).is_err());
        assert!(fill("{port", &v).is_err());
    }

    const MODELS: &str = r#"format = 1
id = "mine"
[[model]]
id = "tiny"
repo = "o/tiny"
context = 4096
capabilities = ["chat"]
tags = ["Small"]
variants = [{ quant = "Q8_0", file = "tiny-q8.gguf", size_bytes = 700000000 }, { quant = "Q4_K_M", file = "tiny-q4.gguf", size_bytes = 400000000 }]
[[model]]
id = "huge"
repo = "o/huge"
context = 4096
capabilities = ["chat", "tools"]
variants = [{ quant = "Q4_K_M", file = "huge-q4.gguf", size_bytes = 90000000000 }]
"#;

    #[test]
    fn model_packs_parse_sort_and_validate() {
        let ms = parse_models(MODELS, "m.models.toml", &mut vec![]).unwrap();
        assert_eq!(ms.len(), 2);
        assert_eq!(ms[0].variants[0].quant, "Q4_K_M", "smallest first");
        assert_eq!(ms[0].tags, ["small"]);
        let e = |t: &str| parse_models(t, "m", &mut vec![]).unwrap_err();
        assert!(e(&MODELS.replace("o/tiny", "nonsense")).contains("repo") || e(&MODELS.replace("o/tiny", "nonsense")).contains("tiny"));
        assert!(e(&MODELS.replace("\"chat\"]\ntags", "\"telepathy\"]\ntags")).contains("capability"));
        assert!(e(&MODELS.replace("tiny-q8.gguf", "tiny-q8.bin")).contains(".gguf"));
        assert!(e(&MODELS.replace("context = 4096\ncapabilities = [\"chat\"]", "context = 1\ncapabilities = [\"chat\"]")).contains("context"));
        assert!(e(&format!("{MODELS}[[model]]\nid = \"tiny\"\nrepo = \"o/x\"\ncontext = 4096\ncapabilities = [\"chat\"]\nvariants = [{{ file = \"a.gguf\", size_bytes = 1 }}]\n")).contains("twice"));
    }

    #[test]
    fn recommendations_respect_the_room_and_tags() {
        let ms = parse_models(MODELS, "m", &mut vec![]).unwrap();
        let room = Room { ram_total: 16 * GIB, ram_available: 12 * GIB, ram_releasable: 0, vram_free: None, unified: false };
        let r = recommend(&ms, None, &room);
        assert_eq!(r.len(), 1, "the 90 GB model does not fit");
        assert_eq!((r[0].model.id.as_str(), r[0].variant.quant.as_str()), ("tiny", "Q8_0"), "the largest variant that fits");
        assert!(recommend(&ms, Some("code"), &room).is_empty());
        assert_eq!(recommend(&ms, Some("small"), &room).len(), 1);
        let none = Room { ram_total: GIB, ram_available: GIB / 4, ..room };
        assert!(recommend(&ms, None, &none).is_empty());
    }

    #[test]
    fn loading_isolates_bad_packs_and_later_packs_win() {
        let p = Packs::load([
            ("a.engine.toml", engine("").as_str()),
            ("bad.engine.toml", "nope ["),
            ("readme.txt", "x"),
            ("m.models.toml", MODELS),
            ("n.models.toml", &MODELS.replace("o/tiny", "o/tiny2")),
        ]);
        assert_eq!(p.engines.len(), 1);
        assert_eq!(p.problems.len(), 2);
        assert_eq!(p.models.iter().find(|m| m.id == "tiny").map(|m| m.repo.as_str()), Some("o/tiny2"));
    }

    #[test]
    fn built_in_packs_are_valid() {
        let p = Packs::builtin();
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert_eq!(p.engines[0].id, "llamafile");
        assert!(p.engines[0].downloads.len() >= 5);
        assert!(p.models.iter().any(|m| m.tags.iter().any(|t| t == "code")));
    }
}
