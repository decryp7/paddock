//! The filter: redacts the text of a request's messages (`messages[].content` as a string or as text parts, and a
//! completion's `prompt`), leaves everything else (model, parameters, roles) as it is, and writes the audit line
//! when the user turned the audit on.

use crate::audit;
use crate::redact::{redact, Counts};
use pitwall_plugin_api::prelude::*;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// The plugin: its manifest, the filter and its settings.
pub fn plugin() -> Plugin {
    let audit_dir: Arc<RwLock<Option<PathBuf>>> = Arc::default();
    let settings = audit_dir.clone();
    Plugin::new(include_str!("../pitwall-plugin.toml"))
        .filter(Redactor { audit_dir })
        .configure(move |s, ctx| configure(&settings, &s, ctx))
}

/// Applies `{"audit": bool}`.
fn configure(audit_dir: &RwLock<Option<PathBuf>>, settings: &Value, ctx: &PluginContext) -> Result<(), PluginError> {
    let obj = settings
        .as_object()
        .ok_or_else(|| PluginError::invalid("the settings must be a JSON object"))?;
    if let Some(k) = obj.keys().find(|k| k.as_str() != "audit") {
        return Err(PluginError::invalid(format!("unknown setting \"{k}\" (the only setting is \"audit\")")));
    }
    let audit = match obj.get("audit") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(PluginError::invalid("\"audit\" must be true or false")),
    };
    let dir = match (audit, &ctx.data_dir) {
        (false, _) => None,
        (true, Some(d)) => Some(d.clone()),
        (true, None) => return Err(PluginError::invalid("the audit log needs a data folder, and Pitwall gave none")),
    };
    *audit_dir.write().unwrap_or_else(|p| p.into_inner()) = dir;
    Ok(())
}

/// The filter.
pub struct Redactor {
    audit_dir: Arc<RwLock<Option<PathBuf>>>,
}

impl Filter for Redactor {
    fn filter_request(&self, mut body: Value) -> Result<FilterVerdict, PluginError> {
        let counts = redact_body(&mut body);
        let dir = self.audit_dir.read().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some(dir) = dir {
            // The audit is best effort: a full disk must not block the user's request.
            let _ = audit::append(&dir, counts);
        }
        Ok(FilterVerdict::forward(body))
    }
}

/// Redacts one string value in place.
fn redact_value(v: &mut Value, counts: &mut Counts) {
    if let Value::String(s) = v {
        let (clean, c) = redact(s);
        if c.total() > 0 {
            *s = clean;
            counts.add(c);
        }
    }
}

/// Redacts the messages (and prompt) of an OpenAI-compatible request body.
pub fn redact_body(body: &mut Value) -> Counts {
    let mut counts = Counts::default();
    if let Some(messages) = body.get_mut("messages").and_then(Value::as_array_mut) {
        for m in messages {
            match m.get_mut("content") {
                Some(Value::Array(parts)) => {
                    for part in parts {
                        if let Some(text) = part.get_mut("text") {
                            redact_value(text, &mut counts);
                        }
                    }
                }
                Some(content) => redact_value(content, &mut counts),
                None => {}
            }
        }
    }
    match body.get_mut("prompt") {
        Some(Value::Array(prompts)) => prompts.iter_mut().for_each(|p| redact_value(p, &mut counts)),
        Some(prompt) => redact_value(prompt, &mut counts),
        None => {}
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use pitwall_plugin_api::{Collect, Reply, Request};

    #[test]
    fn messages_and_prompts_are_redacted_and_nothing_else() {
        let mut body = json!({
            "model": "admin@corp.example",
            "messages": [
                {"role": "system", "content": "Server 10.1.2.3"},
                {"role": "user", "content": [
                    {"type": "text", "text": "mail bob@x.io the key deadbeefdeadbeefdeadbeefdeadbeef"},
                    {"type": "image_url", "image_url": {"url": "data:..."}}
                ]},
                {"role": "assistant"}
            ],
            "temperature": 0.2
        });
        let counts = redact_body(&mut body);
        assert_eq!((counts.emails, counts.ipv4, counts.secrets), (1, 1, 1));
        assert_eq!(body["model"], "admin@corp.example", "only message text is touched");
        assert_eq!(body["messages"][0]["content"], "Server [ipv4]");
        assert_eq!(body["messages"][1]["content"][0]["text"], "mail [email] the key [secret]");
        assert_eq!(body["messages"][1]["content"][1]["image_url"]["url"], "data:...");
        assert_eq!(body["temperature"], 0.2);
        let mut completion = json!({"prompt": ["a@b.io", 3], "model": "m"});
        assert_eq!(redact_body(&mut completion).emails, 1);
        assert_eq!(completion["prompt"], json!(["[email]", 3]));
        let mut single = json!({"prompt": "ping 127.0.0.1"});
        redact_body(&mut single);
        assert_eq!(single["prompt"], "ping [ipv4]");
        let mut odd = json!([1, 2]);
        assert_eq!(redact_body(&mut odd), Counts::default());
    }

    #[test]
    fn the_plugin_filters_through_its_contract_and_audits_counts_only() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = PluginContext {
            data_dir: Some(dir.path().to_path_buf()),
        };
        let p = plugin().with_context(ctx);
        assert!(p.check().is_ok(), "{:?}", p.check());
        let ask = |r: Request| p.answer(r, &mut Collect::default());
        let request = json!({"messages": [{"role": "user", "content": "I am carol@secret.example"}]});
        // Audit off (the default): nothing is written.
        assert_eq!(ask(Request::Configure { settings: json!({}).into() }).unwrap(), Reply::Configured);
        let filtered = ask(Request::FilterRequest { body: request.clone().into() }).unwrap();
        let Reply::FilteredRequest(FilterVerdict::Forward(body)) = filtered else {
            panic!("{filtered:?}")
        };
        assert_eq!(body["messages"][0]["content"], "I am [email]");
        assert!(!dir.path().join(audit::LOG_FILE).exists());
        // Audit on: one line of counts, no content.
        ask(Request::Configure {
            settings: json!({"audit": true}).into(),
        })
        .unwrap();
        ask(Request::FilterRequest { body: request.into() }).unwrap();
        let log = std::fs::read_to_string(dir.path().join(audit::LOG_FILE)).unwrap();
        assert!(log.contains("\"emails\":1") && !log.contains("carol") && !log.contains("secret.example"), "{log}");
        // Responses pass unchanged.
        let r = ask(Request::FilterResponse { body: json!({"x": "a@b.io"}).into() }).unwrap();
        assert_eq!(r, Reply::FilteredResponse(json!({"x": "a@b.io"}).into()));
    }

    #[test]
    fn bad_settings_say_what_to_fix() {
        let dir = RwLock::new(None);
        let none = PluginContext::default();
        assert!(configure(&dir, &json!([]), &none).unwrap_err().message.contains("JSON object"));
        assert!(configure(&dir, &json!({"audt": true}), &none).unwrap_err().message.contains("unknown setting"));
        assert!(configure(&dir, &json!({"audit": "yes"}), &none).unwrap_err().message.contains("true or false"));
        assert!(configure(&dir, &json!({"audit": true}), &none).unwrap_err().message.contains("data folder"));
        assert!(configure(&dir, &json!({"audit": false}), &none).is_ok());
    }
}
