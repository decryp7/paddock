//! llama-server's protocol (ADR 0004): the command line Pitwall starts it with, the `/v1/chat/completions` and
//! `/v1/embeddings` bodies it sends, and the parsing of what comes back (stream chunks, `/props`, embeddings).
//! The server runs on 127.0.0.1 with a random API key, `--offline` (it never downloads anything itself) and no web UI.

use crate::chat::{ChatError, ChatEvent, ChatRequest, EmbedOutput, Finish, OutputFormat, Part, Role, Timings, ToolChoice, Usage};
use serde_json::{json, Map, Value};

/// What kind of server to start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeKind {
    /// Chat and completion.
    Chat,
    /// `/v1/embeddings`.
    Embedding,
    /// `/v1/rerank`.
    Rerank,
}

/// How to start one llama-server.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    /// The model file (first part of a split model).
    pub model: String,
    /// The vision projector.
    pub mmproj: Option<String>,
    /// The name clients see (`--alias`).
    pub alias: String,
    /// The loopback port.
    pub port: u16,
    /// `--ctx-size`: the KV positions of **all** slots together. llama-server splits it evenly, so each slot gets
    /// `context / parallel` tokens; build it with `Slots::launch_context` from the context per slot.
    pub context: u32,
    /// Layers on the GPU (`None`: all).
    pub gpu_layers: Option<u32>,
    /// CPU threads (0: llama.cpp decides).
    pub threads: u32,
    /// `--parallel`: slots, i.e. requests served at once.
    pub parallel: u32,
    /// What it serves.
    pub kind: ServeKind,
    /// Host-memory prompt cache in MiB (`--cache-ram`; 0 disables it).
    pub cache_ram_mib: u32,
    /// Where `POST /slots/{id}?action=save|restore` keep files (`--slot-save-path`); `None` leaves saving off.
    pub slot_save_path: Option<String>,
}

/// The environment variable llama-server reads its API key from (never put on the command line).
pub const API_KEY_ENV: &str = "LLAMA_API_KEY";

/// The command-line arguments. Every option is passed explicitly: llama-server's defaults change between releases.
pub fn args(l: &Launch) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--model".into(),
        l.model.clone(),
        "--alias".into(),
        l.alias.clone(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        l.port.to_string(),
        "--ctx-size".into(),
        l.context.to_string(),
        "--n-gpu-layers".into(),
        l.gpu_layers.map_or_else(|| "999".to_string(), |n| n.to_string()),
        "--parallel".into(),
        l.parallel.max(1).to_string(),
        "--cache-ram".into(),
        l.cache_ram_mib.to_string(),
        "--offline".into(),
        "--no-webui".into(),
        // A request over a slot's context is refused, never shifted (dropping the start of a conversation silently).
        "--no-context-shift".into(),
        "--log-verbosity".into(),
        "1".into(),
    ];
    if l.threads > 0 {
        a.extend(["--threads".into(), l.threads.to_string()]);
    }
    match l.kind {
        ServeKind::Chat => {
            a.extend(["--jinja".into(), "--cache-reuse".into(), "256".into()]);
            if let Some(p) = &l.mmproj {
                a.extend(["--mmproj".into(), p.clone()]);
            }
            if let Some(p) = &l.slot_save_path {
                a.extend(["--slot-save-path".into(), p.clone()]);
            }
        }
        ServeKind::Embedding => a.push("--embeddings".into()),
        ServeKind::Rerank => a.push("--reranking".into()),
    }
    a
}

