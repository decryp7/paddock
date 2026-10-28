//! Limits, policy, audit and failure containment of the server.

use crate::common;

use common::*;
use paddock_jobs::FakeClock;
use paddock_log::Redactor;
use paddock_mcp::{
    AuditLog, CallCtx, Limits, Outcome, Policy, Server, Session, ToolDef, ToolOutput, page,
};
use serde_json::{Value, json};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn text(reply: &Value) -> String {
    reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn it_is_off_until_the_user_turns_it_on_and_follows_policy_changes_at_once() {
    let r = rig();
    r.server.set_policy(Policy::default());
    assert!(!r.server.policy().enabled);
    let s = Session::new("c");
    for method in ["initialize", "ping", "tools/list"] {
        let reply = rpc(
            &r.server,
            &s,
            json!({"jsonrpc":"2.0","id":1,"method":method}),
        );
        assert_eq!(reply["error"]["code"], -32600, "{method}");
        assert!(
            reply["error"]["message"]
                .as_str()
                .unwrap()
                .contains("switched off in demo")
        );
    }
    r.server.set_policy(Policy::read_only());
    let s = ready(&r.server);
    assert_eq!(
        text(&call(&r.server, &s, 2, "echo", json!({"text":"a"}))),
        "a"
    );
    r.server.set_policy(Policy::default());
    assert_eq!(
        call(&r.server, &s, 3, "echo", json!({"text":"a"}))["error"]["code"],
        -32600,
        "switching off ends service on a live session"
    );
}

#[test]
fn an_action_works_only_after_the_user_names_it() {
    let r = rig();
    let s = ready(&r.server);
    assert_eq!(
        call(&r.server, &s, 1, "delete", json!({}))["error"]["code"],
        -32602
    );
    r.server
        .set_policy(Policy::read_only().allow_action("delete"));
    assert_eq!(
        text(&call(&r.server, &s, 2, "delete", json!({}))),
        "deleted"
    );
    let only_echo = Policy {
        tools: Some(["echo".to_string()].into()),
        ..Policy::read_only().allow_action("delete")
    };
    r.server.set_policy(only_echo);
    assert_eq!(
        call(&r.server, &s, 3, "delete", json!({}))["error"]["code"],
        -32602,
        "the tool list narrows actions too"
    );
    assert_eq!(
        call(&r.server, &s, 4, "boom", json!({}))["error"]["code"],
        -32602,
        "and read-only tools"
    );
    let audit = r.audit.entries();
    assert_eq!(
        audit.iter().map(|e| e.outcome).collect::<Vec<_>>(),
        [
            Outcome::Refused,
            Outcome::Ok,
            Outcome::Refused,
            Outcome::Refused
        ]
    );
}

#[test]
fn tools_come_and_go_while_serving() {
    let r = rig();
    let s = ready(&r.server);
    r.server
        .add_tool(ToolDef::read_only(
            "late",
            "Added later.",
            json!({"type":"object"}),
            |_, _| Ok(ToolOutput::text("late")),
        ))
        .unwrap();
    assert_eq!(text(&call(&r.server, &s, 1, "late", json!({}))), "late");
    assert!(r.server.remove_tool("late") && !r.server.remove_tool("late"));
    assert_eq!(
        call(&r.server, &s, 2, "late", json!({}))["error"]["code"],
        -32602
    );
    let bad = ToolDef::read_only("bad name", "d", json!({"type":"object"}), |_, _| {
        Ok(ToolOutput::default())
    });
    assert!(r.server.add_tool(bad).is_err());
    assert!(
        r.server
            .add_resource(paddock_mcp::ResourceDef::new("nope", "n", || Ok(
                String::new()
            )))
            .is_err()
    );
}

#[test]
fn long_results_are_cut_and_long_lists_are_paged_with_the_cursor_handed_to_the_tool() {
    let r = rig();
    let seen = Arc::new(Mutex::new(Vec::<Option<String>>::new()));
    let seen2 = seen.clone();
    let items: Vec<String> = (0..5).map(|i| format!("item{i}")).collect();
    r.server
        .add_tool(
            ToolDef::read_only(
                "list",
                "Lists items.",
                json!({"type":"object"}),
                move |ctx: &CallCtx, _| {
                    seen2.lock().unwrap().push(ctx.cursor.clone());
                    let (page_items, next) = page::slice(&items, ctx.cursor.as_deref(), 2)
                        .map_err(paddock_mcp::ToolError)?;
                    Ok(ToolOutput {
                        text: page_items.join("\n"),
                        structured: None,
                        next_cursor: next,
                    })
                },
            )
            .paged(),
        )
        .unwrap();
    r.server
        .add_tool(
            ToolDef::read_only("big", "Big.", json!({"type":"object"}), |_, _| {
                Ok(ToolOutput::text("é".repeat(1000)))
            })
            .max_result_bytes(101),
        )
        .unwrap();
    let s = ready(&r.server);
    let first = call(&r.server, &s, 1, "list", json!({}));
    assert!(
        text(&first).contains("item0\nitem1") && text(&first).contains("cursor \"p2\""),
        "{first}"
    );
    assert_eq!(first["result"]["_meta"]["paddock/nextCursor"], "p2");
    let second = call(&r.server, &s, 2, "list", json!({"cursor":"p2"}));
    assert!(text(&second).contains("item2"));
    let last = call(&r.server, &s, 3, "list", json!({"cursor":"p4"}));
    assert!(text(&last).starts_with("item4") && last["result"].get("_meta").is_none());
    let bad = call(&r.server, &s, 4, "list", json!({"cursor":"zzz"}));
    assert_eq!(bad["result"]["isError"], true);
    assert!(text(&bad).contains("not a cursor"));
    assert_eq!(
        *seen.lock().unwrap(),
        [
            None,
            Some("p2".to_string()),
            Some("p4".to_string()),
            Some("zzz".to_string())
        ]
    );
    let big = call(&r.server, &s, 5, "big", json!({}));
    assert!(
        text(&big).starts_with("éé") && text(&big).contains("[truncated at 101 bytes"),
        "cut on a character boundary"
    );
    assert_eq!(big["result"]["_meta"]["paddock/truncated"], true);
}

#[test]
fn a_panicking_tool_is_contained_and_the_server_goes_on() {
    let r = rig();
    let s = ready(&r.server);
    let boom = call(&r.server, &s, 1, "boom", json!({}));
    assert_eq!(boom["result"]["isError"], true);
    assert!(text(&boom).contains("failed inside demo"));
    assert_eq!(
        text(&call(
            &r.server,
            &s,
            2,
            "echo",
            json!({"text":"still here"})
        )),
        "still here"
    );
    assert_eq!(r.audit.entries()[0].outcome, Outcome::Panicked);
}

#[test]
fn a_tool_that_hangs_is_cancelled_at_its_time_limit_and_slow_calls_cannot_pile_up() {
    let audit = Arc::new(AuditLog::new(10));
    let server = Server::new(
        "demo",
        "1",
        Arc::new(FakeClock::new()),
        audit.clone(),
        Arc::new(Redactor::new()),
    )
    .with_limits(Limits {
        max_abandoned: 1,
        ..Limits::default()
    });
    // The handler blocks until the test releases it, then reports whether its token had been cancelled.
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (report_tx, report_rx) = mpsc::channel::<bool>();
    // After reporting it stays inside the handler until the test lets it end, so the test controls when the abandoned call ends.
    let (end_tx, end_rx) = mpsc::channel::<()>();
    let release_rx = Mutex::new(release_rx);
    let report_tx = Mutex::new(report_tx);
    let end_rx = Mutex::new(end_rx);
    server
        .add_tool(
            ToolDef::read_only("slow", "Hangs.", json!({"type":"object"}), move |ctx, _| {
                let _ = release_rx.lock().unwrap().recv();
                let _ = report_tx.lock().unwrap().send(ctx.cancel.is_cancelled());
                let _ = end_rx.lock().unwrap().recv();
                Ok(ToolOutput::text("late"))
            })
            .timeout(Duration::from_millis(5)),
        )
        .unwrap();
    server
        .add_tool(ToolDef::read_only(
            "quick",
            "Quick.",
            json!({"type":"object"}),
            |_, _| Ok(ToolOutput::text("q")),
        ))
        .unwrap();
    server.set_policy(Policy::read_only());
    let s = ready(&server);
    let slow = call(&server, &s, 1, "slow", json!({}));
    assert_eq!(slow["result"]["isError"], true);
    assert!(text(&slow).contains("took longer than"), "{slow}");
    let busy = call(&server, &s, 2, "quick", json!({}));
    assert!(
        text(&busy).contains("busy finishing earlier slow calls"),
        "{busy}"
    );
    // Snapshot the audit before any retry: the log is a ring buffer, so later refusals would evict the timeout.
    let outcomes: Vec<Outcome> = audit.entries().iter().map(|e| e.outcome).take(2).collect();
    assert_eq!(outcomes, [Outcome::TimedOut, Outcome::Refused]);
    release_tx.send(()).unwrap();
    assert!(
        report_rx.recv().unwrap(),
        "the hung call saw its cancel token cancelled"
    );
    // Many refused retries while the abandoned call is still running (more than the audit log holds) stay refused.
    for id in 3..13 {
        let again = call(&server, &s, id, "quick", json!({}));
        assert!(
            text(&again).contains("busy finishing earlier slow calls"),
            "{again}"
        );
    }
    // Let the abandoned call end and wait for the server to uncount it (an event, not a time).
    end_tx.send(()).unwrap();
    server.wait_for_abandoned_calls();
    let reply = call(&server, &s, 13, "quick", json!({}));
    assert_eq!(text(&reply), "q");
}

#[test]
fn every_call_is_audited_with_redacted_arguments_and_the_servers_clock() {
    let r = rig();
    r.server
        .add_tool(ToolDef::read_only(
            "search",
            "Searches.",
            json!({"type":"object"}),
            |_, _| Ok(ToolOutput::text("found 3")),
        ))
        .unwrap();
    let s = ready(&r.server);
    call(
        &r.server,
        &s,
        1,
        "search",
        json!({"query":"errors","limit":5,"apiKey":"sk-live-123","content":"private file body","nested":{"password":"p"}}),
    );
    call(&r.server, &s, 2, "echo", json!({"text": 7}));
    let entries = r.audit.entries();
    assert_eq!(entries.len(), 2);
    let e = &entries[0];
    assert_eq!(
        (
            e.client.as_str(),
            e.tool.as_str(),
            e.outcome,
            e.result_bytes,
            e.at_ms,
            e.duration_ms
        ),
        ("tester", "search", Outcome::Ok, 7, 1_700_000_000_000, 0)
    );
    let args = e.arguments.to_string();
    for leaked in ["sk-live-123", "private file body", "\"p\""] {
        assert!(
            !args.contains(leaked),
            "{leaked} was written to the audit log: {args}"
        );
    }
    assert!(
        e.arguments["query"]
            .as_str()
            .unwrap()
            .starts_with("<omitted"),
        "a query is content: only its size is logged"
    );
    assert_eq!(e.arguments["limit"], 5);
    assert_eq!(entries[1].outcome, Outcome::ToolError);
    r.audit.clear();
    assert!(r.audit.entries().is_empty());
    assert!(r.server.now() > std::time::UNIX_EPOCH);
}

#[test]
fn the_client_name_comes_from_initialize_when_the_transport_has_none() {
    let r = rig();
    let s = Session::new("");
    rpc(
        &r.server,
        &s,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"Pitwall","version":"1"}}}),
    );
    call(&r.server, &s, 2, "echo", json!({"text":"x"}));
    assert_eq!(r.audit.entries()[0].client, "Pitwall");
    let keyed = Session::new("from-key");
    rpc(
        &r.server,
        &keyed,
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientInfo":{"name":"claims-to-be-someone-else"}}}),
    );
    call(&r.server, &keyed, 2, "echo", json!({"text":"x"}));
    assert_eq!(
        r.audit.entries()[1].client,
        "from-key",
        "a name the transport authenticated is not replaced"
    );
}

