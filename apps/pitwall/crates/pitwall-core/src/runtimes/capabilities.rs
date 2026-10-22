//! The capability matrix (ADR 0018): what each feature (vision, tools, embeddings, rerank, JSON-schema output) needs
//! from the llama.cpp engine build, and the pure [`Matrix::gate`] that answers "may this build serve it?".
//!
//! The matrix is data: `capabilities.json` next to `llama-runtimes.json`, embedded at build time. Each capability has
//! a minimum llama.cpp release number (`minBuild`, e.g. 5600 for `b5600`) and, optionally, backends that can never
//! serve it (`unsupportedBackends`, a build id to a reason). A build whose number is unknown (a custom `llama-server`)
//! is trusted: Pitwall cannot tell, and refusing would only get in the way.

use super::builds::tag_number;
use crate::api::{ApiError, ErrorKind};
use crate::select::Capability;
use serde::Deserialize;
use std::collections::BTreeMap;

/// The embedded matrix.
const EMBEDDED: &str = include_str!("capabilities.json");

/// The newest schema version understood.
pub const VERSION: u32 = 1;

/// What one capability needs.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Requirement {
    /// Plain words for the UI.
    pub label: String,
    /// The oldest llama.cpp release number that serves it (0: every build).
    #[serde(default)]
    pub min_build: u64,
    /// Build ids that cannot serve it, with the reason.
    #[serde(default)]
    pub unsupported_backends: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct File {
    version: u32,
    capabilities: BTreeMap<String, serde_json::Value>,
}

/// The matrix: capability to requirement.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Matrix {
    entries: BTreeMap<Capability, Requirement>,
}

/// The facts about a build the matrix needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildFacts {
    /// The build id (`vulkan`, `cuda-13`, ...).
    pub id: String,
    /// The llama.cpp release number, when known.
    pub number: Option<u64>,
}

impl BuildFacts {
    /// Facts from a build id and a release tag such as `b11669`.
    pub fn new(id: &str, tag: &str) -> BuildFacts {
        BuildFacts {
            id: id.to_string(),
            number: tag_number(tag),
        }
    }
}

/// The answer of [`Matrix::gate`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// The build can serve everything asked.
    Allowed,
    /// A newer build can; install it in Engines.
    NeedsBuild {
        /// The capability that needs the newest build.
        capability: Capability,
        /// The release number needed.
        min_build: u64,
        /// The release number of the build in use.
        have: u64,
        /// What to do.
        hint: String,
    },
    /// This kind of build never can.
    NotSupported {
        /// The capability.
        capability: Capability,
        /// Why.
        reason: String,
    },
}

impl Matrix {
    /// The embedded matrix (checked by a test, so it always parses).
    pub fn embedded() -> Matrix {
        Matrix::parse(EMBEDDED).unwrap_or_default()
    }

    /// Parses a matrix. Unknown capability names and entries that do not parse are left out (a newer file with extra
    /// features still works); a newer `version` or damaged JSON is an error.
    pub fn parse(text: &str) -> Result<Matrix, String> {
        let f: File = serde_json::from_str(text).map_err(|e| format!("the capability matrix is not valid: {e}"))?;
        if f.version > VERSION {
            return Err(format!("the capability matrix has version {}, newer than {VERSION}", f.version));
        }
        let mut entries = BTreeMap::new();
        for (name, v) in f.capabilities {
            let cap = serde_json::from_value::<Capability>(serde_json::Value::String(name)).ok();
            let req = serde_json::from_value::<Requirement>(v).ok();
            if let (Some(c), Some(r)) = (cap, req) {
                entries.insert(c, r);
            }
        }
        Ok(Matrix { entries })
    }

    /// A matrix from entries (tests and callers that build one).
    pub fn from_entries(entries: impl IntoIterator<Item = (Capability, Requirement)>) -> Matrix {
        Matrix {
            entries: entries.into_iter().collect(),
        }
    }

    /// The requirement of a capability (none: every build serves it).
    pub fn requirement(&self, c: Capability) -> Option<&Requirement> {
        self.entries.get(&c)
    }

