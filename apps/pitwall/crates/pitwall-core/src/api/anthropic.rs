//! Anthropic Messages-compatible API (`POST /v1/messages`): the request and response objects and the streaming event
//! sequence (`message_start`, `content_block_start`, `content_block_delta`, `content_block_stop`, `message_delta`,
//! `message_stop`) the official SDKs read.

use super::{model_ref, object, opt, opt_f32, opt_u32, stops, ApiError, ErrorKind};
use crate::chat::{ChatEvent, ChatOutput, ChatRequest, Finish, Image, ImageMime, Message, Part, Role, Tool, ToolCall, ToolChoice, Usage};
use serde_json::{json, Value};

fn blocks(v: &Value, role: Role) -> Result<Vec<Message>, ApiError> {
    let items = match v {
        Value::String(s) => return Ok(vec![Message::text(role, s.clone())]),
        Value::Array(a) => a,
        _ => return Err(ApiError::invalid("`content` must be a string or an array of content blocks")),
    };
    let mut main = Message { role, parts: vec![], tool_calls: vec![], tool_call_id: None };
    let mut results = vec![];
    for b in items {
        match b.get("type").and_then(Value::as_str).unwrap_or_default() {
            "text" => main.parts.push(Part::Text(b.get("text").and_then(Value::as_str).unwrap_or_default().to_string())),
            "image" => {
                let src = b.get("source").ok_or_else(|| ApiError::invalid("an image block has no `source`"))?;
                if src.get("type").and_then(Value::as_str) != Some("base64") {
                    return Err(ApiError::invalid("images must be base64 sources (Pitwall never fetches remote content)"));
                }
                let mime = src
                    .get("media_type")
                    .and_then(Value::as_str)
                    .and_then(ImageMime::parse)
                    .ok_or_else(|| ApiError::invalid("an image's media_type must be image/png, image/jpeg, image/webp or image/gif"))?;
                main.parts.push(Part::Image(Image::from_base64(mime, src.get("data").and_then(Value::as_str).unwrap_or_default())?));
            }
            "tool_use" => main.tool_calls.push(ToolCall {
                id: b.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                name: b.get("name").and_then(Value::as_str).ok_or_else(|| ApiError::invalid("a tool_use block has no `name`"))?.to_string(),
                arguments: b.get("input").map_or_else(|| "{}".to_string(), Value::to_string),
            }),
            "tool_result" => {
                let text = match b.get("content") {
                    Some(Value::String(s)) => s.clone(),
                    Some(Value::Array(parts)) => parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""),
                    _ => String::new(),
                };
                results.push(Message {
                    role: Role::Tool,
                    parts: vec![Part::Text(text)],
                    tool_calls: vec![],
                    tool_call_id: b.get("tool_use_id").and_then(Value::as_str).map(str::to_string),
                });
            }
            // Thinking blocks of earlier turns carry no instruction for a local model.
            "thinking" | "redacted_thinking" => {}
            other => return Err(ApiError::new(ErrorKind::NotAvailable, format!("content blocks of type {other:?} are not supported"))),
        }
    }
    let mut out = results;
    if !main.parts.is_empty() || !main.tool_calls.is_empty() {
        out.push(main);
    }
    Ok(out)
}

