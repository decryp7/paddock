//! OpenAI-compatible API: Chat Completions, Responses, Embeddings and Models (request and response objects as the
//! official SDKs send and read them). Audio transcription is routed by the server and answered "not available"
//! until a whisper.cpp engine is installed.

use super::{model_ref, object, opt, opt_f32, opt_u32, stops, ApiError, ModelEntry};
use crate::chat::{
    ChatEvent, ChatOutput, ChatRequest, EmbedOutput, EmbedRequest, Finish, Image, Message, ModelRef, OutputFormat, Part, Role, Tool, ToolCall,
    ToolChoice,
};
use serde_json::{json, Map, Value};

fn role(s: &str) -> Result<Role, ApiError> {
    match s {
        "system" | "developer" => Ok(Role::System),
        "user" => Ok(Role::User),
        "assistant" => Ok(Role::Assistant),
        "tool" | "function" => Ok(Role::Tool),
        other => Err(ApiError::invalid(format!("unknown message role {other:?}"))),
    }
}

/// Message content: a string or an array of parts (`text` / `input_text` / `output_text`, `image_url` /
/// `input_image`). Remote image URLs are refused.
fn parts(v: &Value) -> Result<Vec<Part>, ApiError> {
    match v {
        Value::Null => Ok(vec![]),
        Value::String(s) => Ok(vec![Part::Text(s.clone())]),
        Value::Array(items) => items
            .iter()
            .map(|p| {
                let t = p.get("type").and_then(Value::as_str).unwrap_or("text");
                match t {
                    "text" | "input_text" | "output_text" => {
                        Ok(Part::Text(p.get("text").and_then(Value::as_str).ok_or_else(|| ApiError::invalid("a text part has no `text`"))?.to_string()))
                    }
                    "image_url" | "input_image" => {
                        let url = p
                            .get("image_url")
                            .and_then(|u| u.as_str().or_else(|| u.get("url").and_then(Value::as_str)))
                            .ok_or_else(|| ApiError::invalid("an image part has no `image_url`"))?;
                        Ok(Part::Image(Image::from_data_url(url)?))
                    }
                    other => Err(ApiError::new(super::ErrorKind::NotAvailable, format!("content parts of type {other:?} are not supported"))),
                }
            })
            .collect(),
        _ => Err(ApiError::invalid("`content` must be a string or an array of parts")),
    }
}

fn tools(o: &Map<String, Value>) -> Result<Vec<Tool>, ApiError> {
    let Some(list) = opt(o, "tools", Value::as_array, "an array")? else { return Ok(vec![]) };
    list.iter()
        .map(|t| {
            // Chat Completions nests the function; Responses puts name and parameters at the top.
            let f = t.get("function").unwrap_or(t);
            let name = f.get("name").and_then(Value::as_str).ok_or_else(|| ApiError::invalid("a tool has no `name`"))?;
            Ok(Tool {
                name: name.to_string(),
                description: f.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
                parameters: f.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}})),
            })
        })
        .collect()
}

fn tool_choice(o: &Map<String, Value>) -> Result<ToolChoice, ApiError> {
    match o.get("tool_choice") {
        None | Some(Value::Null) => Ok(ToolChoice::Auto),
        Some(Value::String(s)) => match s.as_str() {
            "auto" => Ok(ToolChoice::Auto),
            "none" => Ok(ToolChoice::None),
            "required" => Ok(ToolChoice::Required),
            other => Err(ApiError::invalid(format!("unknown tool_choice {other:?}"))),
        },
        Some(v) => v
            .pointer("/function/name")
            .or_else(|| v.get("name"))
            .and_then(Value::as_str)
            .map(|n| ToolChoice::Named(n.to_string()))
            .ok_or_else(|| ApiError::invalid("`tool_choice` names no function")),
    }
}

