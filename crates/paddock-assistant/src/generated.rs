//! The generated TypeScript of this crate (checked on every test run: a stale file fails, `PADDOCK_WRITE_TS=1` rewrites it),
//! in the `workbench/assistant` entry point of `@decryp7/paddock-ui`:
//!
//! - `assistant.ts`: the wire types (ts-rs), the event name and the limits.
//! - `assistant-fixtures.ts`: a conversation with a tool call and the events of a turn, produced by this crate's own code.
//! - `assistant-commands.ts` (with the `tauri` feature): the adapter's command names.

use crate::approval::{Approval, ApprovalChoice};
use crate::conversation::*;
use crate::event::*;
use crate::service::{AssistantInfo, MAX_USER_CHARS, SendAck, SendRequest};
use crate::tool::ToolInfo;
use crate::types::{MAX_ATTACHMENTS, Role, ToolCall, Usage};
use paddock_commands::testing::{WriteMode, sync_generated};
use serde_json::json;
use std::path::{Path, PathBuf};
use ts_rs::TS;

const REGENERATE: &str =
    "PADDOCK_WRITE_TS=1 cargo test -p paddock-assistant --all-features --lib generated";

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../ui/projects/paddock-ui/workbench/assistant/generated")
}

fn sync(name: &str, text: &str) {
    sync_generated(&dir().join(name), text, WriteMode::from_env())
        .unwrap_or_else(|e| panic!("{e}"));
}

fn types() -> String {
    let decls = [
        Role::decl(),
        ToolCall::decl(),
        Usage::decl(),
        EndReason::decl(),
        ErrorKind::decl(),
        ContextItem::decl(),
        ToolOutcome::decl(),
        ApprovalChoice::decl(),
        Approval::decl(),
        StoredError::decl(),
        StoredMessage::decl(),
        Conversation::decl(),
        ConversationSummary::decl(),
        ToolInfo::decl(),
        AssistantInfo::decl(),
        crate::context::ContextLevel::decl(),
        crate::context::ContextUse::decl(),
        crate::types::Attachment::decl(),
        SendRequest::decl(),
        SendAck::decl(),
        crate::provider::ProviderPhase::decl(),
        AssistantEvent::decl(),
        Envelope::decl(),
    ];
    let body: String = decls
        .into_iter()
        .map(|d| format!("export {d}\n\n"))
        .collect();
    format!(
        "// Generated from the Rust wire types of paddock-assistant (ts-rs). Do not edit: {REGENERATE}.\n\n\
import type {{ AiState }} from '@decryp7/paddock-ui/shell/pitwall';\n\n\
{body}\
/** Every window hears it while a run goes on; the payload is an `Envelope`. */\n\
export const ASSISTANT_EVENT = '{EVENT}';\n\n\
/** The limits the engine enforces (the composer shows them; the engine checks again). */\n\
export const ASSISTANT_LIMITS = {{ userChars: {MAX_USER_CHARS}, contextItems: {MAX_CONTEXT_ITEMS}, contextChars: {MAX_CONTEXT_CHARS}, conversations: {MAX_CONVERSATIONS}, messages: {MAX_MESSAGES}, attachments: {MAX_ATTACHMENTS} }} as const;\n"
    )
}

fn fixtures() -> String {
    let user = StoredMessage::user(
        "Why did the build fail?",
        vec![ContextItem {
            id: "demo.selection".into(),
            label: "Selection".into(),
            text: "main.rs:12".into(),
        }],
        1_000,
    );
    let call = ToolCall {
        id: "call_1".into(),
        name: "find_events".into(),
        input: json!({"text": "build failed"}),
    };
    let conversation = Conversation {
        id: "c1-1".into(),
        title: "Why did the build fail?".into(),
        created_ms: 1_000,
        updated_ms: 4_000,
        messages: vec![
            user,
            StoredMessage::assistant("", vec![call], None, 2_000),
            StoredMessage::tool(
                "call_1",
                "find_events",
                "3 events",
                true,
                "3 events found".into(),
                Some(json!({"ids": [4, 7, 9]})),
                3_000,
            )
            .with_approval(Some(Approval::NotAsked)),
            StoredMessage::assistant(
                "The build failed at event 7: the linker ran out of memory.",
                vec![],
                Some(EndReason::Answered),
                4_000,
            ),
        ],
    };
    let usage = Usage {
        input_tokens: 120,
        output_tokens: 24,
    };
    let events = [
        AssistantEvent::Started {
            title: conversation.title.clone(),
        },
        AssistantEvent::ToolStarted {
            call_id: "call_1".into(),
            name: "find_events".into(),
            summary: "build failed".into(),
            title: None,
            write: false,
            destructive: false,
            input: json!({"query": "build failed"}),
        },
        AssistantEvent::ToolFinished {
            call_id: "call_1".into(),
            name: "find_events".into(),
            ok: true,
            summary: "3 events found".into(),
            output: Some("events 4, 7, 9".into()),
            data: Some(json!({"ids": [4, 7, 9]})),
            outcome: None,
            approval: Some(Approval::NotAsked),
        },
        AssistantEvent::Delta {
            text: "The build failed at event 7".into(),
        },
        AssistantEvent::Done {
            text: "The build failed at event 7: the linker ran out of memory.".into(),
            usage,
            end: EndReason::Answered,
            message: None,
        },
    ];
    let envelopes: Vec<Envelope> = events
        .into_iter()
        .enumerate()
        .map(|(i, event)| Envelope {
            conversation_id: "c1-1".into(),
            seq: i as u64 + 1,
            event,
        })
        .collect();
    format!(
        "// Generated by the tests of paddock-assistant from its own code. Do not edit: {REGENERATE}.\n\n\
import type {{ Conversation, Envelope }} from './assistant';\n\n\
/** A conversation with a read-only tool call, as `assistant_load` returns it. */\n\
export const FIXTURE_CONVERSATION: Conversation = {};\n\n\
/** The events of its last turn, in order. */\n\
export const FIXTURE_EVENTS: Envelope[] = {};\n",
        serde_json::to_string_pretty(&conversation).unwrap(),
        serde_json::to_string_pretty(&envelopes).unwrap(),
    )
}

#[test]
fn generated_typescript_is_current() {
    sync("assistant.ts", &types());
    sync("assistant-fixtures.ts", &fixtures());
}

#[cfg(feature = "tauri")]
#[test]
fn generated_command_names_are_current() {
    let text = paddock_commands::ts::names_module(
        &crate::tauri::COMMANDS,
        "ASSISTANT_COMMANDS",
        "AssistantCommand",
        REGENERATE,
    )
    .unwrap();
    sync("assistant-commands.ts", &text);
}

#[cfg(feature = "tauri")]
#[test]
fn every_command_is_async() {
    paddock_commands::testing::assert_async(
        &crate::tauri::COMMANDS,
        &[Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path()],
    );
}