    /// Whether `build` can serve every capability in `needed`. A backend that can never serve one wins over a build
    /// that is merely too old; among too-old answers the newest requirement is returned, so one upgrade fixes all.
    pub fn gate(&self, needed: &[Capability], build: &BuildFacts) -> Gate {
        let mut worst: Option<(u64, Capability)> = None;
        for &c in needed {
            let Some(req) = self.entries.get(&c) else { continue };
            if let Some(reason) = req.unsupported_backends.get(&build.id) {
                return Gate::NotSupported {
                    capability: c,
                    reason: reason.clone(),
                };
            }
            if let Some(have) = build.number {
                if have < req.min_build && worst.is_none_or(|(m, _)| req.min_build > m) {
                    worst = Some((req.min_build, c));
                }
            }
        }
        match worst {
            Some((min_build, capability)) => Gate::NeedsBuild {
                capability,
                min_build,
                have: build.number.unwrap_or(0),
                hint: upgrade_hint(min_build),
            },
            None => Gate::Allowed,
        }
    }

    /// One answer per capability the matrix knows, in capability order (for the Engines page).
    pub fn report(&self, build: &BuildFacts) -> Vec<(Capability, String, Gate)> {
        self.entries
            .iter()
            .map(|(c, r)| (*c, r.label.clone(), self.gate(&[*c], build)))
            .collect()
    }

    /// The label of a capability.
    pub fn label(&self, c: Capability) -> String {
        self.entries.get(&c).map_or_else(|| format!("{c:?}"), |r| r.label.clone())
    }
}

/// What to tell the user when a build is too old.
pub fn upgrade_hint(min_build: u64) -> String {
    format!("Install llama.cpp b{min_build} or newer in Engines, or unpin the old build.")
}