/// Parses a `POST /v1/messages` body.
pub fn parse_messages(v: &Value) -> Result<ChatRequest, ApiError> {
    let o = object(v, "the request")?;
    let mut req = ChatRequest { model: model_ref(o)?, ..Default::default() };
    match o.get("system") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) => req.messages.push(Message::text(Role::System, s.clone())),
        Some(Value::Array(a)) => {
            let text: Vec<&str> = a.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect();
            req.messages.push(Message::text(Role::System, text.join("\n")));
        }
        Some(_) => return Err(ApiError::invalid("`system` must be a string or an array of text blocks")),
    }
    let msgs = opt(o, "messages", Value::as_array, "an array")?.ok_or_else(|| ApiError::invalid("`messages` is required"))?;
    for m in msgs {
        let role = match m.get("role").and_then(Value::as_str) {
            Some("user") => Role::User,
            Some("assistant") => Role::Assistant,
            _ => return Err(ApiError::invalid("each message's `role` must be \"user\" or \"assistant\"")),
        };
        req.messages.extend(blocks(m.get("content").unwrap_or(&Value::Null), role)?);
    }
    req.max_tokens = Some(opt_u32(o, "max_tokens")?.ok_or_else(|| ApiError::invalid("`max_tokens` is required"))?);
    req.temperature = opt_f32(o, "temperature")?;
    req.top_p = opt_f32(o, "top_p")?;
    req.top_k = opt_u32(o, "top_k")?;
    req.stop = stops(o, "stop_sequences")?;
    req.stream = opt(o, "stream", Value::as_bool, "a boolean")?.unwrap_or(false);
    if let Some(list) = opt(o, "tools", Value::as_array, "an array")? {
        for t in list {
            let name = t.get("name").and_then(Value::as_str).ok_or_else(|| ApiError::invalid("a tool has no `name`"))?;
            if t.get("type").and_then(Value::as_str).is_some_and(|ty| ty != "custom") {
                return Err(ApiError::new(ErrorKind::NotAvailable, format!("server tools such as {name:?} are not available in Pitwall")));
            }
            req.tools.push(Tool {
                name: name.to_string(),
                description: t.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
                parameters: t.get("input_schema").cloned().unwrap_or_else(|| json!({"type": "object"})),
            });
        }
    }
    req.tool_choice = match o.get("tool_choice") {
        None | Some(Value::Null) => ToolChoice::Auto,
        Some(c) => match c.get("type").and_then(Value::as_str) {
            Some("auto") => ToolChoice::Auto,
            Some("any") => ToolChoice::Required,
            Some("none") => ToolChoice::None,
            Some("tool") => ToolChoice::Named(c.get("name").and_then(Value::as_str).ok_or_else(|| ApiError::invalid("tool_choice names no tool"))?.to_string()),
            _ => return Err(ApiError::invalid("unknown tool_choice")),
        },
    };
    req.check()?;
    Ok(req)
}

fn stop_reason(f: Option<Finish>) -> &'static str {
    match f {
        Some(Finish::Length) => "max_tokens",
        Some(Finish::ToolCalls) => "tool_use",
        _ => "end_turn",
    }
}

fn input_of(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| json!({}))
}