#[test]
fn an_overlong_line_has_an_answer_that_names_the_limit() {
    let r = rig();
    let reply: Value = serde_json::from_str(&r.server.too_long()).unwrap();
    assert_eq!(reply["error"]["code"], -32600);
    assert!(
        reply["error"]["message"]
            .as_str()
            .unwrap()
            .contains(&r.server.limits().max_line_bytes.to_string())
    );
}

#[test]
fn cancels_for_calls_that_never_come_are_remembered_only_up_to_a_bound() {
    let r = rig();
    let s = ready(&r.server);
    for id in 0..200 {
        let line =
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":id}})
                .to_string();
        assert!(r.server.handle(&s, &line).is_none());
    }
    for params in [json!({}), json!({"requestId": [1]})] {
        let line =
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":params}).to_string();
        assert!(
            r.server.handle(&s, &line).is_none(),
            "a cancel without a usable id is ignored"
        );
    }
    // The oldest cancels were forgotten, the newest still stand: call 199 is cancelled before it starts and gets no answer,
    // call 0 runs normally.
    assert!(call_line(&r, &s, 0).is_some());
    assert!(call_line(&r, &s, 199).is_none());
}

fn call_line(r: &Rig, s: &Session, id: i64) -> Option<String> {
    let line = json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":"echo","arguments":{"text":"x"}}}).to_string();
    r.server.handle(s, &line)
}

