//! The heart of the log: takes an event (target, message, properties), applies the redaction rules, keeps breadcrumbs,
//! de-duplicates repeated errors, formats CLEF and hands the line to the sinks. `tracing` events and `log` records both
//! end here, through [`ClefLayer`].

use crate::breadcrumbs::{
    BREADCRUMBS_PER_ERROR, Breadcrumb, Breadcrumbs, DEDUP_WINDOW, Dedup, ErrorDedup, bounded_json,
};
use crate::clef::{Enrichment, format_event};
use crate::clock::Clock;
use crate::error::lock;
use crate::files::LineSink;
use crate::redact::Redact;
use serde_json::{Map, Value};
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record as SpanRecord};
use tracing::{Event, Subscriber};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

/// Field carrying a bridged `log` record's key/values as a JSON object.
pub(crate) const KV_FIELD: &str = "log.kv";

pub(crate) struct Core {
    pub(crate) sinks: Vec<Box<dyn LineSink>>,
    pub(crate) redact: Arc<dyn Redact>,
    pub(crate) crumbs: Mutex<Breadcrumbs>,
    pub(crate) dedup: Mutex<ErrorDedup>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) enrich: Enrichment,
}

impl Core {
    fn write(&self, level: log::Level, target: &str, message: &str, props: Map<String, Value>) {
        let line = format_event(
            self.clock.now(),
            level,
            target,
            message,
            props,
            &self.enrich,
        );
        for sink in &self.sinks {
            sink.write_line(level, &line);
        }
    }

    /// Processes one event (see the module docs).
    pub(crate) fn emit(
        &self,
        level: log::Level,
        target: &str,
        message: &str,
        props: Map<String, Value>,
    ) {
        let now = self.clock.now();
        let fatal = props.get("Severity").and_then(Value::as_str) == Some("Fatal");
        let mut props: Map<String, Value> = props
            .into_iter()
            .filter_map(|(k, v)| self.redact.field(&k, v).map(|v| (k, v)))
            .collect();
        let message = self.redact.message(message);
        if let Some(b) = Breadcrumb::from_props(now, &props) {
            lock(&self.crumbs).push(b);
        }
        if level == log::Level::Error || fatal {
            let context = props
                .get("SourceContext")
                .and_then(Value::as_str)
                .unwrap_or(target);
            let key = format!("{context}\u{1f}{message}");
            match lock(&self.dedup).check(&key, now) {
                Dedup::Suppress if !fatal => return,
                Dedup::LogAfterRepeats(n) => self.summary(level, target, &message, n),
                _ => {}
            }
            let window = props.get("Window").and_then(Value::as_str);
            let crumbs = lock(&self.crumbs).recent_for(window, BREADCRUMBS_PER_ERROR);
            if !crumbs.is_empty() && !props.contains_key("Breadcrumbs") {
                props.insert("Breadcrumbs".into(), Value::from(bounded_json(crumbs)));
            }
        }
        self.write(level, target, &message, props);
    }

    fn summary(&self, level: log::Level, target: &str, message: &str, n: u64) {
        let mut props = Map::new();
        props.insert("Repeats".into(), Value::from(n));
        props.insert(
            "DedupWindowSeconds".into(),
            Value::from(DEDUP_WINDOW.num_seconds()),
        );
        self.write(
            level,
            target,
            &format!(
                "{message} (repeated {n} times in {}s)",
                DEDUP_WINDOW.num_seconds()
            ),
            props,
        );
    }

    /// Writes the pending "repeated n times" summaries (call at exit and before a report is collected).
    pub(crate) fn flush_repeats(&self) {
        for (key, n) in lock(&self.dedup).drain_repeats() {
            let (context, message) = key.split_once('\u{1f}').unwrap_or(("", key.as_str()));
            self.summary(log::Level::Error, context, message, n);
        }
    }
}

/// Fields of a span, kept in its extensions.
struct SpanFields(Map<String, Value>);

#[derive(Default)]
struct FieldVisitor {
    message: Option<String>,
    target: Option<String>,
    kv: Option<String>,
    fields: Map<String, Value>,
}

impl FieldVisitor {
    fn put(&mut self, field: &Field, value: Value) {
        match field.name() {
            "message" => {
                self.message = Some(
                    value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string()),
                )
            }
            "log.target" => self.target = value.as_str().map(str::to_string),
            KV_FIELD => self.kv = value.as_str().map(str::to_string),
            // Where a bridged record came from: not properties.
            "log.module_path" | "log.file" | "log.line" => {}
            name => {
                self.fields.insert(name.to_string(), value);
            }
        }
    }
}

impl Visit for FieldVisitor {
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.put(field, Value::from(value));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.put(field, Value::from(value));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.put(field, Value::from(value));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.put(field, Value::from(value));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, Value::from(value));
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.put(field, Value::from(format!("{value:?}")));
    }
}

pub(crate) fn log_level(level: &tracing::Level) -> log::Level {
    match *level {
        tracing::Level::ERROR => log::Level::Error,
        tracing::Level::WARN => log::Level::Warn,
        tracing::Level::INFO => log::Level::Info,
        tracing::Level::DEBUG => log::Level::Debug,
        tracing::Level::TRACE => log::Level::Trace,
    }
}

/// The `tracing` layer that formats every event as CLEF through [`Core`]. The fields of the enclosing spans become
/// properties of every event (IPC spans carry `Window`, `OperationId` and `Command`).
pub(crate) struct ClefLayer(pub(crate) Arc<Core>);

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for ClefLayer {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut v = FieldVisitor::default();
        attrs.record(&mut v);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(v.fields));
        }
    }

    fn on_record(&self, id: &Id, values: &SpanRecord<'_>, ctx: Context<'_, S>) {
        let mut v = FieldVisitor::default();
        values.record(&mut v);
        if let Some(span) = ctx.span(id)
            && let Some(f) = span.extensions_mut().get_mut::<SpanFields>()
        {
            f.0.extend(v.fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut v = FieldVisitor::default();
        event.record(&mut v);
        let mut props = Map::new();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(f) = span.extensions().get::<SpanFields>() {
                    props.extend(f.0.clone());
                }
            }
        }
        props.extend(std::mem::take(&mut v.fields));
        if let Some(Ok(Value::Object(kv))) = v.kv.as_deref().map(serde_json::from_str::<Value>) {
            props.extend(kv);
        }
        let meta = event.metadata();
        self.0.emit(
            log_level(meta.level()),
            v.target.as_deref().unwrap_or(meta.target()),
            v.message.as_deref().unwrap_or_default(),
            props,
        );
    }
}
