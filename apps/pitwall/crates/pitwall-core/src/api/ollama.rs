//! Ollama-compatible API: `/api/chat`, `/api/generate`, `/api/embed` (and the legacy `/api/embeddings`), `/api/tags`,
//! `/api/ps`, `/api/show` and `/api/version`. Streams are NDJSON and **on by default**, as in Ollama. Model management
//! (`pull`, `push`, `create`, `copy`, `delete`) is Pitwall's window's job and answered "not available".

use super::{object, opt, opt_f32, opt_u32, stops, ApiError, ModelEntry};
use crate::chat::{
    ChatEvent, ChatOutput, ChatRequest, EmbedOutput, EmbedRequest, Finish, Image, ImageMime, Message, ModelRef, OutputFormat, Part, Role,
    Tool, ToolCall,
};
use crate::time::{rfc3339, Millis};
use serde_json::{json, Map, Value};

/// The Ollama version Pitwall reports (`/api/version`): clients check it for feature support.
pub const COMPAT_VERSION: &str = "0.12.0";

/// How long to keep a model loaded after this request (`keep_alive`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeepAlive {
    /// Unload right after (`0`).
    Unload,
    /// Keep for this long.
    For(Millis),
    /// Keep loaded (negative values).
    Forever,
}

/// Parses `keep_alive`: seconds as a number, or a duration string (`"5m"`, `"1h"`, `"30s"`, `"-1"`).
pub fn parse_keep_alive(v: &Value) -> Result<Option<KeepAlive>, ApiError> {
    let bad = || ApiError::invalid("`keep_alive` must be a number of seconds or a duration such as \"5m\"");
    let secs: f64 = match v {
        Value::Null => return Ok(None),
        Value::Number(n) => n.as_f64().ok_or_else(bad)?,
        Value::String(s) => {
            let s = s.trim();
            let (num, mult) = if let Some(n) = s.strip_suffix("ms") {
                (n, 0.001)
            } else if let Some(n) = s.strip_suffix('s') {
                (n, 1.0)
            } else if let Some(n) = s.strip_suffix('m') {
                (n, 60.0)
            } else if let Some(n) = s.strip_suffix('h') {
                (n, 3600.0)
            } else {
                (s, 1.0)
            };
            num.trim().parse::<f64>().map_err(|_| bad())? * mult
        }
        _ => return Err(bad()),
    };
    Ok(Some(if !secs.is_finite() || secs < 0.0 {
        KeepAlive::Forever
    } else if secs == 0.0 {
        KeepAlive::Unload
    } else {
        KeepAlive::For((secs * 1000.0).min(1e15) as Millis)
    }))
}

/// A parsed `/api/chat` or `/api/generate` request.
#[derive(Debug, Clone, PartialEq)]
pub struct OllamaRequest {
    /// The neutral request.
    pub chat: ChatRequest,
    /// `keep_alive`.
    pub keep_alive: Option<KeepAlive>,
    /// No messages or an empty prompt: only load (or, with `keep_alive: 0`, unload) the model.
    pub load_only: bool,
}

fn images(o: &Map<String, Value>) -> Result<Vec<Part>, ApiError> {
    let Some(list) = opt(o, "images", Value::as_array, "an array of base64 strings")? else {
        return Ok(vec![]);
    };
    list.iter()
        .map(|i| {
            let data = i.as_str().ok_or_else(|| ApiError::invalid("`images` must hold base64 strings"))?;
            let mime = ImageMime::sniff_base64(data).ok_or_else(|| ApiError::invalid("an image is not PNG, JPEG, WebP or GIF"))?;
            Ok(Part::Image(Image::from_base64(mime, data)?))
        })
        .collect()
}

