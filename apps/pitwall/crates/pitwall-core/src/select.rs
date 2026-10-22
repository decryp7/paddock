//! Capability-based model selection (ADR 0003, `/x/v1/select` and `model: "auto:…"`): an app asks for what it needs
//! ("vision, at least 8k context, fast") and Pitwall picks the model, reporting the choice in the response
//! (`X-Pitwall-Model`, `X-Pitwall-Chosen-By`). Benchmarks on this machine feed the "fast" choice.

use crate::fit::Verdict;
use crate::gguf::ModelKind;
use crate::ids::ModelId;
use serde::{Deserialize, Serialize};

/// What a model can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Capability {
    /// Chat and completion.
    Chat,
    /// Reads images.
    Vision,
    /// Calls tools.
    Tools,
    /// Grammar-constrained JSON output.
    Json,
    /// Embeddings.
    Embeddings,
    /// Reranking.
    Rerank,
    /// Speech to text.
    Transcription,
}

impl Capability {
    /// Parses a capability name.
    pub fn parse(s: &str) -> Option<Capability> {
        match s.trim().to_ascii_lowercase().as_str() {
            "chat" | "completion" | "text" => Some(Capability::Chat),
            "vision" | "image" | "images" => Some(Capability::Vision),
            "tools" | "tool" | "functions" => Some(Capability::Tools),
            "json" | "structured" => Some(Capability::Json),
            "embeddings" | "embedding" | "embed" => Some(Capability::Embeddings),
            "rerank" | "reranker" => Some(Capability::Rerank),
            "transcription" | "speech" | "audio" => Some(Capability::Transcription),
            _ => None,
        }
    }
}

/// What a model can do, from what its file says.
pub fn capabilities_of(kind: ModelKind, has_projector: bool, chat_template: Option<&str>, name: &str) -> Vec<Capability> {
    match kind {
        ModelKind::Embedding if name.to_ascii_lowercase().contains("rerank") => vec![Capability::Rerank],
        ModelKind::Embedding => vec![Capability::Embeddings],
        ModelKind::Language => {
            let mut c = vec![Capability::Chat, Capability::Json];
            if chat_template.is_some_and(|t| t.contains("tool")) {
                c.push(Capability::Tools);
            }
            if has_projector {
                c.push(Capability::Vision);
            }
            c
        }
    }
}

/// What the app prefers among models that qualify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Prefer {
    /// A loaded model first (no cold start), then the fastest.
    #[default]
    Balanced,
    /// The highest tokens/s.
    Fast,
    /// The largest model that fits.
    Quality,
    /// The smallest model.
    Small,
}

/// What the app needs.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Requirements {
    /// Capabilities it must have.
    pub capabilities: Vec<Capability>,
    /// Context it must offer.
    pub min_context: Option<u32>,
    /// The tie-breaker.
    pub prefer: Prefer,
}

impl Requirements {
    /// Parses `auto:vision,tools,8k,fast` (the part after `auto:` of a `model` field). Unknown words are refused so
    /// a typo doesn't silently pick another model.
    pub fn parse_auto(spec: &str) -> Result<Requirements, String> {
        let mut r = Requirements::default();
        for w in spec.split([',', '+', ' ']).map(str::trim).filter(|w| !w.is_empty()) {
            let lw = w.to_ascii_lowercase();
            if let Some(c) = Capability::parse(&lw) {
                if !r.capabilities.contains(&c) {
                    r.capabilities.push(c);
                }
            } else if let Some(n) = parse_context(&lw) {
                r.min_context = Some(n);
            } else {
                r.prefer = match lw.as_str() {
                    "fast" => Prefer::Fast,
                    "quality" | "best" => Prefer::Quality,
                    "small" | "light" => Prefer::Small,
                    "balanced" => Prefer::Balanced,
                    _ => return Err(format!("{w:?} is not a capability (chat, vision, tools, json, embeddings, rerank), a context like 8k, or fast/quality/small")),
                };
            }
        }
        Ok(r)
    }
}

