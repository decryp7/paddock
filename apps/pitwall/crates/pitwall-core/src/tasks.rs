//! Task endpoints (`/x/v1/summarize`, `/x/v1/classify`, `/x/v1/extract`, `/x/v1/translate`; ADR 0003): typed inputs
//! turned into chat requests with versioned prompt templates. A template's text never changes within a version; an
//! improvement is a new version, and clients may pin one (`"template": "summarize@1"`).

use crate::api::ApiError;
use crate::chat::{ChatRequest, Message, ModelRef, OutputFormat, Role};
use serde_json::{json, Value};

/// Longest input text of a task.
pub const MAX_TASK_TEXT: usize = 2 * 1024 * 1024;
/// Most labels of a classification.
pub const MAX_LABELS: usize = 200;

/// A task kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// A summary.
    Summarize,
    /// One label (or several) from a list.
    Classify,
    /// JSON matching a schema.
    Extract,
    /// A translation.
    Translate,
}

impl Task {
    /// From the route name.
    pub fn parse(s: &str) -> Option<Task> {
        match s {
            "summarize" => Some(Task::Summarize),
            "classify" => Some(Task::Classify),
            "extract" => Some(Task::Extract),
            "translate" => Some(Task::Translate),
            _ => None,
        }
    }

    /// The route name.
    pub fn name(self) -> &'static str {
        match self {
            Task::Summarize => "summarize",
            Task::Classify => "classify",
            Task::Extract => "extract",
            Task::Translate => "translate",
        }
    }

    /// The newest template version.
    pub fn latest(self) -> u32 {
        1
    }
}

fn system(task: Task, version: u32) -> Option<&'static str> {
    match (task, version) {
        (Task::Summarize, 1) => Some("You summarise text accurately and concisely. Keep facts, names and numbers exact. Do not add information that is not in the text. Answer with the summary only."),
        (Task::Classify, 1) => Some("You classify text. Answer with JSON only, choosing labels exactly as given."),
        (Task::Extract, 1) => Some("You extract information from text into JSON that matches the given schema. Use null for anything the text does not say. Answer with JSON only."),
        (Task::Translate, 1) => Some("You translate text faithfully, keeping formatting, names and numbers. Answer with the translation only."),
        _ => None,
    }
}

fn text_of(v: &Value) -> Result<String, ApiError> {
    let t = v.get("text").and_then(Value::as_str).ok_or_else(|| ApiError::invalid("`text` is required"))?;
    if t.len() > MAX_TASK_TEXT {
        return Err(ApiError::new(crate::api::ErrorKind::TooLarge, format!("`text` is longer than {MAX_TASK_TEXT} bytes")));
    }
    Ok(t.to_string())
}