impl Gate {
    /// The API error for a refused request, `None` when allowed. `model` and `build` (a label such as `vulkan b4000`)
    /// are named so the client and the person behind it know what to change.
    pub fn into_error(self, matrix: &Matrix, model: &str, build: &str) -> Option<ApiError> {
        match self {
            Gate::Allowed => None,
            Gate::NeedsBuild {
                capability,
                min_build,
                have,
                hint,
            } => Some(ApiError::new(
                ErrorKind::EngineBuildTooOld,
                format!(
                    "{model} needs {}, which needs llama.cpp b{min_build} or newer; the engine build in use ({build}, b{have}) is older. {hint}",
                    matrix.label(capability).to_lowercase()
                ),
            )),
            Gate::NotSupported { capability, reason } => Some(ApiError::new(
                ErrorKind::CapabilityUnsupported,
                format!(
                    "{model} needs {}, which the engine build in use ({build}) cannot serve: {reason}. Use another build in Engines or a model that does not need it.",
                    matrix.label(capability).to_lowercase()
                ),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(label: &str, min: u64, bad: &[(&str, &str)]) -> Requirement {
        Requirement {
            label: label.into(),
            min_build: min,
            unsupported_backends: bad.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        }
    }

    fn matrix() -> Matrix {
        Matrix::from_entries([
            (
                Capability::Vision,
                req("Image input", 5600, &[("openvino", "OpenVINO has no multimodal path")]),
            ),
            (Capability::Tools, req("Tool calling", 4600, &[])),
            (Capability::Rerank, req("Reranking", 4100, &[])),
        ])
    }

    #[test]
    fn facts_come_from_the_id_and_the_release_tag() {
        assert_eq!(BuildFacts::new("vulkan", "b11669"), b("vulkan", Some(11669)));
        assert_eq!(BuildFacts::new("cpu", "nightly"), b("cpu", None));
    }

    fn b(id: &str, n: Option<u64>) -> BuildFacts {
        BuildFacts { id: id.into(), number: n }
    }

    #[test]
    fn embedded_matrix_parses_and_is_complete() {
        let m = Matrix::embedded();
        for c in [
            Capability::Chat,
            Capability::Json,
            Capability::Tools,
            Capability::Vision,
            Capability::Embeddings,
            Capability::Rerank,
        ] {
            assert!(m.requirement(c).is_some(), "{c:?}");
        }
        // The catalog's own pin serves everything.
        assert_eq!(
            m.gate(
                &[Capability::Vision, Capability::Tools, Capability::Rerank],
                &b("vulkan", Some(11669))
            ),
            Gate::Allowed
        );
        assert!(m.requirement(Capability::Transcription).is_none());
    }

    #[test]
    fn gate_truth_table() {
        let m = matrix();
        // Allowed: new enough, nothing needed, capability the matrix does not know, unknown build number.
        assert_eq!(m.gate(&[Capability::Vision], &b("vulkan", Some(5600))), Gate::Allowed);
        assert_eq!(m.gate(&[], &b("vulkan", Some(1))), Gate::Allowed);
        assert_eq!(m.gate(&[Capability::Chat], &b("vulkan", Some(1))), Gate::Allowed);
        assert_eq!(m.gate(&[Capability::Vision], &b("custom", None)), Gate::Allowed);
        // Too old.
        let g = m.gate(&[Capability::Vision], &b("vulkan", Some(5599)));
        assert!(
            matches!(&g, Gate::NeedsBuild { capability: Capability::Vision, min_build: 5600, have: 5599, hint } if hint.contains("b5600"))
        );
        // The newest requirement wins, so one upgrade fixes all.
        let g = m.gate(&[Capability::Tools, Capability::Vision, Capability::Rerank], &b("cpu", Some(4000)));
        assert!(matches!(
            g,
            Gate::NeedsBuild {
                capability: Capability::Vision,
                min_build: 5600,
                ..
            }
        ));
        // Only the lacking capability is named.
        let g = m.gate(&[Capability::Tools, Capability::Vision], &b("cpu", Some(5000)));
        assert!(matches!(
            g,
            Gate::NeedsBuild {
                capability: Capability::Vision,
                ..
            }
        ));
        // A backend that can never serve it wins over "too old".
        let g = m.gate(&[Capability::Tools, Capability::Vision], &b("openvino", Some(1)));
        assert!(matches!(&g, Gate::NotSupported { capability: Capability::Vision, reason } if reason.contains("multimodal")));
        // The unsupported backend is checked even for a build of unknown number.
        assert!(matches!(
            m.gate(&[Capability::Vision], &b("openvino", None)),
            Gate::NotSupported { .. }
        ));
        // Other backends are fine.
        assert_eq!(m.gate(&[Capability::Vision], &b("metal", Some(9000))), Gate::Allowed);
    }

    #[test]
    fn report_lists_every_capability() {
        let r = matrix().report(&b("cpu", Some(4200)));
        assert_eq!(r.len(), 3);
        let by = |c| r.iter().find(|(x, _, _)| *x == c).map(|(_, _, g)| g.clone());
        assert_eq!(
            by(Capability::Tools),
            Some(Gate::NeedsBuild {
                capability: Capability::Tools,
                min_build: 4600,
                have: 4200,
                hint: upgrade_hint(4600)
            })
        );
        assert_eq!(by(Capability::Rerank), Some(Gate::Allowed));
        assert!(!r[0].1.is_empty());
    }

    #[test]
    fn parse_is_tolerant_but_not_of_damage_or_newer_files() {
        let m = Matrix::parse(
            r#"{"version":1,"capabilities":{"vision":{"label":"V","minBuild":7},"telepathy":{"label":"T"},"tools":{"nonsense":1},"json":{"label":"J"}}}"#,
        )
        .unwrap();
        assert_eq!(m.requirement(Capability::Vision).map(|r| r.min_build), Some(7));
        assert_eq!(m.requirement(Capability::Json).map(|r| r.min_build), Some(0));
        assert!(m.requirement(Capability::Tools).is_none());
        assert!(Matrix::parse("{").unwrap_err().contains("not valid"));
        assert!(Matrix::parse(r#"{"version":99,"capabilities":{}}"#).unwrap_err().contains("newer"));
    }

    #[test]
    fn refusals_are_explained_errors() {
        let m = matrix();
        assert!(Gate::Allowed.into_error(&m, "x", "cpu b1").is_none());
        let old = m
            .gate(&[Capability::Vision], &b("cpu", Some(4000)))
            .into_error(&m, "llava", "cpu b4000")
            .unwrap();
        assert_eq!(
            (old.kind, old.status(), old.code()),
            (ErrorKind::EngineBuildTooOld, 503, "engine_build_too_old")
        );
        assert!(
            old.message.contains("llava")
                && old.message.contains("b5600")
                && old.message.contains("b4000")
                && old.message.contains("Engines"),
            "{}",
            old.message
        );
        let no = m
            .gate(&[Capability::Vision], &b("openvino", Some(9000)))
            .into_error(&m, "llava", "openvino b9000")
            .unwrap();
        assert_eq!(
            (no.kind, no.status(), no.code()),
            (ErrorKind::CapabilityUnsupported, 400, "capability_unsupported")
        );
        assert!(no.message.contains("openvino") && no.message.contains("multimodal"));
        // Labels fall back to the capability's name for a capability the matrix does not list.
        assert_eq!(matrix().label(Capability::Chat), "Chat");
    }
}