/// `8k`, `32K`, `ctx>=8k`, `16384`.
fn parse_context(w: &str) -> Option<u32> {
    let w = w.trim_start_matches("ctx").trim_start_matches(">=").trim_start_matches('=');
    let (n, mult) = match w.strip_suffix('k') {
        Some(n) => (n, 1024),
        None => (w, 1),
    };
    n.parse::<u32>().ok().filter(|n| *n > 0).and_then(|n| n.checked_mul(mult))
}

/// A model that could be chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Its id.
    pub id: ModelId,
    /// What it can do.
    pub capabilities: Vec<Capability>,
    /// Its trained context.
    pub context_max: Option<u32>,
    /// File size.
    pub size: u64,
    /// Loaded now.
    pub loaded: bool,
    /// Measured generation speed on this machine.
    pub gen_tps: Option<f64>,
    /// Whether it fits now.
    pub fit: Verdict,
}

/// The choice and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// The model.
    pub id: ModelId,
    /// One sentence for logs and the inspector.
    pub reason: String,
}

/// Picks a model, or says what nothing offered.
pub fn choose(req: &Requirements, candidates: &[Candidate]) -> Result<Choice, String> {
    let needed: Vec<Capability> = if req.capabilities.is_empty() {
        vec![Capability::Chat]
    } else {
        req.capabilities.clone()
    };
    let ok: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| needed.iter().all(|n| c.capabilities.contains(n)))
        .filter(|c| req.min_context.is_none_or(|m| c.context_max.is_none_or(|have| have >= m)))
        .filter(|c| c.fit != Verdict::WontFit)
        .collect();
    let speed = |c: &Candidate| c.gen_tps.unwrap_or(1e12 / (c.size.max(1) as f64));
    let best = match req.prefer {
        Prefer::Balanced => ok.iter().max_by(|a, b| {
            (a.loaded, speed(a))
                .partial_cmp(&(b.loaded, speed(b)))
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        Prefer::Fast => ok.iter().max_by(|a, b| speed(a).total_cmp(&speed(b))),
        Prefer::Quality => ok.iter().max_by_key(|c| c.size),
        Prefer::Small => ok.iter().min_by_key(|c| c.size),
    };
    match best {
        Some(c) => {
            let names: Vec<String> = needed.iter().map(|n| format!("{n:?}").to_ascii_lowercase()).collect();
            Ok(Choice {
                id: c.id.clone(),
                reason: format!(
                    "chosen for {} ({:?}{})",
                    names.join(", "),
                    req.prefer,
                    if c.loaded { ", already loaded" } else { "" }
                ),
            })
        }
        None => Err(format!(
            "no installed model offers {}{}; download one in Pitwall ▸ Models",
            needed
                .iter()
                .map(|n| format!("{n:?}").to_ascii_lowercase())
                .collect::<Vec<_>>()
                .join(", "),
            req.min_context
                .map(|m| format!(" with at least {m} tokens of context"))
                .unwrap_or_default()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, caps: &[Capability], size: u64, loaded: bool, tps: Option<f64>) -> Candidate {
        Candidate {
            id: ModelId::parse(id).unwrap(),
            capabilities: caps.to_vec(),
            context_max: Some(32768),
            size,
            loaded,
            gen_tps: tps,
            fit: Verdict::Fits,
        }
    }

    #[test]
    fn capability_names_and_derivation() {
        for (s, c) in [
            ("vision", Capability::Vision),
            ("TOOLS", Capability::Tools),
            ("embed", Capability::Embeddings),
            ("rerank", Capability::Rerank),
            ("audio", Capability::Transcription),
            ("json", Capability::Json),
            ("chat", Capability::Chat),
        ] {
            assert_eq!(Capability::parse(s), Some(c));
        }
        assert_eq!(Capability::parse("telepathy"), None);
        assert_eq!(
            capabilities_of(ModelKind::Embedding, false, None, "nomic-embed"),
            vec![Capability::Embeddings]
        );
        assert_eq!(
            capabilities_of(ModelKind::Embedding, false, None, "bge-reranker"),
            vec![Capability::Rerank]
        );
        let c = capabilities_of(ModelKind::Language, true, Some("{% if tools %}"), "m");
        assert_eq!(c, vec![Capability::Chat, Capability::Json, Capability::Tools, Capability::Vision]);
        assert_eq!(
            capabilities_of(ModelKind::Language, false, None, "m"),
            vec![Capability::Chat, Capability::Json]
        );
        assert_eq!(serde_json::to_value(Capability::Embeddings).unwrap(), "embeddings");
    }

    #[test]
    fn auto_specs() {
        let r = Requirements::parse_auto("vision, tools,8k fast").unwrap();
        assert_eq!(r.capabilities, vec![Capability::Vision, Capability::Tools]);
        assert_eq!((r.min_context, r.prefer), (Some(8192), Prefer::Fast));
        assert_eq!(Requirements::parse_auto("ctx>=32k+quality").unwrap().min_context, Some(32768));
        assert_eq!(Requirements::parse_auto("16384,small,vision,vision").unwrap().capabilities.len(), 1);
        assert_eq!(Requirements::parse_auto("best").unwrap().prefer, Prefer::Quality);
        assert_eq!(Requirements::parse_auto("balanced").unwrap().prefer, Prefer::Balanced);
        assert_eq!(Requirements::parse_auto("").unwrap(), Requirements::default());
        assert!(Requirements::parse_auto("visoin").unwrap_err().contains("not a capability"));
        assert_eq!(parse_context("0k"), None);
        assert_eq!(parse_context("99999999k"), None);
    }

    #[test]
    fn choosing() {
        use Capability::*;
        let small = cand("small", &[Chat, Json], 2, false, None);
        let big = cand("big", &[Chat, Json, Tools], 8, false, Some(20.0));
        let vis = cand("vis", &[Chat, Vision], 6, true, Some(30.0));
        let emb = cand("emb", &[Embeddings], 1, false, None);
        let all = [small.clone(), big.clone(), vis.clone(), emb.clone()];
        let pick = |caps: &[Capability], prefer: Prefer| {
            choose(
                &Requirements {
                    capabilities: caps.to_vec(),
                    min_context: None,
                    prefer,
                },
                &all,
            )
            .map(|c| c.id.to_string())
        };
        assert_eq!(pick(&[], Prefer::Balanced).unwrap(), "vis", "loaded first");
        assert_eq!(pick(&[Vision], Prefer::Fast).unwrap(), "vis");
        assert_eq!(pick(&[Chat], Prefer::Quality).unwrap(), "big");
        assert_eq!(pick(&[Chat], Prefer::Small).unwrap(), "small");
        assert_eq!(pick(&[Embeddings], Prefer::Balanced).unwrap(), "emb");
        assert_eq!(pick(&[Tools], Prefer::Fast).unwrap(), "big");
        let e = pick(&[Rerank], Prefer::Balanced).unwrap_err();
        assert!(e.contains("rerank") && e.contains("Models"), "{e}");
        // Context and fit filters.
        let short = Candidate {
            context_max: Some(4096),
            ..big.clone()
        };
        let r = Requirements {
            capabilities: vec![Tools],
            min_context: Some(8192),
            prefer: Prefer::Balanced,
        };
        assert!(choose(&r, &[short]).unwrap_err().contains("8192 tokens"));
        let huge = Candidate {
            fit: Verdict::WontFit,
            ..big.clone()
        };
        assert!(choose(
            &Requirements {
                capabilities: vec![Tools],
                ..Default::default()
            },
            &[huge]
        )
        .is_err());
        let c = choose(&Requirements::default(), &[vis]).unwrap();
        assert!(c.reason.contains("already loaded"));
        // Unknown speed: smaller is assumed faster.
        assert_eq!(
            choose(
                &Requirements {
                    prefer: Prefer::Fast,
                    ..Default::default()
                },
                &[small.clone(), Candidate { gen_tps: None, ..big }]
            )
            .unwrap()
            .id
            .as_str(),
            "small"
        );
        let nan = Candidate {
            gen_tps: Some(f64::NAN),
            ..small.clone()
        };
        assert!(choose(&Requirements::default(), &[nan, small]).is_ok());
    }
}