/// Builds the chat request of a task from its JSON input. Returns the request and the template used (`name@version`).
pub fn build(task: Task, v: &Value) -> Result<(ChatRequest, String), ApiError> {
    let version = match v.get("template").and_then(Value::as_str) {
        None => task.latest(),
        Some(t) => {
            let (name, ver) = t.split_once('@').ok_or_else(|| ApiError::invalid("`template` must be name@version"))?;
            if name != task.name() {
                return Err(ApiError::invalid(format!("template {t:?} is not a {} template", task.name())));
            }
            ver.parse().map_err(|_| ApiError::invalid("the template version must be a number"))?
        }
    };
    let sys = system(task, version).ok_or_else(|| ApiError::invalid(format!("there is no {}@{version} template", task.name())))?;
    let text = text_of(v)?;
    let (user, format) = match task {
        Task::Summarize => {
            let style = match v.get("style").and_then(Value::as_str).unwrap_or("paragraph") {
                "bullets" => "as short bullet points",
                "headline" => "as one headline",
                _ => "as one short paragraph",
            };
            let words = v.get("maxWords").and_then(Value::as_u64).map(|w| format!(" in at most {w} words")).unwrap_or_default();
            (format!("Summarise the following text {style}{words}.\n\n<text>\n{text}\n</text>"), OutputFormat::Text)
        }
        Task::Classify => {
            let labels: Vec<String> = v
                .get("labels")
                .and_then(Value::as_array)
                .ok_or_else(|| ApiError::invalid("`labels` is required"))?
                .iter()
                .map(|l| l.as_str().map(str::to_string).ok_or_else(|| ApiError::invalid("`labels` must hold strings")))
                .collect::<Result<_, _>>()?;
            if labels.is_empty() || labels.len() > MAX_LABELS {
                return Err(ApiError::invalid(format!("give 1 to {MAX_LABELS} labels")));
            }
            let multi = v.get("multiLabel").and_then(Value::as_bool).unwrap_or(false);
            let schema = if multi {
                json!({"type": "object", "properties": {"labels": {"type": "array", "items": {"enum": labels}}}, "required": ["labels"]})
            } else {
                json!({"type": "object", "properties": {"label": {"enum": labels}}, "required": ["label"]})
            };
            let ask = if multi { "every label that applies" } else { "the one label that fits best" };
            (format!("Choose {ask} from: {}.\n\n<text>\n{text}\n</text>", labels.join(", ")), OutputFormat::JsonSchema(schema))
        }
        Task::Extract => {
            let schema = v.get("schema").filter(|s| s.is_object()).cloned().ok_or_else(|| ApiError::invalid("`schema` (a JSON schema object) is required"))?;
            (format!("Extract the information as JSON matching this schema:\n{schema}\n\n<text>\n{text}\n</text>"), OutputFormat::JsonSchema(schema))
        }
        Task::Translate => {
            let to = v.get("to").and_then(Value::as_str).filter(|s| !s.trim().is_empty() && s.len() <= 64).ok_or_else(|| ApiError::invalid("`to` (the target language) is required"))?;
            let from = v.get("from").and_then(Value::as_str).map(|f| format!(" from {f}")).unwrap_or_default();
            (format!("Translate the following text{from} into {to}.\n\n<text>\n{text}\n</text>"), OutputFormat::Text)
        }
    };
    let model = match v.get("model").and_then(Value::as_str) {
        Some(m) if !m.trim().is_empty() && m != "auto" => ModelRef::Named(m.trim().to_string()),
        _ => ModelRef::Default,
    };
    let req = ChatRequest {
        model,
        messages: vec![Message::text(Role::System, sys), Message::text(Role::User, user)],
        format,
        temperature: Some(0.0),
        max_tokens: v.get("maxTokens").and_then(Value::as_u64).map(|n| n.min(u64::from(u32::MAX)) as u32),
        ..Default::default()
    };
    Ok((req, format!("{}@{version}", task.name())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_build_requests() {
        for n in ["summarize", "classify", "extract", "translate"] {
            assert_eq!(Task::parse(n).unwrap().name(), n);
        }
        assert_eq!(Task::parse("poem"), None);
        let (r, t) = build(Task::Summarize, &json!({"text": "long text", "style": "bullets", "maxWords": 50, "model": "m"})).unwrap();
        assert_eq!(t, "summarize@1");
        assert!(r.messages[1].text_content().contains("bullet points in at most 50 words"));
        assert_eq!(r.model, ModelRef::Named("m".into()));
        assert_eq!(r.temperature, Some(0.0));
        let (r, _) = build(Task::Summarize, &json!({"text": "x", "style": "headline", "model": "auto"})).unwrap();
        assert!(r.messages[1].text_content().contains("one headline"));
        assert_eq!(r.model, ModelRef::Default);
        assert!(build(Task::Summarize, &json!({"text": "x"})).unwrap().0.messages[1].text_content().contains("paragraph"));
        let (r, _) = build(Task::Classify, &json!({"text": "x", "labels": ["bug", "feature"]})).unwrap();
        assert!(matches!(&r.format, OutputFormat::JsonSchema(s) if s["properties"]["label"]["enum"][1] == "feature"));
        let (r, _) = build(Task::Classify, &json!({"text": "x", "labels": ["a"], "multiLabel": true, "maxTokens": 9})).unwrap();
        assert!(matches!(&r.format, OutputFormat::JsonSchema(s) if s["properties"]["labels"]["type"] == "array"));
        assert_eq!(r.max_tokens, Some(9));
        let (r, _) = build(Task::Extract, &json!({"text": "x", "schema": {"type": "object"}})).unwrap();
        assert!(matches!(r.format, OutputFormat::JsonSchema(_)));
        let (r, _) = build(Task::Translate, &json!({"text": "Hallo", "from": "German", "to": "English", "template": "translate@1"})).unwrap();
        assert!(r.messages[1].text_content().contains("from German into English"));
    }

    #[test]
    fn bad_inputs_are_refused() {
        let cases = [
            (Task::Summarize, json!({}), "`text` is required"),
            (Task::Summarize, json!({"text": "x", "template": "summarize"}), "name@version"),
            (Task::Summarize, json!({"text": "x", "template": "classify@1"}), "not a summarize"),
            (Task::Summarize, json!({"text": "x", "template": "summarize@x"}), "a number"),
            (Task::Summarize, json!({"text": "x", "template": "summarize@9"}), "no summarize@9"),
            (Task::Classify, json!({"text": "x"}), "`labels`"),
            (Task::Classify, json!({"text": "x", "labels": []}), "1 to"),
            (Task::Classify, json!({"text": "x", "labels": [1]}), "strings"),
            (Task::Extract, json!({"text": "x", "schema": "s"}), "`schema`"),
            (Task::Translate, json!({"text": "x"}), "`to`"),
        ];
        for (t, v, needle) in cases {
            let e = build(t, &v).unwrap_err().to_string();
            assert!(e.contains(needle), "{v}: {e}");
        }
        let big = "a".repeat(MAX_TASK_TEXT + 1);
        assert_eq!(build(Task::Summarize, &json!({"text": big})).unwrap_err().status(), 413);
    }
}