fn options(o: &Map<String, Value>, req: &mut ChatRequest) -> Result<(), ApiError> {
    req.model = match o.get("model").and_then(Value::as_str) {
        Some(m) if !m.trim().is_empty() => ModelRef::Named(m.trim().trim_end_matches(":latest").to_string()),
        _ => return Err(ApiError::invalid("`model` is required")),
    };
    req.stream = opt(o, "stream", Value::as_bool, "a boolean")?.unwrap_or(true);
    req.format = match o.get("format") {
        None | Some(Value::Null) => OutputFormat::Text,
        Some(Value::String(s)) if s == "json" => OutputFormat::JsonObject,
        Some(Value::String(s)) if s.is_empty() => OutputFormat::Text,
        Some(v @ Value::Object(_)) => OutputFormat::JsonSchema(v.clone()),
        Some(_) => return Err(ApiError::invalid("`format` must be \"json\" or a JSON schema")),
    };
    if let Some(opts) = opt(o, "options", Value::as_object, "an object")? {
        req.temperature = opt_f32(opts, "temperature")?;
        req.top_p = opt_f32(opts, "top_p")?;
        req.top_k = opt_u32(opts, "top_k")?;
        req.seed = opt(opts, "seed", Value::as_i64, "an integer")?;
        req.stop = stops(opts, "stop")?;
        req.context = opt_u32(opts, "num_ctx")?;
        // -1 / -2 mean "until the context is full" in Ollama.
        req.max_tokens = match opts.get("num_predict").and_then(Value::as_i64) {
            Some(n) if n > 0 => Some(n.min(i64::from(u32::MAX)) as u32),
            _ => None,
        };
    }
    if let Some(list) = opt(o, "tools", Value::as_array, "an array")? {
        for t in list {
            let f = t.get("function").unwrap_or(t);
            let name = f
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::invalid("a tool has no `name`"))?;
            req.tools.push(Tool {
                name: name.into(),
                description: f.get("description").and_then(Value::as_str).unwrap_or_default().into(),
                parameters: f.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object"})),
            });
        }
    }
    Ok(())
}

/// Parses `POST /api/chat`.
pub fn parse_chat(v: &Value) -> Result<OllamaRequest, ApiError> {
    let o = object(v, "the request")?;
    let mut chat = ChatRequest::default();
    options(o, &mut chat)?;
    for (n, m) in opt(o, "messages", Value::as_array, "an array")?.into_iter().flatten().enumerate() {
        let mo = object(m, "a message")?;
        let role = match mo.get("role").and_then(Value::as_str) {
            Some("system") => Role::System,
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            Some("tool") => Role::Tool,
            _ => return Err(ApiError::invalid("each message's `role` must be system, user, assistant or tool")),
        };
        let mut parts = vec![Part::Text(
            mo.get("content").and_then(Value::as_str).unwrap_or_default().to_string(),
        )];
        parts.extend(images(mo)?);
        let tool_calls = mo
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .enumerate()
            .map(|(i, c)| ToolCall {
                id: format!("call_{n}_{i}"),
                name: c.pointer("/function/name").and_then(Value::as_str).unwrap_or_default().into(),
                arguments: c.pointer("/function/arguments").map_or_else(|| "{}".into(), Value::to_string),
            })
            .collect();
        chat.messages.push(Message {
            role,
            parts,
            tool_calls,
            tool_call_id: mo.get("tool_name").and_then(Value::as_str).map(str::to_string),
        });
    }
    let keep_alive = parse_keep_alive(o.get("keep_alive").unwrap_or(&Value::Null))?;
    let load_only = chat.messages.is_empty();
    if !load_only {
        chat.check()?;
    }
    Ok(OllamaRequest {
        chat,
        keep_alive,
        load_only,
    })
}

/// Parses `POST /api/generate` (`raw` and `suffix` are not supported: the model's chat template always applies).
pub fn parse_generate(v: &Value) -> Result<OllamaRequest, ApiError> {
    let o = object(v, "the request")?;
    let mut chat = ChatRequest::default();
    options(o, &mut chat)?;
    if o.get("suffix").and_then(Value::as_str).is_some_and(|s| !s.is_empty()) {
        return Err(ApiError::new(
            super::ErrorKind::NotAvailable,
            "fill-in-the-middle (`suffix`) is not supported",
        ));
    }
    if let Some(s) = opt(o, "system", Value::as_str, "a string")?.filter(|s| !s.is_empty()) {
        chat.messages.push(Message::text(Role::System, s));
    }
    let prompt = opt(o, "prompt", Value::as_str, "a string")?.unwrap_or_default();
    let mut parts = vec![Part::Text(prompt.to_string())];
    parts.extend(images(o)?);
    let load_only = prompt.is_empty() && parts.len() == 1;
    if !load_only {
        chat.messages.push(Message {
            role: Role::User,
            parts,
            tool_calls: vec![],
            tool_call_id: None,
        });
        chat.check()?;
    }
    let keep_alive = parse_keep_alive(o.get("keep_alive").unwrap_or(&Value::Null))?;
    Ok(OllamaRequest {
        chat,
        keep_alive,
        load_only,
    })
}