/// The `message` object.
pub fn messages_response(out: &ChatOutput, id: &str, model: &str) -> Value {
    let mut content = vec![];
    if !out.text.is_empty() {
        content.push(json!({"type": "text", "text": out.text}));
    }
    for c in &out.tool_calls {
        content.push(json!({"type": "tool_use", "id": c.id, "name": c.name, "input": input_of(&c.arguments)}));
    }
    json!({
        "id": id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason(out.finish),
        "stop_sequence": null,
        "usage": {"input_tokens": out.usage.prompt_tokens, "output_tokens": out.usage.completion_tokens}
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Text,
    Tool(u32),
}

/// Writes the Messages streaming events for one answer.
#[derive(Debug)]
pub struct MessagesStream {
    id: String,
    model: String,
    started: bool,
    open: Option<Block>,
    index: u32,
    prompt_tokens: u32,
}

impl MessagesStream {
    /// A stream; `prompt_tokens` is reported in `message_start` when known.
    pub fn new(id: &str, model: &str, prompt_tokens: u32) -> MessagesStream {
        MessagesStream { id: id.into(), model: model.into(), started: false, open: None, index: 0, prompt_tokens }
    }

    fn close(&mut self, out: &mut Vec<String>) {
        if self.open.take().is_some() {
            out.push(crate::sse::named("content_block_stop", &json!({"type": "content_block_stop", "index": self.index}).to_string()));
            self.index += 1;
        }
    }

    fn open(&mut self, block: Block, start: Value, out: &mut Vec<String>) {
        if self.open == Some(block) {
            return;
        }
        self.close(out);
        self.open = Some(block);
        out.push(crate::sse::named(
            "content_block_start",
            &json!({"type": "content_block_start", "index": self.index, "content_block": start}).to_string(),
        ));
    }

    /// The SSE frames for one event.
    pub fn event(&mut self, e: &ChatEvent) -> Vec<String> {
        let mut out = vec![];
        if !self.started {
            self.started = true;
            let msg = json!({
                "id": self.id, "type": "message", "role": "assistant", "model": self.model, "content": [],
                "stop_reason": null, "stop_sequence": null,
                "usage": {"input_tokens": self.prompt_tokens, "output_tokens": 0}
            });
            out.push(crate::sse::named("message_start", &json!({"type": "message_start", "message": msg}).to_string()));
        }
        match e {
            ChatEvent::Delta(t) => {
                self.open(Block::Text, json!({"type": "text", "text": ""}), &mut out);
                let d = json!({"type": "content_block_delta", "index": self.index, "delta": {"type": "text_delta", "text": t}});
                out.push(crate::sse::named("content_block_delta", &d.to_string()));
            }
            ChatEvent::Reasoning(_) => {}
            ChatEvent::ToolCall { index, id, name, arguments } => {
                let start = json!({
                    "type": "tool_use",
                    "id": id.clone().unwrap_or_else(|| format!("toolu_{index}")),
                    "name": name.clone().unwrap_or_default(),
                    "input": {}
                });
                self.open(Block::Tool(*index), start, &mut out);
                if !arguments.is_empty() {
                    let d = json!({"type": "content_block_delta", "index": self.index, "delta": {"type": "input_json_delta", "partial_json": arguments}});
                    out.push(crate::sse::named("content_block_delta", &d.to_string()));
                }
            }
            ChatEvent::Done { finish, usage, .. } => {
                self.close(&mut out);
                let Usage { completion_tokens, .. } = *usage;
                let d = json!({
                    "type": "message_delta",
                    "delta": {"stop_reason": stop_reason(Some(*finish)), "stop_sequence": null},
                    "usage": {"output_tokens": completion_tokens}
                });
                out.push(crate::sse::named("message_delta", &d.to_string()));
                out.push(crate::sse::named("message_stop", &json!({"type": "message_stop"}).to_string()));
            }
        }
        out
    }
}

/// `POST /v1/messages/count_tokens` answer.
pub fn count_tokens_response(input_tokens: u32) -> Value {
    json!({"input_tokens": input_tokens})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_sdk_request() {
        let v = json!({
            "model": "qwen3-8b",
            "max_tokens": 256,
            "system": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}],
            "messages": [
                {"role": "user", "content": [
                    {"type": "text", "text": "look"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo="}}
                ]},
                {"role": "assistant", "content": [{"type": "thinking", "thinking": "x"}, {"type": "text", "text": "calling"}, {"type": "tool_use", "id": "toolu_1", "name": "f", "input": {"a": 1}}]},
                {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": [{"type": "text", "text": "42"}]}, {"type": "text", "text": "and?"}]}
            ],
            "tools": [{"name": "f", "description": "d", "input_schema": {"type": "object"}}],
            "tool_choice": {"type": "tool", "name": "f"},
            "stop_sequences": ["END"],
            "temperature": 0.1,
            "top_k": 20,
            "stream": true
        });
        let r = parse_messages(&v).unwrap();
        assert_eq!(r.messages[0].text_content(), "a\nb");
        assert!(r.messages[1].images().next().is_some());
        assert_eq!(r.messages[2].tool_calls[0].arguments, "{\"a\":1}");
        assert_eq!(r.messages[3].role, Role::Tool);
        assert_eq!(r.messages[3].text_content(), "42");
        assert_eq!(r.messages[4].text_content(), "and?");
        assert_eq!(r.tool_choice, ToolChoice::Named("f".into()));
        assert_eq!((r.max_tokens, r.top_k, r.stream), (Some(256), Some(20), true));
        assert_eq!(r.stop, vec!["END"]);
    }

    #[test]
    fn refuses_bad_requests() {
        let base = |extra: Value| {
            let mut v = json!({"model": "m", "max_tokens": 10, "messages": [{"role": "user", "content": "hi"}]});
            for (k, x) in extra.as_object().unwrap() {
                v[k] = x.clone();
            }
            v
        };
        let cases = [
            (json!({"messages": [{"role": "user", "content": "hi"}]}), "`max_tokens` is required"),
            (json!({"max_tokens": 1}), "`messages` is required"),
            (base(json!({"system": 5})), "`system` must be"),
            (base(json!({"messages": [{"role": "system", "content": "x"}]})), "role"),
            (base(json!({"messages": [{"role": "user", "content": 5}]})), "string or an array"),
            (base(json!({"messages": [{"role": "user", "content": [{"type": "image"}]}]})), "no `source`"),
            (base(json!({"messages": [{"role": "user", "content": [{"type": "image", "source": {"type": "url", "url": "https://x"}}]}]})), "never fetches"),
            (base(json!({"messages": [{"role": "user", "content": [{"type": "image", "source": {"type": "base64", "media_type": "image/bmp", "data": "AAAA"}}]}]})), "media_type"),
            (base(json!({"messages": [{"role": "assistant", "content": [{"type": "tool_use"}]}]})), "no `name`"),
            (base(json!({"messages": [{"role": "user", "content": [{"type": "document"}]}]})), "not supported"),
            (base(json!({"tools": [{"description": "x"}]})), "no `name`"),
            (base(json!({"tools": [{"type": "web_search_20250305", "name": "web_search"}]})), "server tools"),
            (base(json!({"tool_choice": {"type": "tool"}})), "names no tool"),
            (base(json!({"tool_choice": {"type": "sometimes"}})), "unknown tool_choice"),
        ];
        for (v, needle) in cases {
            let e = parse_messages(&v).unwrap_err().to_string();
            assert!(e.contains(needle), "{v}: {e}");
        }
        for (c, want) in [("auto", ToolChoice::Auto), ("any", ToolChoice::Required), ("none", ToolChoice::None)] {
            let r = parse_messages(&base(json!({"tool_choice": {"type": c}, "system": null}))).unwrap();
            assert_eq!(r.tool_choice, want);
        }
        let r = parse_messages(&base(json!({"system": "s", "tools": [{"name": "g", "type": "custom"}], "messages": [{"role": "user", "content": [{"type": "tool_result", "tool_use_id": "x", "content": "plain"}, {"type": "tool_result"}]}]})))
            .unwrap();
        assert_eq!(r.messages[0].text_content(), "s");
        assert_eq!(r.messages[1].text_content(), "plain");
        assert_eq!(r.messages[2].text_content(), "");
        assert_eq!(r.tools[0].parameters["type"], "object");
    }

    #[test]
    fn responses_and_streams() {
        let out = ChatOutput {
            text: "hi".into(),
            tool_calls: vec![ToolCall { id: "toolu_1".into(), name: "f".into(), arguments: "{\"a\":1}".into() }, ToolCall { id: "t2".into(), name: "g".into(), arguments: "not json".into() }],
            finish: Some(Finish::ToolCalls),
            usage: Usage { prompt_tokens: 5, completion_tokens: 2 },
            ..Default::default()
        };
        let r = messages_response(&out, "msg_1", "m");
        assert_eq!(r["type"], "message");
        assert_eq!(r["content"][0]["type"], "text");
        assert_eq!(r["content"][1]["input"]["a"], 1);
        assert_eq!(r["content"][2]["input"], json!({}));
        assert_eq!(r["stop_reason"], "tool_use");
        assert_eq!(r["usage"]["input_tokens"], 5);
        assert_eq!(messages_response(&ChatOutput { finish: Some(Finish::Length), ..Default::default() }, "x", "m")["stop_reason"], "max_tokens");
        assert_eq!(messages_response(&ChatOutput::default(), "x", "m")["stop_reason"], "end_turn");

        let mut s = MessagesStream::new("msg_2", "m", 7);
        let f = s.event(&ChatEvent::Delta("He".into()));
        assert!(f[0].starts_with("event: message_start") && f[0].contains("\"input_tokens\":7"));
        assert!(f[1].starts_with("event: content_block_start") && f[2].contains("text_delta"));
        let f = s.event(&ChatEvent::Delta("y".into()));
        assert_eq!(f.len(), 1, "same block");
        assert!(s.event(&ChatEvent::Reasoning("r".into())).is_empty());
        let f = s.event(&ChatEvent::ToolCall { index: 0, id: Some("toolu_9".into()), name: Some("f".into()), arguments: "{\"a\"".into() });
        assert!(f[0].contains("content_block_stop") && f[1].contains("\"type\":\"tool_use\"") && f[1].contains("\"index\":1"));
        assert!(f[2].contains("input_json_delta"));
        let f = s.event(&ChatEvent::ToolCall { index: 1, id: None, name: None, arguments: String::new() });
        assert!(f[1].contains("toolu_1"), "generated id");
        let f = s.event(&ChatEvent::Done { finish: Finish::ToolCalls, usage: Usage { prompt_tokens: 7, completion_tokens: 4 }, timings: None });
        assert!(f[0].contains("content_block_stop"));
        assert!(f[1].contains("\"stop_reason\":\"tool_use\"") && f[1].contains("\"output_tokens\":4"));
        assert!(f[2].starts_with("event: message_stop"));
        assert_eq!(count_tokens_response(3)["input_tokens"], 3);
    }
}