fn format(v: Option<&Value>) -> Result<OutputFormat, ApiError> {
    let Some(f) = v.filter(|v| !v.is_null()) else { return Ok(OutputFormat::Text) };
    match f.get("type").and_then(Value::as_str) {
        Some("text") => Ok(OutputFormat::Text),
        Some("json_object") => Ok(OutputFormat::JsonObject),
        Some("json_schema") => {
            // Chat Completions: {json_schema: {schema}}; Responses: {schema} directly.
            let schema = f.pointer("/json_schema/schema").or_else(|| f.get("schema")).cloned();
            schema.map(OutputFormat::JsonSchema).ok_or_else(|| ApiError::invalid("a json_schema format needs a `schema`"))
        }
        _ => Err(ApiError::invalid("unknown response format")),
    }
}

fn sampling(o: &Map<String, Value>, req: &mut ChatRequest) -> Result<(), ApiError> {
    req.temperature = opt_f32(o, "temperature")?;
    req.top_p = opt_f32(o, "top_p")?;
    req.seed = opt(o, "seed", Value::as_i64, "an integer")?;
    req.stream = opt(o, "stream", Value::as_bool, "a boolean")?.unwrap_or(false);
    req.tools = tools(o)?;
    req.tool_choice = tool_choice(o)?;
    req.model = model_ref(o)?;
    Ok(())
}

/// Parses a `POST /v1/chat/completions` body.
pub fn parse_chat(v: &Value) -> Result<ChatRequest, ApiError> {
    let o = object(v, "the request")?;
    let msgs = opt(o, "messages", Value::as_array, "an array")?.ok_or_else(|| ApiError::invalid("`messages` is required"))?;
    let mut req = ChatRequest::default();
    for m in msgs {
        let r = role(m.get("role").and_then(Value::as_str).ok_or_else(|| ApiError::invalid("a message has no `role`"))?)?;
        let calls = m
            .get("tool_calls")
            .and_then(Value::as_array)
            .map(|cs| {
                cs.iter()
                    .map(|c| {
                        let f = c.get("function").unwrap_or(&Value::Null);
                        Ok(ToolCall {
                            id: c.get("id").and_then(Value::as_str).unwrap_or_default().to_string(),
                            name: f.get("name").and_then(Value::as_str).ok_or_else(|| ApiError::invalid("a tool call has no name"))?.to_string(),
                            arguments: match f.get("arguments") {
                                Some(Value::String(s)) => s.clone(),
                                Some(other) => other.to_string(),
                                None => "{}".into(),
                            },
                        })
                    })
                    .collect::<Result<Vec<_>, ApiError>>()
            })
            .transpose()?
            .unwrap_or_default();
        req.messages.push(Message {
            role: r,
            parts: parts(m.get("content").unwrap_or(&Value::Null))?,
            tool_calls: calls,
            tool_call_id: m.get("tool_call_id").and_then(Value::as_str).map(str::to_string),
        });
    }
    sampling(o, &mut req)?;
    req.max_tokens = opt_u32(o, "max_completion_tokens")?.or(opt_u32(o, "max_tokens")?);
    req.stop = stops(o, "stop")?;
    req.format = format(o.get("response_format"))?;
    req.check()?;
    Ok(req)
}

fn finish_reason(f: Option<Finish>) -> &'static str {
    match f {
        Some(Finish::Length) => "length",
        Some(Finish::ToolCalls) => "tool_calls",
        _ => "stop",
    }
}

fn tool_calls_json(calls: &[ToolCall]) -> Value {
    calls.iter().map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments}})).collect()
}

/// The `chat.completion` object.
pub fn chat_response(out: &ChatOutput, id: &str, model: &str, created_secs: u64) -> Value {
    let mut message = json!({"role": "assistant", "content": out.text});
    if !out.reasoning.is_empty() {
        message["reasoning_content"] = json!(out.reasoning);
    }
    if !out.tool_calls.is_empty() {
        message["tool_calls"] = tool_calls_json(&out.tool_calls);
        if out.text.is_empty() {
            message["content"] = Value::Null;
        }
    }
    json!({
        "id": id,
        "object": "chat.completion",
        "created": created_secs,
        "model": model,
        "system_fingerprint": "pitwall",
        "choices": [{"index": 0, "message": message, "finish_reason": finish_reason(out.finish), "logprobs": null}],
        "usage": {
            "prompt_tokens": out.usage.prompt_tokens,
            "completion_tokens": out.usage.completion_tokens,
            "total_tokens": out.usage.prompt_tokens + out.usage.completion_tokens
        }
    })
}