#[test]
fn a_cancel_that_arrives_while_the_tool_runs_stops_it_and_it_gets_no_answer() {
    let r = rig();
    let (started_tx, started_rx) = mpsc::channel::<()>();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let (seen_tx, seen_rx) = mpsc::channel::<bool>();
    let (started_tx, release_rx, seen_tx) = (
        Mutex::new(started_tx),
        Mutex::new(release_rx),
        Mutex::new(seen_tx),
    );
    r.server
        .add_tool(ToolDef::read_only(
            "hold",
            "Holds.",
            json!({"type":"object"}),
            move |ctx, _| {
                let _ = started_tx.lock().unwrap().send(());
                let _ = release_rx.lock().unwrap().recv();
                let _ = seen_tx.lock().unwrap().send(ctx.cancel.is_cancelled());
                Ok(ToolOutput::text("late"))
            },
        ))
        .unwrap();
    let s = ready(&r.server);
    let answer = std::thread::scope(|scope| {
        let call = scope.spawn(|| {
            let line =
                json!({"jsonrpc":"2.0","id":"c1","method":"tools/call","params":{"name":"hold"}})
                    .to_string();
            r.server.handle(&s, &line)
        });
        started_rx.recv().unwrap();
        let cancel =
            json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":"c1"}})
                .to_string();
        assert!(r.server.handle(&s, &cancel).is_none());
        call.join().unwrap()
    });
    assert!(answer.is_none(), "a cancelled call is not answered");
    release_tx.send(()).unwrap();
    assert!(seen_rx.recv().unwrap(), "the tool's token was cancelled");
    assert_eq!(
        r.audit.entries().last().unwrap().outcome,
        Outcome::Cancelled
    );
}
