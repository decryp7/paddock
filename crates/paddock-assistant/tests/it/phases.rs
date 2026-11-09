//! A turn's phases (ADR 0023 WP-4a, ER-18): a provider that knows them (queued behind another request, loading the model
//! with its elapsed time, generating) reports them through `LlmProvider::chat_phased`, and every window hears them as
//! `AssistantEvent::Phase` before the first word. A provider that knows none reports none. No sleeps.

use crate::support::*;
use paddock_assistant::*;
use std::sync::Arc;

/// Reports the phases of a cold start, then answers.
struct Cold;

impl LlmProvider for Cold {
    fn id(&self) -> &str {
        "cold"
    }
    fn display_name(&self) -> String {
        "Cold".into()
    }
    fn context_length(&self) -> usize {
        4096
    }
    fn chat(
        &self,
        req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<ChatResponse, ProviderError> {
        self.chat_phased(req, on_delta, &mut |_| {})
    }
    fn chat_phased(
        &self,
        _req: &ChatRequest,
        on_delta: &mut dyn FnMut(&str),
        on_phase: &mut dyn FnMut(ProviderPhase),
    ) -> Result<ChatResponse, ProviderError> {
        on_phase(ProviderPhase::Queued);
        on_phase(ProviderPhase::Loading { elapsed_ms: 0 });
        on_phase(ProviderPhase::Loading { elapsed_ms: 1500 });
        on_phase(ProviderPhase::Generating);
        on_delta("warm");
        Ok(text("warm now"))
    }
}

#[test]
fn a_provider_reports_its_phases_before_the_first_word() {
    let dir = tempfile::tempdir().unwrap();
    let clock = paddock_health::FakeClock::at_wall_ms(1_700_000_000_000);
    let store = Arc::new(ConversationStore::open(
        dir.path().join("a.json"),
        Arc::new(clock.clone()),
    ));
    let assistant = Assistant::new(
        Arc::new(Cold),
        Tools::none(),
        store,
        Arc::new(clock),
        AssistantConfig::new("Demo", "sys"),
    );
    let (sink, rx) = sink();
    assistant.send(ask("hello"), sink).unwrap();
    let events = until_end(&rx);
    let seen: Vec<String> = events
        .iter()
        .filter_map(|e| match e {
            AssistantEvent::Phase { phase } => Some(format!("{phase:?}")),
            AssistantEvent::Delta { text } => Some(format!("delta {text}")),
            _ => None,
        })
        .collect();
    assert_eq!(
        seen,
        [
            "Queued",
            "Loading { elapsed_ms: 0 }",
            "Loading { elapsed_ms: 1500 }",
            "Generating",
            "delta warm"
        ]
    );
    assert_eq!(done(&events).0, "warm now");
    // On the wire: `{"type":"phase","phase":{"kind":"loading","elapsedMs":1500}}`.
    let wire = serde_json::to_value(AssistantEvent::Phase {
        phase: ProviderPhase::Loading { elapsed_ms: 1500 },
    })
    .unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"type": "phase", "phase": {"kind": "loading", "elapsedMs": 1500}})
    );
}

#[test]
fn a_provider_that_knows_no_phases_reports_none() {
    let r = rig(vec![Step::Reply(vec!["hi"], text("hi"))]);
    let (_, events) = r.turn(ask("hello"));
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, AssistantEvent::Phase { .. }))
    );
}