/// Writes `chat.completion.chunk` events.
#[derive(Debug)]
pub struct ChatStream {
    id: String,
    model: String,
    created: u64,
    started: bool,
}

impl ChatStream {
    /// A stream for one answer.
    pub fn new(id: &str, model: &str, created_secs: u64) -> ChatStream {
        ChatStream { id: id.into(), model: model.into(), created: created_secs, started: false }
    }

    fn chunk(&self, delta: Value, finish: Option<&str>, usage: Option<Value>) -> String {
        let mut c = json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "system_fingerprint": "pitwall",
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish, "logprobs": null}],
        });
        if let Some(u) = usage {
            c["usage"] = u;
        }
        crate::sse::data(&c.to_string())
    }

    /// The SSE frames for one event (the first carries the role).
    pub fn event(&mut self, e: &ChatEvent) -> Vec<String> {
        let mut out = vec![];
        if !self.started {
            self.started = true;
            out.push(self.chunk(json!({"role": "assistant", "content": ""}), None, None));
        }
        match e {
            ChatEvent::Delta(t) => out.push(self.chunk(json!({"content": t}), None, None)),
            ChatEvent::Reasoning(t) => out.push(self.chunk(json!({"reasoning_content": t}), None, None)),
            ChatEvent::ToolCall { index, id, name, arguments } => {
                let mut call = json!({"index": index, "function": {"arguments": arguments}});
                if let Some(id) = id {
                    call["id"] = json!(id);
                    call["type"] = json!("function");
                }
                if let Some(n) = name {
                    call["function"]["name"] = json!(n);
                }
                out.push(self.chunk(json!({"tool_calls": [call]}), None, None));
            }
            ChatEvent::Done { finish, usage, .. } => {
                let u = json!({
                    "prompt_tokens": usage.prompt_tokens,
                    "completion_tokens": usage.completion_tokens,
                    "total_tokens": usage.prompt_tokens + usage.completion_tokens
                });
                out.push(self.chunk(json!({}), Some(finish_reason(Some(*finish))), Some(u)));
                out.push(crate::sse::data("[DONE]"));
            }
        }
        out
    }
}

/// Parses a `POST /v1/responses` body (`input` as a string or items, `instructions`, function tools, `text.format`).
pub fn parse_responses(v: &Value) -> Result<ChatRequest, ApiError> {
    let o = object(v, "the request")?;
    let mut req = ChatRequest::default();
    if let Some(i) = opt(o, "instructions", Value::as_str, "a string")? {
        req.messages.push(Message::text(Role::System, i));
    }
    match o.get("input") {
        Some(Value::String(s)) => req.messages.push(Message::text(Role::User, s.clone())),
        Some(Value::Array(items)) => {
            for it in items {
                match it.get("type").and_then(Value::as_str).unwrap_or("message") {
                    "message" => {
                        let r = role(it.get("role").and_then(Value::as_str).unwrap_or("user"))?;
                        req.messages.push(Message { role: r, parts: parts(it.get("content").unwrap_or(&Value::Null))?, tool_calls: vec![], tool_call_id: None });
                    }
                    "function_call" => req.messages.push(Message {
                        role: Role::Assistant,
                        parts: vec![],
                        tool_calls: vec![ToolCall {
                            id: it.get("call_id").and_then(Value::as_str).unwrap_or_default().to_string(),
                            name: it.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
                            arguments: it.get("arguments").and_then(Value::as_str).unwrap_or("{}").to_string(),
                        }],
                        tool_call_id: None,
                    }),
                    "function_call_output" => req.messages.push(Message {
                        role: Role::Tool,
                        parts: vec![Part::Text(match it.get("output") {
                            Some(Value::String(s)) => s.clone(),
                            Some(other) => other.to_string(),
                            None => String::new(),
                        })],
                        tool_calls: vec![],
                        tool_call_id: it.get("call_id").and_then(Value::as_str).map(str::to_string),
                    }),
                    other => return Err(ApiError::new(super::ErrorKind::NotAvailable, format!("input items of type {other:?} are not supported"))),
                }
            }
        }
        _ => return Err(ApiError::invalid("`input` is required (a string or an array of items)")),
    }
    sampling(o, &mut req)?;
    req.max_tokens = opt_u32(o, "max_output_tokens")?;
    req.format = format(o.get("text").and_then(|t| t.get("format")))?;
    if o.get("previous_response_id").is_some_and(|v| !v.is_null()) {
        return Err(ApiError::new(super::ErrorKind::NotAvailable, "Pitwall does not store responses: send the whole conversation in `input`"));
    }
    req.check()?;
    Ok(req)
}