/// The `/v1/chat/completions` body for a request. Always streamed, with usage, and with prompt caching. `slot` asks for
/// a specific llama-server slot (`id_slot`), so a conversation's prompt cache is found again (slot affinity, ADR 0019).
pub fn chat_body(req: &ChatRequest, alias: &str, slot: Option<u32>) -> Value {
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            };
            let content = if m.images().next().is_some() || m.audio().next().is_some() {
                Value::Array(
                    m.parts
                        .iter()
                        .map(|p| match p {
                            Part::Text(t) => json!({"type": "text", "text": t}),
                            Part::Image(i) => json!({"type": "image_url", "image_url": {"url": i.data_url()}}),
                            // Inline data only; never `url` (llama-server would fetch it) and never `--media-path`.
                            Part::Audio(a) => json!({"type": "input_audio", "input_audio": {"data": a.data, "format": a.format.as_str()}}),
                        })
                        .collect(),
                )
            } else {
                Value::String(m.text_content())
            };
            let mut o = Map::new();
            o.insert("role".into(), role.into());
            o.insert("content".into(), content);
            if !m.tool_calls.is_empty() {
                o.insert(
                    "tool_calls".into(),
                    m.tool_calls
                        .iter()
                        .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.arguments}}))
                        .collect(),
                );
            }
            if let Some(id) = &m.tool_call_id {
                o.insert("tool_call_id".into(), id.clone().into());
            }
            Value::Object(o)
        })
        .collect();
    let mut o = Map::new();
    o.insert("model".into(), alias.into());
    o.insert("messages".into(), Value::Array(messages));
    o.insert("stream".into(), true.into());
    o.insert("stream_options".into(), json!({"include_usage": true}));
    o.insert("cache_prompt".into(), true.into());
    if let Some(n) = slot {
        o.insert("id_slot".into(), n.into());
    }
    if !req.tools.is_empty() {
        o.insert(
            "tools".into(),
            req.tools
                .iter()
                .map(
                    |t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}),
                )
                .collect(),
        );
        o.insert(
            "tool_choice".into(),
            match &req.tool_choice {
                ToolChoice::Auto => json!("auto"),
                ToolChoice::None => json!("none"),
                ToolChoice::Required => json!("required"),
                ToolChoice::Named(n) => json!({"type": "function", "function": {"name": n}}),
            },
        );
    }
    match &req.format {
        OutputFormat::Text => {}
        OutputFormat::JsonObject => {
            o.insert("response_format".into(), json!({"type": "json_object"}));
        }
        OutputFormat::JsonSchema(s) => {
            o.insert(
                "response_format".into(),
                json!({"type": "json_schema", "json_schema": {"name": "output", "schema": s}}),
            );
        }
    }
    let mut put = |k: &str, v: Option<Value>| {
        if let Some(v) = v {
            o.insert(k.into(), v);
        }
    };
    put("max_tokens", req.max_tokens.map(Value::from));
    put("temperature", req.temperature.map(|t| json!(t)));
    put("top_p", req.top_p.map(|t| json!(t)));
    put("top_k", req.top_k.map(Value::from));
    put("seed", req.seed.map(Value::from));
    put("stop", (!req.stop.is_empty()).then(|| json!(req.stop)));
    Value::Object(o)
}

/// Folds llama-server stream chunks into [`ChatEvent`]s. The finish reason, usage and timings arrive in different
/// chunks; `Done` is emitted on `[DONE]` (or [`Stream::finish`] if the stream just ends).
#[derive(Debug, Default)]
pub struct Stream {
    finish: Option<Finish>,
    usage: Usage,
    timings: Option<Timings>,
    done: bool,
}

impl Stream {
    /// A new stream.
    pub fn new() -> Stream {
        Stream::default()
    }

