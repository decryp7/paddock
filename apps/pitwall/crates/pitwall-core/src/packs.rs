//! Model catalog packs (ADR 0011, narrowed by ADR 0020): TOML files that list curated models for the library's
//! "will this fit?" and the recommendations. Engine packs no longer exist: llama.cpp is the only engine.
//!
//! Packs are parsed once, here, into typed values. Unknown keys are warnings; an invalid pack is skipped with a
//! message that says what to fix; one bad pack never stops the others.

use crate::fit::{self, Fit, ModelSize, Room, Verdict};
use crate::select::Capability;
use serde::{Deserialize, Serialize};

/// The pack format this version reads.
pub const FORMAT: u32 = 1;
/// Largest pack file accepted.
pub const MAX_PACK_BYTES: usize = 256 * 1024;
const MAX_MODELS: usize = 500;
const MAX_VARIANTS: usize = 32;

/// A problem with a pack, attributed to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// The pack's file name.
    pub file: String,
    /// What to fix.
    pub message: String,
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
    /// Catalog models (later packs replace earlier ones with the same model id).
    pub models: Vec<CatalogModel>,
    /// Packs that were skipped.
    pub problems: Vec<Problem>,
    /// Unknown keys and other non-fatal notes.
    pub warnings: Vec<Problem>,
}

fn is_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && s.starts_with(|c: char| c.is_ascii_alphanumeric())
}

type Table = toml::map::Map<String, toml::Value>;

/// Known keys per table, for the unknown-key warnings.
fn warn_unknown(t: &Table, known: &[&str], path: &str, file: &str, out: &mut Vec<Problem>) {
    for k in t.keys().filter(|k| !known.contains(&k.as_str())) {
        out.push(Problem {
            file: file.into(),
            message: format!("unknown key \"{path}{k}\" is ignored"),
        });
    }
}

fn s(t: &Table, k: &str) -> Option<String> {
    t.get(k).and_then(toml::Value::as_str).map(str::to_string)
}

fn u(t: &Table, k: &str) -> Option<u64> {
    t.get(k).and_then(toml::Value::as_integer).and_then(|n| u64::try_from(n).ok())
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
    s(&t, "id")
        .filter(|i| is_slug(i))
        .ok_or("\"id\" must be 1 to 64 letters, digits, - _ .")?;
    let mut out: Vec<CatalogModel> = vec![];
    let list = t
        .get("model")
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if list.len() > MAX_MODELS {
        return Err(format!("at most {MAX_MODELS} models per pack"));
    }
    for (i, m) in list.iter().enumerate() {
        let n = i + 1;
        let m = m.as_table().ok_or_else(|| format!("model #{n} is not a table"))?;
        warn_unknown(
            m,
            &["id", "name", "repo", "context", "capabilities", "tags", "variants"],
            "model.",
            file,
            warnings,
        );
        let id = s(m, "id")
            .filter(|i| is_slug(i))
            .ok_or_else(|| format!("model #{n}: \"id\" must be 1 to 64 letters, digits, - _ ."))?;
        let repo = s(m, "repo").ok_or_else(|| format!("model {id}: \"repo\" is required"))?;
        crate::hf::RepoId::parse(&repo).map_err(|e| format!("model {id}: {e}"))?;
        let context = u(m, "context")
            .filter(|c| (256..=10_000_000).contains(c))
            .ok_or_else(|| format!("model {id}: \"context\" must be 256 to 10000000"))? as u32;
        let strings = |k: &str| -> Vec<String> {
            m.get(k)
                .and_then(toml::Value::as_array)
                .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
                .unwrap_or_default()
        };
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
        let vs = m
            .get("variants")
            .and_then(toml::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if vs.is_empty() || vs.len() > MAX_VARIANTS {
            return Err(format!("model {id}: \"variants\" needs 1 to {MAX_VARIANTS} entries"));
        }
        let mut variants = vec![];
        for v in vs {
            let v = v.as_table().ok_or_else(|| format!("model {id}: a variant is not a table"))?;
            warn_unknown(v, &["quant", "file", "size_bytes"], "model.variants.", file, warnings);
            let file_name = s(v, "file")
                .filter(|f| crate::hf::valid_file_path(f) && f.ends_with(".gguf"))
                .ok_or_else(|| format!("model {id}: a variant needs a \"file\" ending in .gguf"))?;
            variants.push(CatalogVariant {
                quant: s(v, "quant").unwrap_or_default(),
                file: file_name,
                size_bytes: u(v, "size_bytes")
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("model {id}: a variant needs \"size_bytes\""))?,
            });
        }
        variants.sort_by_key(|v| v.size_bytes);
        if out.iter().any(|x| x.id == id) {
            return Err(format!("model {id} is listed twice"));
        }
        out.push(CatalogModel {
            name: s(m, "name").unwrap_or_else(|| id.clone()),
            id,
            repo,
            context,
            capabilities,
            tags,
            variants,
        });
    }
    Ok(out)
}