fn response_output(out: &ChatOutput, id: &str) -> Vec<Value> {
    let mut items = vec![];
    if !out.text.is_empty() || out.tool_calls.is_empty() {
        items.push(json!({
            "type": "message", "id": format!("msg_{id}"), "status": "completed", "role": "assistant",
            "content": [{"type": "output_text", "text": out.text, "annotations": []}]
        }));
    }
    for c in &out.tool_calls {
        items.push(json!({"type": "function_call", "id": format!("fc_{}", c.id), "call_id": c.id, "name": c.name, "arguments": c.arguments, "status": "completed"}));
    }
    items
}

/// The `response` object.
pub fn responses_response(out: &ChatOutput, id: &str, model: &str, created_secs: u64) -> Value {
    let incomplete = out.finish == Some(Finish::Length);
    json!({
        "id": id,
        "object": "response",
        "created_at": created_secs,
        "status": if incomplete { "incomplete" } else { "completed" },
        "incomplete_details": if incomplete { json!({"reason": "max_output_tokens"}) } else { Value::Null },
        "model": model,
        "output": response_output(out, id),
        "output_text": out.text,
        "usage": {
            "input_tokens": out.usage.prompt_tokens,
            "output_tokens": out.usage.completion_tokens,
            "total_tokens": out.usage.prompt_tokens + out.usage.completion_tokens
        }
    })
}

/// Writes Responses streaming events (`response.created`, `response.output_text.delta`, …, `response.completed`).
#[derive(Debug)]
pub struct ResponsesStream {
    id: String,
    model: String,
    created: u64,
    seq: u64,
    out: ChatOutput,
    started: bool,
}

impl ResponsesStream {
    /// A stream for one answer.
    pub fn new(id: &str, model: &str, created_secs: u64) -> ResponsesStream {
        ResponsesStream { id: id.into(), model: model.into(), created: created_secs, seq: 0, out: ChatOutput::default(), started: false }
    }

    fn emit(&mut self, name: &str, mut body: Value) -> String {
        body["type"] = json!(name);
        body["sequence_number"] = json!(self.seq);
        self.seq += 1;
        crate::sse::named(name, &body.to_string())
    }

    /// The SSE frames for one event.
    pub fn event(&mut self, e: &ChatEvent) -> Vec<String> {
        let mut frames = vec![];
        if !self.started {
            self.started = true;
            let skeleton = json!({"id": self.id, "object": "response", "created_at": self.created, "status": "in_progress", "model": self.model, "output": []});
            frames.push(self.emit("response.created", json!({"response": skeleton})));
        }
        self.out.push(e);
        match e {
            ChatEvent::Delta(t) => {
                let item = format!("msg_{}", self.id);
                frames.push(self.emit("response.output_text.delta", json!({"item_id": item, "output_index": 0, "content_index": 0, "delta": t})));
            }
            ChatEvent::Reasoning(_) | ChatEvent::ToolCall { .. } => {}
            ChatEvent::Done { .. } => {
                if !self.out.text.is_empty() {
                    let item = format!("msg_{}", self.id);
                    let text = self.out.text.clone();
                    frames.push(self.emit("response.output_text.done", json!({"item_id": item, "output_index": 0, "content_index": 0, "text": text})));
                }
                for (i, item) in response_output(&self.out, &self.id).into_iter().enumerate() {
                    frames.push(self.emit("response.output_item.done", json!({"output_index": i, "item": item})));
                }
                let r = responses_response(&self.out, &self.id, &self.model, self.created);
                frames.push(self.emit("response.completed", json!({"response": r})));
            }
        }
        frames
    }
}