/// Durations of an answer (nanoseconds in Ollama's final object).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Stats {
    /// Whole request.
    pub total_ms: u64,
    /// Loading the model (cold start).
    pub load_ms: u64,
    /// Prompt processing.
    pub prompt_ms: u64,
    /// Generation.
    pub eval_ms: u64,
}

fn done_reason(f: Option<Finish>) -> &'static str {
    match f {
        Some(Finish::Length) => "length",
        _ => "stop",
    }
}

fn tool_calls_json(calls: &[ToolCall]) -> Value {
    calls.iter().map(|c| json!({"function": {"name": c.name, "arguments": serde_json::from_str::<Value>(&c.arguments).unwrap_or_else(|_| json!({}))}})).collect()
}

fn finals(o: &mut Map<String, Value>, out: &ChatOutput, s: &Stats) {
    let ns = |ms: u64| json!(ms.saturating_mul(1_000_000));
    o.insert("done".into(), json!(true));
    o.insert("done_reason".into(), json!(done_reason(out.finish)));
    o.insert("total_duration".into(), ns(s.total_ms));
    o.insert("load_duration".into(), ns(s.load_ms));
    o.insert("prompt_eval_count".into(), json!(out.usage.prompt_tokens));
    o.insert("prompt_eval_duration".into(), ns(s.prompt_ms));
    o.insert("eval_count".into(), json!(out.usage.completion_tokens));
    o.insert("eval_duration".into(), ns(s.eval_ms));
}

/// Writes `/api/chat` or `/api/generate` NDJSON lines.
#[derive(Debug)]
pub struct Stream {
    model: String,
    generate: bool,
    out: ChatOutput,
}

impl Stream {
    /// A stream for `/api/chat` (`generate: false`) or `/api/generate`.
    pub fn new(model: &str, generate: bool) -> Stream {
        Stream {
            model: model.into(),
            generate,
            out: ChatOutput::default(),
        }
    }

    fn line(&self, now: Millis, text: &str, tool_calls: Option<Value>) -> Map<String, Value> {
        let mut o = Map::new();
        o.insert("model".into(), json!(self.model));
        o.insert("created_at".into(), json!(rfc3339(now)));
        if self.generate {
            o.insert("response".into(), json!(text));
        } else {
            let mut m = json!({"role": "assistant", "content": text});
            if let Some(t) = tool_calls {
                m["tool_calls"] = t;
            }
            o.insert("message".into(), m);
        }
        o
    }

    /// The NDJSON lines for one event. Tool calls are sent whole, with the final line (Ollama never streams pieces).
    pub fn event(&mut self, e: &ChatEvent, now: Millis, stats: &Stats) -> Vec<String> {
        self.out.push(e);
        match e {
            ChatEvent::Delta(t) => {
                let mut o = self.line(now, t, None);
                o.insert("done".into(), json!(false));
                vec![format!("{}\n", Value::Object(o))]
            }
            ChatEvent::Reasoning(_) | ChatEvent::ToolCall { .. } => vec![],
            ChatEvent::Done { .. } => {
                let calls = (!self.out.tool_calls.is_empty()).then(|| tool_calls_json(&self.out.tool_calls));
                let mut o = self.line(now, "", calls);
                finals(&mut o, &self.out, stats);
                vec![format!("{}\n", Value::Object(o))]
            }
        }
    }
}

/// The whole (non-streamed) answer of `/api/chat` or `/api/generate`.
pub fn response(out: &ChatOutput, model: &str, generate: bool, now: Millis, stats: &Stats) -> Value {
    let s = Stream::new(model, generate);
    let calls = (!out.tool_calls.is_empty()).then(|| tool_calls_json(&out.tool_calls));
    let mut o = s.line(now, &out.text, calls);
    finals(&mut o, out, stats);
    Value::Object(o)
}

/// The answer to a load-only (or unload) request.
pub fn load_response(model: &str, generate: bool, now: Millis, unloaded: bool) -> Value {
    let s = Stream::new(model, generate);
    let mut o = s.line(now, "", None);
    o.insert("done".into(), json!(true));
    o.insert("done_reason".into(), json!(if unloaded { "unload" } else { "load" }));
    Value::Object(o)
}