    /// Parses one `data:` payload.
    pub fn chunk(&mut self, data: &str) -> Result<Vec<ChatEvent>, ChatError> {
        if self.done {
            return Ok(vec![]);
        }
        if data.trim() == "[DONE]" {
            return Ok(self.finish().into_iter().collect());
        }
        let v: Value = serde_json::from_str(data).map_err(|e| ChatError::Invalid(format!("the engine sent an unreadable chunk: {e}")))?;
        if let Some(err) = v.get("error") {
            let msg = err.get("message").and_then(Value::as_str).unwrap_or("the engine failed");
            return Err(ChatError::Invalid(msg.to_string()));
        }
        let mut out = vec![];
        if let Some(choice) = v.get("choices").and_then(Value::as_array).and_then(|c| c.first()) {
            let delta = choice.get("delta").unwrap_or(&Value::Null);
            if let Some(t) = delta.get("reasoning_content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                out.push(ChatEvent::Reasoning(t.to_string()));
            }
            if let Some(t) = delta.get("content").and_then(Value::as_str).filter(|t| !t.is_empty()) {
                out.push(ChatEvent::Delta(t.to_string()));
            }
            for (n, call) in delta.get("tool_calls").and_then(Value::as_array).into_iter().flatten().enumerate() {
                let f = call.get("function").unwrap_or(&Value::Null);
                out.push(ChatEvent::ToolCall {
                    index: call
                        .get("index")
                        .and_then(Value::as_u64)
                        .map_or(n as u32, |i| i.min(u64::from(u32::MAX)) as u32),
                    id: call.get("id").and_then(Value::as_str).map(str::to_string),
                    name: f.get("name").and_then(Value::as_str).map(str::to_string),
                    arguments: f.get("arguments").and_then(Value::as_str).unwrap_or_default().to_string(),
                });
            }
            if let Some(r) = choice.get("finish_reason").and_then(Value::as_str) {
                self.finish = Some(match r {
                    "length" => Finish::Length,
                    "tool_calls" => Finish::ToolCalls,
                    _ => Finish::Stop,
                });
            }
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0).min(u64::from(u32::MAX)) as u32;
            self.usage = Usage {
                prompt_tokens: n("prompt_tokens"),
                completion_tokens: n("completion_tokens"),
            };
        }
        if let Some(t) = v.get("timings").filter(|t| t.is_object()) {
            let f = |k: &str| {
                t.get(k)
                    .and_then(Value::as_f64)
                    .filter(|x| x.is_finite() && *x >= 0.0)
                    .unwrap_or(0.0)
            };
            self.timings = Some(Timings {
                prompt_tps: f("prompt_per_second"),
                gen_tps: f("predicted_per_second"),
                cached_tokens: t.get("cache_n").and_then(Value::as_u64).unwrap_or(0).min(u64::from(u32::MAX)) as u32,
            });
        }
        Ok(out)
    }

    /// Ends the stream (once): `Done` with what was collected; a stream that ended without a finish reason was cut.
    pub fn finish(&mut self) -> Option<ChatEvent> {
        if self.done {
            return None;
        }
        self.done = true;
        Some(ChatEvent::Done {
            finish: self.finish.unwrap_or(Finish::Stop),
            usage: self.usage,
            timings: self.timings,
        })
    }
}

/// The `/v1/embeddings` body.
pub fn embed_body(inputs: &[String], alias: &str) -> Value {
    json!({"model": alias, "input": inputs, "encoding_format": "float"})
}

/// Parses a `/v1/embeddings` answer, in input order.
pub fn parse_embeddings(v: &Value, expected: usize) -> Result<EmbedOutput, ChatError> {
    let bad = |m: &str| ChatError::Invalid(format!("the engine's embeddings are unusable: {m}"));
    let data = v.get("data").and_then(Value::as_array).ok_or_else(|| bad("no data"))?;
    let mut vectors: Vec<(usize, Vec<f32>)> = data
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let idx = d.get("index").and_then(Value::as_u64).map_or(i, |x| x as usize);
            let e = d
                .get("embedding")
                .and_then(Value::as_array)
                .ok_or_else(|| bad("an item has no embedding"))?;
            let v = e
                .iter()
                .map(|x| x.as_f64().map(|f| f as f32).ok_or_else(|| bad("a value is not a number")))
                .collect::<Result<_, _>>()?;
            Ok((idx, v))
        })
        .collect::<Result<_, ChatError>>()?;
    if vectors.len() != expected {
        return Err(bad(&format!("{} vectors for {expected} inputs", vectors.len())));
    }
    vectors.sort_by_key(|(i, _)| *i);
    let prompt_tokens = v
        .pointer("/usage/prompt_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(u64::from(u32::MAX)) as u32;
    Ok(EmbedOutput {
        vectors: vectors.into_iter().map(|(_, v)| v).collect(),
        prompt_tokens,
    })
}

/// What a running llama-server says about itself (`GET /props`); every field optional (builds differ; IMT ADR 0009).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Props {
    /// The context the server allocated.
    pub n_ctx: Option<u32>,
    /// It reads images.
    pub vision: Option<bool>,
    /// It reads audio.
    pub audio: Option<bool>,
    /// The model file name (never a full path).
    pub model: Option<String>,
    /// Its build, e.g. `b6800-abc1234`.
    pub build: Option<String>,
    /// It has a chat template.
    pub chat_template: bool,
}