/// Parses a `POST /v1/embeddings` body (`input`: a string or an array of strings; token arrays are refused).
pub fn parse_embeddings(v: &Value) -> Result<EmbedRequest, ApiError> {
    let o = object(v, "the request")?;
    let input = match o.get("input") {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .map(|x| x.as_str().map(str::to_string).ok_or_else(|| ApiError::invalid("`input` must hold strings (token arrays are not supported)")))
            .collect::<Result<_, _>>()?,
        _ => return Err(ApiError::invalid("`input` is required")),
    };
    if o.get("encoding_format").and_then(Value::as_str).is_some_and(|f| f != "float") {
        return Err(ApiError::invalid("only `encoding_format: \"float\"` is supported"));
    }
    let req = EmbedRequest { model: model_ref(o)?, input };
    req.check()?;
    Ok(req)
}

/// The embeddings `list` object.
pub fn embeddings_response(out: &EmbedOutput, model: &str) -> Value {
    json!({
        "object": "list",
        "data": out.vectors.iter().enumerate().map(|(i, v)| json!({"object": "embedding", "index": i, "embedding": v})).collect::<Vec<_>>(),
        "model": model,
        "usage": {"prompt_tokens": out.prompt_tokens, "total_tokens": out.prompt_tokens}
    })
}

/// `GET /v1/models`.
pub fn models(list: &[ModelEntry]) -> Value {
    json!({
        "object": "list",
        "data": list.iter().map(|m| json!({"id": m.id.as_str(), "object": "model", "created": m.modified / 1000, "owned_by": "pitwall"})).collect::<Vec<_>>()
    })
}

/// `GET /v1/models/{id}`.
pub fn model(m: &ModelEntry) -> Value {
    json!({"id": m.id.as_str(), "object": "model", "created": m.modified / 1000, "owned_by": "pitwall"})
}