/// Parses `POST /api/embed` (`input`: string or array) or the legacy `/api/embeddings` (`prompt`).
pub fn parse_embed(v: &Value) -> Result<(EmbedRequest, bool), ApiError> {
    let o = object(v, "the request")?;
    let model = match o.get("model").and_then(Value::as_str) {
        Some(m) if !m.trim().is_empty() => ModelRef::Named(m.trim().trim_end_matches(":latest").to_string()),
        _ => return Err(ApiError::invalid("`model` is required")),
    };
    let (input, legacy) = match (o.get("input"), o.get("prompt")) {
        (Some(Value::String(s)), _) => (vec![s.clone()], false),
        (Some(Value::Array(a)), _) => (
            a.iter()
                .map(|x| {
                    x.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| ApiError::invalid("`input` must hold strings"))
                })
                .collect::<Result<_, _>>()?,
            false,
        ),
        (None, Some(Value::String(p))) => (vec![p.clone()], true),
        _ => return Err(ApiError::invalid("`input` is required")),
    };
    let req = EmbedRequest { model, input };
    req.check()?;
    Ok((req, legacy))
}

/// The `/api/embed` answer (or `/api/embeddings` when `legacy`).
pub fn embed_response(out: &EmbedOutput, model: &str, legacy: bool, stats: &Stats) -> Value {
    if legacy {
        return json!({"embedding": out.vectors.first().cloned().unwrap_or_default()});
    }
    json!({
        "model": model,
        "embeddings": out.vectors,
        "total_duration": stats.total_ms.saturating_mul(1_000_000),
        "load_duration": stats.load_ms.saturating_mul(1_000_000),
        "prompt_eval_count": out.prompt_tokens
    })
}

fn details(m: &ModelEntry) -> Value {
    json!({
        "parent_model": "",
        "format": "gguf",
        "family": m.family.clone().unwrap_or_default(),
        "families": m.family.iter().collect::<Vec<_>>(),
        "parameter_size": m.parameter_size.clone().unwrap_or_default(),
        "quantization_level": m.quant.clone().unwrap_or_default()
    })
}

/// `GET /api/tags`.
pub fn tags(list: &[ModelEntry]) -> Value {
    json!({"models": list.iter().map(|m| json!({
        "name": m.id.as_str(),
        "model": m.id.as_str(),
        "modified_at": rfc3339(m.modified),
        "size": m.size,
        "digest": m.sha256.clone().unwrap_or_default(),
        "details": details(m)
    })).collect::<Vec<_>>()})
}

/// `GET /api/ps`: the loaded models.
pub fn ps(list: &[ModelEntry]) -> Value {
    json!({"models": list.iter().filter_map(|m| {
        let l = m.loaded?;
        Some(json!({
            "name": m.id.as_str(),
            "model": m.id.as_str(),
            "size": l.ram + l.vram,
            "size_vram": l.vram,
            "digest": m.sha256.clone().unwrap_or_default(),
            "details": details(m),
            "expires_at": l.expires_at.map_or_else(|| "0001-01-01T00:00:00Z".to_string(), rfc3339)
        }))
    }).collect::<Vec<_>>()})
}

/// `POST /api/show`.
pub fn show(m: &ModelEntry, capabilities: &[&str]) -> Value {
    json!({
        "details": details(m),
        "model_info": {"general.architecture": m.family.clone().unwrap_or_default()},
        "capabilities": capabilities,
        "modified_at": rfc3339(m.modified)
    })
}