/// Parses `/props`.
pub fn parse_props(v: &Value) -> Props {
    let n_ctx = v
        .pointer("/default_generation_settings/n_ctx")
        .or_else(|| v.get("n_ctx"))
        .and_then(Value::as_u64)
        .map(|n| n.min(u64::from(u32::MAX)) as u32);
    let model = v
        .get("model_path")
        .and_then(Value::as_str)
        .map(|p| p.rsplit(['/', '\\']).next().unwrap_or(p).to_string())
        .filter(|s| !s.is_empty());
    Props {
        n_ctx,
        vision: v.pointer("/modalities/vision").and_then(Value::as_bool),
        audio: v.pointer("/modalities/audio").and_then(Value::as_bool),
        model,
        build: v.get("build_info").and_then(Value::as_str).map(str::to_string),
        chat_template: v.get("chat_template").and_then(Value::as_str).is_some_and(|t| !t.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Image, ImageMime, Message, Tool, ToolCall};

    fn launch(kind: ServeKind) -> Launch {
        Launch {
            model: "m.gguf".into(),
            mmproj: Some("p.gguf".into()),
            alias: "m".into(),
            port: 4100,
            context: 8192,
            gpu_layers: None,
            threads: 0,
            parallel: 0,
            kind,
            cache_ram_mib: 0,
            slot_save_path: None,
        }
    }

    #[test]
    fn command_lines() {
        let a = args(&launch(ServeKind::Chat));
        let s = a.join(" ");
        assert!(s.contains("--host 127.0.0.1 --port 4100"), "{s}");
        assert!(s.contains("--n-gpu-layers 999") && s.contains("--parallel 1") && s.contains("--offline") && s.contains("--no-webui"));
        assert!(s.contains("--jinja") && s.contains("--mmproj p.gguf") && !s.contains("--threads"));
        assert!(!s.contains("api-key"), "the key goes in the environment");
        let e = args(&Launch {
            gpu_layers: Some(10),
            threads: 6,
            ..launch(ServeKind::Embedding)
        })
        .join(" ");
        assert!(e.contains("--embeddings") && e.contains("--n-gpu-layers 10") && e.contains("--threads 6") && !e.contains("--mmproj"));
        assert!(args(&launch(ServeKind::Rerank)).contains(&"--reranking".to_string()));
    }

    #[test]
    fn the_server_never_drops_the_start_of_a_conversation_to_make_room() {
        // A request over a slot's context is refused, never shifted: the chat checks the window before sending and says so in
        // words (user decision 2026-11-08). llama.cpp b11669 has the flag (`llama-server --help`, checked 2026-11-08).
        for kind in [ServeKind::Chat, ServeKind::Embedding, ServeKind::Rerank] {
            assert!(args(&launch(kind)).contains(&"--no-context-shift".to_string()), "{kind:?}");
        }
    }

    #[test]
    fn parallel_slots_split_the_context_size() {
        use crate::slots::Slots;
        let slots = Slots::new(4).unwrap();
        let a = args(&Launch {
            context: slots.launch_context(8192),
            parallel: slots.get(),
            ..launch(ServeKind::Chat)
        });
        let pos = |flag: &str| a.iter().position(|x| x == flag).map(|i| a[i + 1].clone());
        assert_eq!(pos("--ctx-size").as_deref(), Some("32768"), "per-slot context x slots");
        assert_eq!(pos("--parallel").as_deref(), Some("4"));
        // llama-server hands each slot ctx-size / parallel tokens: the context per slot is what was asked for.
        assert_eq!(32_768 / 4, 8192);
    }

    #[test]
    fn an_audio_clip_is_sent_inline_as_input_audio() {
        let a = crate::chat::Audio::from_base64(crate::chat::AudioFormat::Mp3, "SUQz").unwrap();
        let req = ChatRequest {
            messages: vec![Message {
                role: Role::User,
                parts: vec![Part::Text("what".into()), Part::Audio(a)],
                tool_calls: vec![],
                tool_call_id: None,
            }],
            ..Default::default()
        };
        let b = chat_body(&req, "alias", None);
        assert_eq!(
            b["messages"][0]["content"][1],
            json!({"type": "input_audio", "input_audio": {"data": "SUQz", "format": "mp3"}})
        );
        assert_eq!(b["messages"][0]["content"][0]["text"], "what");
    }

    #[test]
    fn chat_bodies() {
        let img = Image::from_base64(ImageMime::Png, "iVBORw0KGgo=").unwrap();
        let req = ChatRequest {
            messages: vec![
                Message::text(Role::System, "be brief"),
                Message {
                    role: Role::User,
                    parts: vec![Part::Text("what".into()), Part::Image(img)],
                    tool_calls: vec![],
                    tool_call_id: None,
                },
                Message {
                    role: Role::Assistant,
                    parts: vec![],
                    tool_calls: vec![ToolCall {
                        id: "c1".into(),
                        name: "f".into(),
                        arguments: "{}".into(),
                    }],
                    tool_call_id: None,
                },
                Message {
                    role: Role::Tool,
                    parts: vec![Part::Text("42".into())],
                    tool_calls: vec![],
                    tool_call_id: Some("c1".into()),
                },
            ],
            tools: vec![Tool {
                name: "f".into(),
                description: "d".into(),
                parameters: json!({"type": "object"}),
            }],
            tool_choice: ToolChoice::Named("f".into()),
            max_tokens: Some(5),
            temperature: Some(0.5),
            top_p: Some(0.9),
            top_k: Some(40),
            seed: Some(1),
            stop: vec!["\n".into()],
            format: OutputFormat::JsonSchema(json!({"type": "object"})),
            ..Default::default()
        };
        let b = chat_body(&req, "alias", None);
        assert_eq!(b["model"], "alias");
        assert_eq!(b["stream"], true);
        assert_eq!(
            b["messages"][1]["content"][1]["image_url"]["url"],
            "data:image/png;base64,iVBORw0KGgo="
        );
        assert_eq!(b["messages"][2]["tool_calls"][0]["function"]["name"], "f");
        assert_eq!(b["messages"][3]["tool_call_id"], "c1");
        assert_eq!(b["messages"][3]["role"], "tool");
        assert_eq!(b["tool_choice"]["function"]["name"], "f");
        assert_eq!(b["response_format"]["type"], "json_schema");
        assert_eq!(
            (b["max_tokens"].as_u64(), b["top_k"].as_u64(), b["seed"].as_i64()),
            (Some(5), Some(40), Some(1))
        );
        assert_eq!(b["stop"][0], "\n");
        for (c, want) in [
            (ToolChoice::Auto, json!("auto")),
            (ToolChoice::None, json!("none")),
            (ToolChoice::Required, json!("required")),
        ] {
            let r = ChatRequest {
                tool_choice: c,
                ..req.clone()
            };
            assert_eq!(chat_body(&r, "a", None)["tool_choice"], want);
        }
        let plain = ChatRequest {
            messages: vec![Message::text(Role::User, "hi")],
            format: OutputFormat::JsonObject,
            ..Default::default()
        };
        let b = chat_body(&plain, "a", None);
        assert_eq!(b["messages"][0]["content"], "hi");
        assert_eq!(b["response_format"]["type"], "json_object");
        assert!(b.get("tools").is_none() && b.get("max_tokens").is_none());
        let text = ChatRequest {
            format: OutputFormat::Text,
            ..plain
        };
        assert!(chat_body(&text, "a", None).get("response_format").is_none());
    }

    #[test]
    fn stream_chunks() {
        let mut s = Stream::new();
        let e = s
            .chunk(r#"{"choices":[{"delta":{"reasoning_content":"r","content":"Hi"}}]}"#)
            .unwrap();
        assert_eq!(e, vec![ChatEvent::Reasoning("r".into()), ChatEvent::Delta("Hi".into())]);
        let e = s
            .chunk(r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"f","arguments":"{"}},{"function":{"arguments":"}"}}]}}]}"#)
            .unwrap();
        assert_eq!(e.len(), 2);
        assert!(matches!(&e[1], ChatEvent::ToolCall { index: 1, id: None, name: None, arguments } if arguments == "}"));
        s.chunk(r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#).unwrap();
        s.chunk(r#"{"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":3},"timings":{"prompt_per_second":100.5,"predicted_per_second":30.0}}"#)
            .unwrap();
        let done = s.chunk("[DONE]").unwrap();
        assert_eq!(
            done,
            vec![ChatEvent::Done {
                finish: Finish::ToolCalls,
                usage: Usage {
                    prompt_tokens: 7,
                    completion_tokens: 3
                },
                timings: Some(Timings {
                    prompt_tps: 100.5,
                    gen_tps: 30.0,
                    cached_tokens: 0
                })
            }]
        );
        assert!(s.chunk("[DONE]").unwrap().is_empty());
        assert!(s.finish().is_none());
        let mut s = Stream::new();
        s.chunk(r#"{"choices":[],"timings":{"prompt_per_second":1.0,"predicted_per_second":2.0,"cache_n":123}}"#)
            .unwrap();
        assert!(matches!(
            s.finish(),
            Some(ChatEvent::Done {
                timings: Some(Timings { cached_tokens: 123, .. }),
                ..
            })
        ));
        let req = ChatRequest {
            messages: vec![crate::chat::Message::text(Role::User, "x")],
            ..Default::default()
        };
        assert_eq!(chat_body(&req, "a", Some(3))["id_slot"], 3);
        let launch = Launch {
            model: "m".into(),
            mmproj: None,
            alias: "a".into(),
            port: 1,
            context: 512,
            gpu_layers: None,
            threads: 0,
            parallel: 1,
            kind: ServeKind::Chat,
            cache_ram_mib: 0,
            slot_save_path: Some("/data/slots".into()),
        };
        let a = args(&launch);
        let at = a.iter().position(|x| x == "--slot-save-path").unwrap();
        assert_eq!(a[at + 1], "/data/slots");
        assert!(chat_body(&req, "a", None).get("id_slot").is_none());
        let mut s = Stream::new();
        s.chunk(r#"{"choices":[{"delta":{"content":""},"finish_reason":"length"}]}"#)
            .unwrap();
        assert!(matches!(
            s.finish(),
            Some(ChatEvent::Done {
                finish: Finish::Length,
                ..
            })
        ));
        let mut s = Stream::new();
        s.chunk(r#"{"choices":[{"finish_reason":"stop"}]}"#).unwrap();
        assert!(matches!(
            s.finish(),
            Some(ChatEvent::Done {
                finish: Finish::Stop,
                timings: None,
                ..
            })
        ));
        assert!(Stream::new().chunk("not json").unwrap_err().to_string().contains("unreadable"));
        let e = Stream::new().chunk(r#"{"error":{"message":"context full"}}"#).unwrap_err();
        assert_eq!(e.to_string(), "context full");
        assert!(Stream::new().chunk(r#"{"error":{}}"#).unwrap_err().to_string().contains("failed"));
    }

    #[test]
    fn embeddings() {
        assert_eq!(embed_body(&["a".into()], "e")["input"][0], "a");
        let v = json!({"data":[{"index":1,"embedding":[0.5]},{"index":0,"embedding":[1.0, 2.0]}],"usage":{"prompt_tokens":4}});
        let o = parse_embeddings(&v, 2).unwrap();
        assert_eq!(o.vectors, vec![vec![1.0, 2.0], vec![0.5]]);
        assert_eq!(o.prompt_tokens, 4);
        assert!(parse_embeddings(&v, 3).unwrap_err().to_string().contains("2 vectors for 3"));
        assert!(parse_embeddings(&json!({}), 1).is_err());
        assert!(parse_embeddings(&json!({"data":[{}]}), 1).is_err());
        assert!(parse_embeddings(&json!({"data":[{"embedding":["x"]}]}), 1).is_err());
        assert_eq!(parse_embeddings(&json!({"data":[{"embedding":[1]}]}), 1).unwrap().prompt_tokens, 0);
    }

    #[test]
    fn props() {
        let p = parse_props(&json!({
            "default_generation_settings": {"n_ctx": 8192},
            "modalities": {"vision": true, "audio": false},
            "model_path": "C:\\models\\qwen\\m.gguf",
            "build_info": "b11669-620d222",
            "chat_template": "{{x}}"
        }));
        assert_eq!(
            p,
            Props {
                n_ctx: Some(8192),
                vision: Some(true),
                audio: Some(false),
                model: Some("m.gguf".into()),
                build: Some("b11669-620d222".into()),
                chat_template: true
            }
        );
        assert_eq!(parse_props(&json!({"n_ctx": 1})).n_ctx, Some(1));
        assert_eq!(parse_props(&json!({})), Props::default());
        assert_eq!(parse_props(&json!({"model_path": ""})).model, None);
    }
}