/// The model a request named, for messages.
pub fn model_name(r: &ModelRef) -> &str {
    match r {
        ModelRef::Named(n) => n,
        ModelRef::Default => "default",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Timings, Usage};

    #[test]
    fn parses_the_sdk_chat_request() {
        let v = json!({
            "model": "qwen3-8b",
            "messages": [
                {"role": "developer", "content": "be brief"},
                {"role": "user", "content": [{"type": "text", "text": "what is this"}, {"type": "image_url", "image_url": {"url": "data:image/png;base64,iVBORw0KGgo="}}]},
                {"role": "assistant", "content": null, "tool_calls": [{"id": "call_1", "type": "function", "function": {"name": "f", "arguments": "{\"a\":1}"}}]},
                {"role": "tool", "tool_call_id": "call_1", "content": "42"}
            ],
            "tools": [{"type": "function", "function": {"name": "f", "description": "d", "parameters": {"type": "object"}}}],
            "tool_choice": {"type": "function", "function": {"name": "f"}},
            "max_completion_tokens": 64,
            "temperature": 0.2,
            "top_p": 0.9,
            "seed": 3,
            "stop": "\n\n",
            "stream": true,
            "response_format": {"type": "json_schema", "json_schema": {"name": "x", "schema": {"type": "object"}}}
        });
        let r = parse_chat(&v).unwrap();
        assert_eq!(r.model, ModelRef::Named("qwen3-8b".into()));
        assert_eq!(r.messages.len(), 4);
        assert_eq!(r.messages[0].role, Role::System);
        assert!(r.has_images());
        assert_eq!(r.messages[2].tool_calls[0].arguments, "{\"a\":1}");
        assert_eq!(r.messages[3].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(r.tool_choice, ToolChoice::Named("f".into()));
        assert_eq!((r.max_tokens, r.seed, r.stream), (Some(64), Some(3), true));
        assert_eq!(r.stop, vec!["\n\n"]);
        assert_eq!(r.format, OutputFormat::JsonSchema(json!({"type": "object"})));
        assert_eq!(model_name(&r.model), "qwen3-8b");
        assert_eq!(model_name(&ModelRef::Default), "default");
    }

    #[test]
    fn refuses_bad_chat_requests() {
        let cases = [
            (json!([]), "JSON object"),
            (json!({"model": "m"}), "`messages` is required"),
            (json!({"messages": [{"content": "x"}]}), "no `role`"),
            (json!({"messages": [{"role": "robot", "content": "x"}]}), "unknown message role"),
            (json!({"messages": [{"role": "user", "content": 5}]}), "string or an array"),
            (json!({"messages": [{"role": "user", "content": [{"type": "text"}]}]}), "no `text`"),
            (json!({"messages": [{"role": "user", "content": [{"type": "image_url"}]}]}), "no `image_url`"),
            (json!({"messages": [{"role": "user", "content": [{"type": "image_url", "image_url": "https://x/y.png"}]}]}), "never fetches"),
            (json!({"messages": [{"role": "user", "content": [{"type": "input_audio"}]}]}), "not supported"),
            (json!({"messages": [{"role": "assistant", "tool_calls": [{"function": {}}]}]}), "no name"),
            (json!({"messages": [{"role": "user", "content": "x"}], "tool_choice": "maybe"}), "unknown tool_choice"),
            (json!({"messages": [{"role": "user", "content": "x"}], "tool_choice": {}}), "names no function"),
            (json!({"messages": [{"role": "user", "content": "x"}], "tools": [{"function": {}}]}), "no `name`"),
            (json!({"messages": [{"role": "user", "content": "x"}], "response_format": {"type": "yaml"}}), "unknown response format"),
            (json!({"messages": [{"role": "user", "content": "x"}], "response_format": {"type": "json_schema"}}), "needs a `schema`"),
            (json!({"messages": [{"role": "user", "content": "x"}], "stream": "yes"}), "`stream` must be a boolean"),
            (json!({"messages": []}), "no messages"),
        ];
        for (v, needle) in cases {
            let e = parse_chat(&v).unwrap_err().to_string();
            assert!(e.contains(needle), "{v}: {e}");
        }
        // Arguments given as an object are kept as JSON text; missing ones become "{}".
        let r = parse_chat(&json!({"messages": [{"role": "assistant", "tool_calls": [{"function": {"name": "f", "arguments": {"a": 1}}}, {"function": {"name": "g"}}]}], "tool_choice": "required", "response_format": {"type": "text"}}))
            .unwrap();
        assert_eq!(r.messages[0].tool_calls[0].arguments, "{\"a\":1}");
        assert_eq!(r.messages[0].tool_calls[1].arguments, "{}");
        assert_eq!(r.tool_choice, ToolChoice::Required);
        let r = parse_chat(&json!({"messages": [{"role": "user", "content": "x"}], "tool_choice": "none", "max_tokens": 9, "response_format": {"type": "json_object"}})).unwrap();
        assert_eq!((r.tool_choice, r.max_tokens, r.format), (ToolChoice::None, Some(9), OutputFormat::JsonObject));
        assert_eq!(parse_chat(&json!({"messages": [{"role": "user", "content": "x"}], "tool_choice": "auto"})).unwrap().tool_choice, ToolChoice::Auto);
    }

    #[test]
    fn chat_responses_and_streams() {
        let out = ChatOutput {
            text: "Hi".into(),
            reasoning: "hm".into(),
            tool_calls: vec![],
            finish: Some(Finish::Stop),
            usage: Usage { prompt_tokens: 3, completion_tokens: 1 },
            timings: Some(Timings::default()),
        };
        let r = chat_response(&out, "chatcmpl-1", "m", 9);
        assert_eq!(r["object"], "chat.completion");
        assert_eq!(r["choices"][0]["message"]["content"], "Hi");
        assert_eq!(r["choices"][0]["message"]["reasoning_content"], "hm");
        assert_eq!(r["choices"][0]["finish_reason"], "stop");
        assert_eq!(r["usage"]["total_tokens"], 4);
        let calls = ChatOutput {
            text: String::new(),
            tool_calls: vec![ToolCall { id: "c".into(), name: "f".into(), arguments: "{}".into() }],
            finish: Some(Finish::ToolCalls),
            ..Default::default()
        };
        let r = chat_response(&calls, "x", "m", 0);
        assert!(r["choices"][0]["message"]["content"].is_null());
        assert_eq!(r["choices"][0]["message"]["tool_calls"][0]["function"]["name"], "f");
        assert_eq!(r["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(chat_response(&ChatOutput { finish: Some(Finish::Length), ..Default::default() }, "x", "m", 0)["choices"][0]["finish_reason"], "length");

        let mut s = ChatStream::new("id", "m", 1);
        let f = s.event(&ChatEvent::Delta("He".into()));
        assert_eq!(f.len(), 2, "role chunk first");
        assert!(f[0].contains("\"role\":\"assistant\""));
        assert!(f[1].starts_with("data: ") && f[1].contains("\"content\":\"He\""));
        assert!(s.event(&ChatEvent::Reasoning("r".into()))[0].contains("reasoning_content"));
        let t = s.event(&ChatEvent::ToolCall { index: 0, id: Some("c".into()), name: Some("f".into()), arguments: "{".into() });
        assert!(t[0].contains("\"name\":\"f\"") && t[0].contains("\"type\":\"function\""));
        let t = s.event(&ChatEvent::ToolCall { index: 0, id: None, name: None, arguments: "}".into() });
        assert!(!t[0].contains("\"name\""));
        let d = s.event(&ChatEvent::Done { finish: Finish::Stop, usage: Usage { prompt_tokens: 1, completion_tokens: 2 }, timings: None });
        assert!(d[0].contains("\"finish_reason\":\"stop\"") && d[0].contains("\"total_tokens\":3"));
        assert_eq!(d[1], "data: [DONE]\n\n");
    }

    #[test]
    fn responses_api() {
        let v = json!({
            "model": "m",
            "instructions": "sys",
            "input": [
                {"role": "user", "content": [{"type": "input_text", "text": "hi"}]},
                {"type": "function_call", "call_id": "c1", "name": "f", "arguments": "{}"},
                {"type": "function_call_output", "call_id": "c1", "output": "42"},
                {"type": "function_call_output", "call_id": "c2", "output": {"a": 1}},
                {"type": "function_call_output", "call_id": "c3"}
            ],
            "tools": [{"type": "function", "name": "f", "parameters": {"type": "object"}}],
            "max_output_tokens": 10,
            "text": {"format": {"type": "json_schema", "name": "o", "schema": {"type": "object"}}}
        });
        let r = parse_responses(&v).unwrap();
        assert_eq!(r.messages.len(), 6);
        assert_eq!(r.messages[0].role, Role::System);
        assert_eq!(r.messages[2].tool_calls[0].id, "c1");
        assert_eq!(r.messages[3].tool_call_id.as_deref(), Some("c1"));
        assert_eq!(r.messages[4].text_content(), "{\"a\":1}");
        assert_eq!(r.messages[5].text_content(), "");
        assert_eq!(r.tools[0].name, "f");
        assert_eq!(r.max_tokens, Some(10));
        assert!(matches!(r.format, OutputFormat::JsonSchema(_)));
        assert_eq!(parse_responses(&json!({"input": "hello"})).unwrap().messages[0].text_content(), "hello");
        assert!(parse_responses(&json!({})).unwrap_err().to_string().contains("`input` is required"));
        assert!(parse_responses(&json!({"input": [{"type": "reasoning"}]})).unwrap_err().to_string().contains("not supported"));
        assert!(parse_responses(&json!({"input": "x", "previous_response_id": "r1"})).unwrap_err().to_string().contains("does not store"));
        let tools_default = parse_responses(&json!({"input": "x", "tools": [{"name": "g"}]})).unwrap();
        assert_eq!(tools_default.tools[0].parameters["type"], "object");

        let out = ChatOutput {
            text: "ok".into(),
            tool_calls: vec![ToolCall { id: "c".into(), name: "f".into(), arguments: "{}".into() }],
            finish: Some(Finish::ToolCalls),
            usage: Usage { prompt_tokens: 2, completion_tokens: 3 },
            ..Default::default()
        };
        let r = responses_response(&out, "resp_1", "m", 5);
        assert_eq!(r["object"], "response");
        assert_eq!(r["status"], "completed");
        assert_eq!(r["output"][0]["content"][0]["type"], "output_text");
        assert_eq!(r["output"][1]["type"], "function_call");
        assert_eq!(r["usage"]["total_tokens"], 5);
        let cut = responses_response(&ChatOutput { finish: Some(Finish::Length), ..Default::default() }, "r", "m", 0);
        assert_eq!(cut["status"], "incomplete");
        assert_eq!(cut["incomplete_details"]["reason"], "max_output_tokens");

        let mut s = ResponsesStream::new("resp_2", "m", 1);
        let f = s.event(&ChatEvent::Delta("a".into()));
        assert!(f[0].starts_with("event: response.created\n") && f[1].contains("response.output_text.delta"));
        assert!(s.event(&ChatEvent::Reasoning("x".into())).is_empty());
        let f = s.event(&ChatEvent::Done { finish: Finish::Stop, usage: Usage::default(), timings: None });
        assert!(f[0].contains("output_text.done") && f.last().unwrap().contains("response.completed"));
        assert!(f.last().unwrap().contains("\"sequence_number\":"));
        let mut empty = ResponsesStream::new("r3", "m", 1);
        let f = empty.event(&ChatEvent::Done { finish: Finish::Stop, usage: Usage::default(), timings: None });
        assert!(!f.iter().any(|x| x.contains("output_text.done")));
    }

    #[test]
    fn embeddings_and_models() {
        let r = parse_embeddings(&json!({"model": "e", "input": ["a", "b"]})).unwrap();
        assert_eq!(r.input, vec!["a", "b"]);
        assert_eq!(parse_embeddings(&json!({"input": "a", "encoding_format": "float"})).unwrap().input, vec!["a"]);
        assert!(parse_embeddings(&json!({"input": [[1, 2]]})).unwrap_err().to_string().contains("token arrays"));
        assert!(parse_embeddings(&json!({"input": "a", "encoding_format": "base64"})).is_err());
        assert!(parse_embeddings(&json!({})).is_err());
        assert!(parse_embeddings(&json!({"input": []})).is_err());
        let out = EmbedOutput { vectors: vec![vec![1.0], vec![2.0]], prompt_tokens: 4 };
        let r = embeddings_response(&out, "e");
        assert_eq!(r["data"][1]["index"], 1);
        assert_eq!(r["data"][1]["object"], "embedding");
        assert_eq!(r["usage"]["prompt_tokens"], 4);
        let m = ModelEntry {
            id: crate::ids::ModelId::parse("m").unwrap(),
            size: 1,
            family: None,
            parameter_size: None,
            quant: None,
            modified: 5000,
            sha256: None,
            loaded: None,
        };
        let l = models(std::slice::from_ref(&m));
        assert_eq!(l["object"], "list");
        assert_eq!(l["data"][0]["id"], "m");
        assert_eq!(model(&m)["created"], 5);
    }
}