impl Packs {
    /// Load packs from `(file name, text)` pairs; files named `*.models.toml` are catalog packs, anything else is
    /// reported. Later files win on a clashing id (user packs after built-ins).
    pub fn load<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> Packs {
        let mut p = Packs::default();
        for (name, text) in files {
            let mut w = vec![];
            if name.ends_with(".models.toml") {
                match parse_models(text, name, &mut w) {
                    Ok(ms) => {
                        for m in ms {
                            p.models.retain(|x| x.id != m.id);
                            p.models.push(m);
                        }
                    }
                    Err(m) => p.problems.push(Problem {
                        file: name.into(),
                        message: m,
                    }),
                }
            } else {
                p.problems.push(Problem {
                    file: name.into(),
                    message: "pack files are named *.models.toml".into(),
                });
            }
            p.warnings.extend(w);
        }
        p
    }

    /// The built-in packs.
    pub fn builtin() -> Packs {
        Packs::load([("starter.models.toml", include_str!("packs/starter.models.toml"))])
    }
}

/// Models matching `tag` (all when `None`) that fit `room`, best first: the largest variant that fits comfortably,
/// models that only fit tightly after, and nothing that will not fit.
pub fn recommend(models: &[CatalogModel], tag: Option<&str>, room: &Room) -> Vec<Recommendation> {
    let mut out: Vec<(u8, u64, Recommendation)> = vec![];
    for m in models.iter().filter(|m| tag.is_none_or(|t| m.tags.iter().any(|x| x == t))) {
        let ctx = m.context.min(8192);
        let assess = |v: &CatalogVariant| fit::assess(Some(&ModelSize::from_file(v.size_bytes, Some(&v.quant))), ctx, room);
        let fits = m
            .variants
            .iter()
            .filter(|v| assess(v).verdict == Verdict::Fits)
            .max_by_key(|v| v.size_bytes);
        let tight = m
            .variants
            .iter()
            .filter(|v| assess(v).verdict == Verdict::Tight)
            .max_by_key(|v| v.size_bytes);
        if let Some((v, rank)) = fits.map(|v| (v, 0u8)).or(tight.map(|v| (v, 1))) {
            out.push((
                rank,
                u64::MAX - v.size_bytes,
                Recommendation {
                    model: m.clone(),
                    variant: v.clone(),
                    fit: assess(v),
                },
            ));
        }
    }
    out.sort_by_key(|(r, s, _)| (*r, *s));
    out.into_iter().map(|(_, _, r)| r).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GIB;

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
        assert!(e(&MODELS.replace(
            "context = 4096\ncapabilities = [\"chat\"]",
            "context = 1\ncapabilities = [\"chat\"]"
        ))
        .contains("context"));
        assert!(e(&format!("{MODELS}[[model]]\nid = \"tiny\"\nrepo = \"o/x\"\ncontext = 4096\ncapabilities = [\"chat\"]\nvariants = [{{ file = \"a.gguf\", size_bytes = 1 }}]\n")).contains("twice"));
    }

    #[test]
    fn recommendations_respect_the_room_and_tags() {
        let ms = parse_models(MODELS, "m", &mut vec![]).unwrap();
        let room = Room {
            ram_total: 16 * GIB,
            ram_available: 12 * GIB,
            ram_releasable: 0,
            vram_free: None,
            unified: false,
        };
        let r = recommend(&ms, None, &room);
        assert_eq!(r.len(), 1, "the 90 GB model does not fit");
        assert_eq!(
            (r[0].model.id.as_str(), r[0].variant.quant.as_str()),
            ("tiny", "Q8_0"),
            "the largest variant that fits"
        );
        assert!(recommend(&ms, Some("code"), &room).is_empty());
        assert_eq!(recommend(&ms, Some("small"), &room).len(), 1);
        let none = Room {
            ram_total: GIB,
            ram_available: GIB / 4,
            ..room
        };
        assert!(recommend(&ms, None, &none).is_empty());
    }

    #[test]
    fn loading_isolates_bad_packs_and_later_packs_win() {
        let p = Packs::load([
            ("bad.models.toml", "nope ["),
            ("readme.txt", "x"),
            ("old.engine.toml", "format = 1"),
            ("m.models.toml", MODELS),
            ("n.models.toml", &MODELS.replace("o/tiny", "o/tiny2")),
        ]);
        assert_eq!(p.problems.len(), 3);
        assert_eq!(p.models.iter().find(|m| m.id == "tiny").map(|m| m.repo.as_str()), Some("o/tiny2"));
    }

    #[test]
    fn built_in_packs_are_valid() {
        let p = Packs::builtin();
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        assert!(p.warnings.is_empty(), "{:?}", p.warnings);
        assert!(p.models.iter().any(|m| m.tags.iter().any(|t| t == "code")));
    }

    #[test]
    fn the_starter_catalog_has_only_current_generation_models_with_real_looking_files() {
        let p = Packs::builtin();
        let ids: Vec<&str> = p.models.iter().map(|m| m.id.as_str()).collect();
        for want in ["qwen3.8-27b", "qwen3.6-35b-a3b", "gemma-4-26b-a4b", "qwen3-coder-next", "qwen3-embedding-0.6b"] {
            assert!(ids.contains(&want), "{want} in {ids:?}");
        }
        // Nothing from the generations before these (Qwen2.x, Qwen3 base sizes, Nomic, Llama 3, Gemma 3).
        for old in ["qwen2", "nomic", "llama-3", "gemma-3", "qwen3-8b", "qwen3-1.7b"] {
            assert!(ids.iter().all(|i| !i.contains(old)), "{old} must not be in the catalog: {ids:?}");
        }
        for m in &p.models {
            assert!(m.repo.contains('/'), "{}", m.id);
            assert!(!m.variants.is_empty(), "{}", m.id);
            for v in &m.variants {
                assert!(v.file.ends_with(".gguf") && !v.file.contains("-of-0"), "{}: split or odd file {}", m.id, v.file);
                assert!(v.size_bytes > 100_000_000, "{}: {}", m.id, v.file);
                assert!(crate::quant::Quant::parse(&v.quant).is_some(), "{}: the fit check must understand {}", m.id, v.quant);
            }
        }
        // A tier for low memory exists, and embeddings are there.
        assert!(p.models.iter().any(|m| m.tags.iter().any(|t| t == "tiny")));
        assert!(p.models.iter().any(|m| m.capabilities == [Capability::Embeddings]));
    }

    #[test]
    fn structural_mistakes_are_named() {
        let ok = |t: &str| parse_models(t, "m", &mut vec![]).unwrap_err();
        assert!(ok("format = 2").contains("format"));
        assert!(ok("not toml [").contains("TOML"));
        assert!(ok(&"x".repeat(MAX_PACK_BYTES + 1)).contains("large"));
        assert!(ok("format = 1\nid = \"-\"\n").contains("id"));
        assert!(ok(&MODELS.replace("tags = [\"Small\"]", "tags = [\"has space\"]")).contains("tags"));
        assert!(ok(&MODELS.replace("capabilities = [\"chat\"]\ntags", "capabilities = []\ntags")).contains("capabilities"));
        assert!(ok(&MODELS.replace("variants = [{ quant = \"Q8_0\", file = \"tiny-q8.gguf\", size_bytes = 700000000 }, { quant = \"Q4_K_M\", file = \"tiny-q4.gguf\", size_bytes = 400000000 }]", "variants = []")).contains("variants"));
        assert!(ok(&MODELS.replace("size_bytes = 700000000", "size_bytes = 0")).contains("size_bytes"));
        assert!(ok(&MODELS.replace("model]]\nid = \"tiny\"", "model]]\nid = \"-\"")).contains("id"));
        assert!(ok(&MODELS.replace("repo = \"o/huge\"\n", "")).contains("repo"));
        assert!(ok("format = 1\nid = \"x\"\nmodel = [1]\n").contains("not a table"));
        let p = Packs::load([("x.txt", "")]);
        assert!(p.problems[0].message.contains("named"));
    }

    #[test]
    fn tight_fits_are_recommended_after_comfortable_ones() {
        let ms = parse_models(MODELS, "m", &mut vec![]).unwrap();
        // Room so small that the 400 MB variant is only a tight fit, and the 700 MB one does not.
        let room = Room {
            ram_total: 2 * GIB,
            ram_available: 1100 * crate::MIB,
            ram_releasable: 0,
            vram_free: None,
            unified: false,
        };
        for r in recommend(&ms, None, &room) {
            assert!(matches!(r.fit.verdict, Verdict::Fits | Verdict::Tight));
        }
    }
}