/// `GET /api/version`.
pub fn version() -> Value {
    json!({"version": COMPAT_VERSION})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::LoadedEntry;
    use crate::chat::Usage;
    use crate::ids::ModelId;

    #[test]
    fn keep_alive_values() {
        assert_eq!(parse_keep_alive(&json!(null)).unwrap(), None);
        assert_eq!(parse_keep_alive(&json!(0)).unwrap(), Some(KeepAlive::Unload));
        assert_eq!(parse_keep_alive(&json!(-1)).unwrap(), Some(KeepAlive::Forever));
        assert_eq!(parse_keep_alive(&json!("-1")).unwrap(), Some(KeepAlive::Forever));
        assert_eq!(parse_keep_alive(&json!(30)).unwrap(), Some(KeepAlive::For(30_000)));
        assert_eq!(parse_keep_alive(&json!("5m")).unwrap(), Some(KeepAlive::For(300_000)));
        assert_eq!(parse_keep_alive(&json!("1h")).unwrap(), Some(KeepAlive::For(3_600_000)));
        assert_eq!(parse_keep_alive(&json!("10s")).unwrap(), Some(KeepAlive::For(10_000)));
        assert_eq!(parse_keep_alive(&json!("500ms")).unwrap(), Some(KeepAlive::For(500)));
        assert!(parse_keep_alive(&json!("soon")).is_err());
        assert!(parse_keep_alive(&json!([])).is_err());
    }

    #[test]
    fn chat_requests() {
        let v = json!({
            "model": "qwen3-8b:latest",
            "messages": [
                {"role": "system", "content": "s"},
                {"role": "user", "content": "look", "images": ["iVBORw0KGgo="]},
                {"role": "assistant", "content": "", "tool_calls": [{"function": {"name": "f", "arguments": {"a": 1}}}]},
                {"role": "tool", "content": "42", "tool_name": "f"}
            ],
            "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}}],
            "format": {"type": "object"},
            "options": {"temperature": 0.3, "num_predict": 50, "num_ctx": 4096, "top_k": 10, "top_p": 0.5, "seed": 2, "stop": ["x"]},
            "keep_alive": "10m"
        });
        let r = parse_chat(&v).unwrap();
        assert_eq!(r.chat.model, ModelRef::Named("qwen3-8b".into()));
        assert!(r.chat.stream, "Ollama streams by default");
        assert!(r.chat.has_images());
        assert_eq!(r.chat.messages[2].tool_calls[0].arguments, "{\"a\":1}");
        assert_eq!(r.chat.messages[3].tool_call_id.as_deref(), Some("f"));
        assert_eq!((r.chat.max_tokens, r.chat.context, r.chat.top_k), (Some(50), Some(4096), Some(10)));
        assert!(matches!(r.chat.format, OutputFormat::JsonSchema(_)));
        assert_eq!(r.keep_alive, Some(KeepAlive::For(600_000)));
        assert!(!r.load_only);
        let load = parse_chat(&json!({"model": "m", "keep_alive": 0, "format": "json", "options": {"num_predict": -1}})).unwrap();
        assert!(load.load_only);
        assert_eq!(load.keep_alive, Some(KeepAlive::Unload));
        assert_eq!(load.chat.format, OutputFormat::JsonObject);
        assert_eq!(
            parse_chat(&json!({"model": "m", "format": ""})).unwrap().chat.format,
            OutputFormat::Text
        );
        for (v, needle) in [
            (json!({"messages": []}), "`model` is required"),
            (json!({"model": "m", "messages": [{"role": "robot"}]}), "role"),
            (json!({"model": "m", "messages": [5]}), "a message must be"),
            (
                json!({"model": "m", "messages": [{"role": "user", "content": "x", "images": ["AAAA"]}]}),
                "not PNG",
            ),
            (
                json!({"model": "m", "messages": [{"role": "user", "images": [5]}]}),
                "base64 strings",
            ),
            (json!({"model": "m", "format": 5}), "`format`"),
            (json!({"model": "m", "tools": [{}]}), "no `name`"),
            (json!({"model": "m", "options": 3}), "`options`"),
        ] {
            let e = parse_chat(&v).unwrap_err().to_string();
            assert!(e.contains(needle), "{v}: {e}");
        }
    }

    #[test]
    fn generate_requests() {
        let r = parse_generate(&json!({"model": "m", "prompt": "hi", "system": "s", "stream": false, "images": ["/9j/4AAQ"]})).unwrap();
        assert_eq!(r.chat.messages.len(), 2);
        assert!(!r.chat.stream && r.chat.has_images());
        let load = parse_generate(&json!({"model": "m"})).unwrap();
        assert!(load.load_only && load.chat.messages.is_empty());
        assert!(parse_generate(&json!({"model": "m", "prompt": "a", "suffix": "b"}))
            .unwrap_err()
            .to_string()
            .contains("suffix"));
        assert!(parse_generate(&json!({"model": "m", "prompt": 3})).is_err());
    }

    #[test]
    fn streams_and_answers() {
        let st = Stats {
            total_ms: 10,
            load_ms: 2,
            prompt_ms: 3,
            eval_ms: 5,
        };
        let mut s = Stream::new("m", false);
        let l = s.event(&ChatEvent::Delta("He".into()), 0, &st);
        let v: Value = serde_json::from_str(l[0].trim()).unwrap();
        assert_eq!((v["message"]["content"].as_str(), v["done"].as_bool()), (Some("He"), Some(false)));
        assert_eq!(v["created_at"], "1970-01-01T00:00:00.000Z");
        assert!(s
            .event(
                &ChatEvent::ToolCall {
                    index: 0,
                    id: None,
                    name: Some("f".into()),
                    arguments: "{\"a\":2}".into()
                },
                0,
                &st
            )
            .is_empty());
        assert!(s.event(&ChatEvent::Reasoning("r".into()), 0, &st).is_empty());
        let l = s.event(
            &ChatEvent::Done {
                finish: Finish::Stop,
                usage: Usage {
                    prompt_tokens: 4,
                    completion_tokens: 6,
                },
                timings: None,
            },
            0,
            &st,
        );
        let v: Value = serde_json::from_str(l[0].trim()).unwrap();
        assert_eq!(v["done"], true);
        assert_eq!(v["message"]["tool_calls"][0]["function"]["arguments"]["a"], 2);
        assert_eq!(
            (v["eval_count"].as_u64(), v["total_duration"].as_u64(), v["load_duration"].as_u64()),
            (Some(6), Some(10_000_000), Some(2_000_000))
        );
        let mut g = Stream::new("m", true);
        let l = g.event(&ChatEvent::Delta("x".into()), 0, &st);
        assert!(l[0].contains("\"response\":\"x\""));
        let out = ChatOutput {
            text: "all".into(),
            finish: Some(Finish::Length),
            ..Default::default()
        };
        let r = response(&out, "m", true, 0, &st);
        assert_eq!((r["response"].as_str(), r["done_reason"].as_str()), (Some("all"), Some("length")));
        let calls = ChatOutput {
            tool_calls: vec![ToolCall {
                id: "1".into(),
                name: "f".into(),
                arguments: "bad".into(),
            }],
            ..Default::default()
        };
        assert_eq!(
            response(&calls, "m", false, 0, &st)["message"]["tool_calls"][0]["function"]["arguments"],
            json!({})
        );
        assert_eq!(load_response("m", false, 0, true)["done_reason"], "unload");
        assert_eq!(load_response("m", true, 0, false)["done_reason"], "load");
    }

    #[test]
    fn embeddings_and_listings() {
        let (r, legacy) = parse_embed(&json!({"model": "e", "input": ["a", "b"]})).unwrap();
        assert_eq!((r.input.len(), legacy), (2, false));
        let (r, legacy) = parse_embed(&json!({"model": "e", "prompt": "a"})).unwrap();
        assert_eq!((r.input.len(), legacy), (1, true));
        assert_eq!(parse_embed(&json!({"model": "e", "input": "x"})).unwrap().0.input, vec!["x"]);
        assert!(parse_embed(&json!({"input": "x"})).is_err());
        assert!(parse_embed(&json!({"model": "e"})).is_err());
        assert!(parse_embed(&json!({"model": "e", "input": [1]})).is_err());
        let out = EmbedOutput {
            vectors: vec![vec![0.5]],
            prompt_tokens: 2,
        };
        assert_eq!(embed_response(&out, "e", false, &Stats::default())["embeddings"][0][0], 0.5);
        assert_eq!(embed_response(&out, "e", true, &Stats::default())["embedding"][0], 0.5);
        assert_eq!(
            embed_response(&EmbedOutput::default(), "e", true, &Stats::default())["embedding"],
            json!([])
        );
        let m = ModelEntry {
            id: ModelId::parse("qwen3-8b").unwrap(),
            size: 5,
            family: Some("qwen3".into()),
            parameter_size: Some("8B".into()),
            quant: Some("Q4_K_M".into()),
            modified: 0,
            sha256: Some("abc".into()),
            loaded: Some(LoadedEntry {
                ram: 1,
                vram: 2,
                expires_at: None,
            }),
        };
        let unloaded = ModelEntry {
            loaded: None,
            family: None,
            ..m.clone()
        };
        let t = tags(&[m.clone(), unloaded.clone()]);
        assert_eq!(t["models"][0]["details"]["quantization_level"], "Q4_K_M");
        assert_eq!(t["models"][1]["details"]["families"], json!([]));
        let p = ps(&[m.clone(), unloaded]);
        assert_eq!(p["models"].as_array().unwrap().len(), 1);
        assert_eq!(p["models"][0]["size"], 3);
        assert_eq!(p["models"][0]["expires_at"], "0001-01-01T00:00:00Z");
        let mut soon = m.clone();
        soon.loaded = Some(LoadedEntry {
            ram: 1,
            vram: 0,
            expires_at: Some(1000),
        });
        assert_eq!(ps(&[soon])["models"][0]["expires_at"], "1970-01-01T00:00:01.000Z");
        assert_eq!(show(&m, &["completion", "vision"])["capabilities"][1], "vision");
        assert_eq!(version()["version"], COMPAT_VERSION);
    }
}
