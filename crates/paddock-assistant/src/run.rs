//! The tool loop: one turn of the assistant. The model may call tools for a bounded number of rounds, tool calls, tokens and
//! wall time (on an injected clock); a stop ends it within a read slice; a call that cannot be read is reported to the model
//! and never run; a tool that writes waits for the user's decision first.

use crate::conversation::StoredMessage;
use crate::event::{AssistantEvent, EndReason};
use crate::provider::{LlmProvider, ProviderError};
use crate::tool::{Access, Tools, argument_problem};
use crate::types::{Cancel, ChatMessage, ChatRequest, StopReason, ToolCall, ToolSpec, Usage};
use paddock_health::Clock;
use serde_json::Value;
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// The limits of one turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    /// Most model calls (a tool round is one).
    pub max_rounds: usize,
    /// Most tool calls over all rounds.
    pub max_tool_calls: usize,
    /// Most tokens (prompts plus answers) over all rounds.
    pub max_tokens: u64,
    /// Longest wall time of the turn, approvals included.
    pub max_wall: Duration,
    /// Tokens one answer may have.
    pub max_answer_tokens: usize,
    /// Longest the turn waits for the user to decide on a write.
    pub approval_wait: Duration,
}

impl Default for Budgets {
    fn default() -> Self {
        Budgets {
            max_rounds: 6,
            max_tool_calls: 12,
            max_tokens: 48_000,
            max_wall: Duration::from_secs(600),
            max_answer_tokens: 1024,
            approval_wait: Duration::from_secs(120),
        }
    }
}

/// What the user decided about a write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Run it.
    Approved,
    /// Do not.
    Denied,
    /// The turn was stopped while waiting.
    Cancelled,
    /// Nobody answered in time.
    TimedOut,
}

#[derive(Default)]
struct GateState {
    waiting: Option<String>,
    decision: Option<bool>,
    /// When the wait for `waiting` ends, fixed when the call was announced (not when the turn got to waiting).
    deadline: Option<Duration>,
}

/// The rendezvous between a turn waiting for approval and `assistant_approve_tool`.
#[derive(Default)]
pub struct Gate {
    state: Mutex<GateState>,
    cv: Condvar,
}

/// How often a waiting turn looks at the clock and the stop flag besides being woken.
const SLICE: Duration = Duration::from_millis(50);

impl Gate {
    fn lock(&self) -> MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records the user's decision for `call_id`; `false` when that call is not waiting.
    pub fn decide(&self, call_id: &str, approved: bool) -> bool {
        let mut s = self.lock();
        if s.waiting.as_deref() != Some(call_id) || s.decision.is_some() {
            return false;
        }
        s.decision = Some(approved);
        self.cv.notify_all();
        true
    }

    /// Registers `call_id` as the call that is about to wait, before the UI hears of it: the decision can arrive as soon as
    /// `ApprovalNeeded` is emitted, which may be before the turn reaches [`Gate::wait`] (regression: `decide` then answered
    /// "not waiting" and the click was lost).
    /// The time limit starts here too, on the injected clock: a test (or a slow machine) that moves the clock right after the
    /// announcement must not push the deadline away.
    fn expect(&self, call_id: &str, clock: &dyn Clock, limit: Duration) {
        let mut s = self.lock();
        s.waiting = Some(call_id.into());
        s.decision = None;
        s.deadline = Some(clock.monotonic() + limit);
    }

    /// Wakes a waiting turn so it sees the stop flag.
    pub fn wake(&self) {
        let _guard = self.lock();
        self.cv.notify_all();
    }

    fn wait(&self, call_id: &str, cancel: &Cancel, clock: &dyn Clock, limit: Duration) -> Decision {
        let mut s = self.lock();
        // Normally set by `expect` already (an earlier decision is kept); a direct wait registers itself.
        if s.waiting.as_deref() != Some(call_id) {
            s.waiting = Some(call_id.into());
            s.deadline = None;
        }
        let deadline = *s.deadline.get_or_insert_with(|| clock.monotonic() + limit);
        let outcome = loop {
            if let Some(approved) = s.decision.take() {
                break if approved {
                    Decision::Approved
                } else {
                    Decision::Denied
                };
            }
            if cancel.is_cancelled() {
                break Decision::Cancelled;
            }
            if clock.monotonic() >= deadline {
                break Decision::TimedOut;
            }
            s = self
                .cv
                .wait_timeout(s, SLICE)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        };
        s.waiting = None;
        s.deadline = None;
        outcome
    }
}

/// Everything one turn needs.
pub struct Turn<'a> {
    /// The model.
    pub provider: &'a dyn LlmProvider,
    /// The app's tools.
    pub tools: &'a Tools,
    /// Time for the budgets.
    pub clock: &'a dyn Clock,
    /// The limits.
    pub budgets: Budgets,
    /// The system prompt.
    pub system: &'a str,
    /// The conversation so far, ending with the user's new message.
    pub history: Vec<ChatMessage>,
    /// The user allowed tools that write for this message.
    pub allow_write: bool,
    /// Stops the turn.
    pub cancel: &'a Cancel,
    /// Where approvals are answered.
    pub gate: &'a Gate,
}

/// How a turn ended.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnEnd {
    /// The final text.
    pub text: String,
    /// Tokens used.
    pub usage: Usage,
    /// Why it ended.
    pub end: EndReason,
    /// What happened, for the budgets.
    pub message: Option<String>,
}

/// Where a turn reports.
pub struct Hooks<'a> {
    /// A progress event.
    pub emit: &'a mut dyn FnMut(AssistantEvent),
    /// A message to keep (called in order, as the turn goes).
    pub keep: &'a mut dyn FnMut(StoredMessage),
}

fn estimate(req: &ChatRequest, answer: &str) -> Usage {
    let input: usize = req.system.chars().count()
        + req
            .messages
            .iter()
            .map(|m| m.content.chars().count())
            .sum::<usize>();
    Usage {
        input_tokens: (input / 4) as u64,
        output_tokens: (answer.chars().count() / 4) as u64,
    }
}

fn cap(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push_str(" …[cut]");
    out
}

struct Finished {
    output: crate::tool::ToolOutput,
}

/// Runs the turn. `Err` is a provider failure (what was generated until then has been kept and the caller reports it).
pub fn run_turn(
    turn: &Turn<'_>,
    hooks: &mut Hooks<'_>,
    now_ms: &dyn Fn() -> u64,
) -> Result<TurnEnd, ProviderError> {
    let start = turn.clock.monotonic();
    let b = turn.budgets;
    let offered: Vec<ToolSpec> = turn.tools.offered(turn.allow_write);
    let mut messages = turn.history.clone();
    let mut usage = Usage::default();
    let mut calls_total = 0usize;
    let mut per_tool: HashMap<String, usize> = HashMap::new();
    // Why tools are off from now on (None: on).
    let mut tools_off: Option<EndReason> = if offered.is_empty() {
        Some(EndReason::Answered)
    } else {
        None
    };
    let mut round = 0usize;
    let end = |text: String, usage: Usage, end: EndReason, message: Option<String>| TurnEnd {
        text,
        usage,
        end,
        message,
    };
    loop {
        if turn.cancel.is_cancelled() {
            return Ok(end(String::new(), usage, EndReason::Cancelled, None));
        }
        if turn.clock.monotonic().saturating_sub(start) > b.max_wall {
            return Ok(budget_end(
                usage,
                EndReason::BudgetTime,
                "The assistant ran out of time for this question.",
            ));
        }
        if usage.total() >= b.max_tokens {
            return Ok(budget_end(
                usage,
                EndReason::BudgetTokens,
                "The assistant used its token budget for this question.",
            ));
        }
        if round + 1 >= b.max_rounds.max(1) && tools_off.is_none() {
            tools_off = Some(EndReason::BudgetRounds);
        }
        let mut system = turn.system.to_string();
        if let Some(reason) = tools_off.filter(|r| *r != EndReason::Answered) {
            let why = if reason == EndReason::BudgetRounds {
                "tool rounds"
            } else {
                "tool calls"
            };
            system.push_str(&format!("\nYou have used all {why}. Answer now from the evidence gathered; do not call tools."));
        }
        let req = ChatRequest {
            system,
            messages: messages.clone(),
            max_tokens: b.max_answer_tokens,
            tools: if tools_off.is_some() {
                vec![]
            } else {
                offered.clone()
            },
            temperature: None,
            json_schema: None,
            cancel: turn.cancel.clone(),
        };
        let mut streamed = String::new();
        let response = turn.provider.chat(&req, &mut |d| {
            streamed.push_str(d);
            (hooks.emit)(AssistantEvent::Delta {
                text: d.to_string(),
            });
        });
        let response = match response {
            Ok(r) => r,
            Err(ProviderError::Cancelled) => {
                keep_partial(hooks, &streamed, EndReason::Cancelled, now_ms());
                return Ok(end(streamed, usage, EndReason::Cancelled, None));
            }
            Err(e) => {
                keep_partial(hooks, &streamed, EndReason::Failed, now_ms());
                return Err(e);
            }
        };
        usage.add(
            response
                .usage
                .filter(|u| u.total() > 0)
                .unwrap_or_else(|| estimate(&req, &response.text)),
        );
        if response.stop_reason == StopReason::Cancelled {
            keep_partial(hooks, &response.text, EndReason::Cancelled, now_ms());
            return Ok(end(response.text, usage, EndReason::Cancelled, None));
        }
        if response.tool_calls.is_empty() {
            if response.text.trim().is_empty() {
                keep_partial(hooks, "", EndReason::Failed, now_ms());
                return Err(ProviderError::Failed(
                    "The assistant gave an empty answer. Try again.".into(),
                ));
            }
            (hooks.keep)(StoredMessage::assistant(
                response.text.clone(),
                vec![],
                Some(EndReason::Answered),
                now_ms(),
            ));
            return Ok(end(response.text, usage, EndReason::Answered, None));
        }
        if let Some(reason) = tools_off {
            // Tools were off and the model asked for them anyway: keep what it said and end with a clear message.
            let text = response.text.clone();
            (hooks.keep)(StoredMessage::assistant(
                text.clone(),
                vec![],
                Some(reason),
                now_ms(),
            ));
            let message = if reason == EndReason::BudgetRounds {
                format!(
                    "The assistant used its {} tool rounds without answering.",
                    b.max_rounds
                )
            } else {
                format!(
                    "The assistant used its {} tool calls without answering.",
                    b.max_tool_calls
                )
            };
            return Ok(end(text, usage, reason, Some(message)));
        }
        (hooks.keep)(StoredMessage::assistant(
            response.text.clone(),
            response.tool_calls.clone(),
            None,
            now_ms(),
        ));
        messages.push(ChatMessage::assistant(
            response.text.clone(),
            response.tool_calls.clone(),
        ));
        for call in &response.tool_calls {
            if turn.cancel.is_cancelled() {
                return Ok(end(response.text, usage, EndReason::Cancelled, None));
            }
            if turn.clock.monotonic().saturating_sub(start) > b.max_wall {
                return Ok(budget_end(
                    usage,
                    EndReason::BudgetTime,
                    "The assistant ran out of time for this question.",
                ));
            }
            let Finished { output } = dispatch(turn, hooks, call, &mut calls_total, &mut per_tool);
            let text = output.text.clone();
            (hooks.keep)(StoredMessage::tool(
                &call.id,
                &call.name,
                text.clone(),
                !output.error,
                output.summary.clone(),
                output.data.clone(),
                now_ms(),
            ));
            messages.push(ChatMessage::tool_result(call.id.clone(), text));
        }
        if calls_total >= b.max_tool_calls && tools_off.is_none() {
            tools_off = Some(EndReason::BudgetToolCalls);
        }
        round += 1;
    }
}

fn budget_end(usage: Usage, end: EndReason, message: &str) -> TurnEnd {
    TurnEnd {
        text: String::new(),
        usage,
        end,
        message: Some(message.into()),
    }
}

fn keep_partial(hooks: &mut Hooks<'_>, text: &str, end: EndReason, at_ms: u64) {
    if !text.is_empty() {
        (hooks.keep)(StoredMessage::assistant(text, vec![], Some(end), at_ms));
    }
}

fn dispatch(
    turn: &Turn<'_>,
    hooks: &mut Hooks<'_>,
    call: &ToolCall,
    total: &mut usize,
    per_tool: &mut HashMap<String, usize>,
) -> Finished {
    let tool = turn.tools.get(&call.name);
    let access = tool.map_or(Access::ReadOnly, |t| t.access());
    let write = access.writes();
    let summary = tool.map_or_else(
        || cap(&call.input.to_string(), 120),
        |t| cap(&t.describe(&call.input), 160),
    );
    (hooks.emit)(AssistantEvent::ToolStarted {
        call_id: call.id.clone(),
        name: call.name.clone(),
        summary: summary.clone(),
        write,
        destructive: access.destructive(),
    });
    let output = decide_and_run(turn, hooks, call, tool, access, &summary, total, per_tool);
    (hooks.emit)(AssistantEvent::ToolFinished {
        call_id: call.id.clone(),
        name: call.name.clone(),
        ok: !output.error,
        summary: output.summary.clone(),
        data: output.data.clone(),
    });
    Finished { output }
}

#[allow(clippy::too_many_arguments)]
fn decide_and_run(
    turn: &Turn<'_>,
    hooks: &mut Hooks<'_>,
    call: &ToolCall,
    tool: Option<&std::sync::Arc<dyn crate::tool::AssistantTool>>,
    access: Access,
    summary: &str,
    total: &mut usize,
    per_tool: &mut HashMap<String, usize>,
) -> crate::tool::ToolOutput {
    use crate::tool::ToolOutput;
    if call.is_malformed() {
        let raw = call
            .input
            .get("_invalid_json")
            .and_then(Value::as_str)
            .unwrap_or("");
        (hooks.emit)(AssistantEvent::Notice {
            message: "The assistant sent a tool call that could not be read; it was not run."
                .into(),
        });
        return ToolOutput::failed(format!(
            "the tool call could not be read ({}). Call a tool again with valid JSON arguments.",
            cap(raw, 120)
        ));
    }
    let offered = tool.filter(|t| turn.allow_write || !t.access().writes());
    let Some(tool) = offered else {
        return ToolOutput::failed(format!("there is no tool called {}", call.name));
    };
    if let Some(problem) = argument_problem(&tool.spec(), &call.input) {
        return ToolOutput::failed(format!("the call to {} was not run: {problem}", call.name));
    }
    let budget = tool.budget();
    let used = per_tool.entry(call.name.clone()).or_default();
    if *used >= budget.max_calls || *total >= turn.budgets.max_tool_calls {
        return ToolOutput::failed(
            "the tool budget for this question is used up; answer from what you have",
        );
    }
    *used += 1;
    *total += 1;
    if access.writes() {
        turn.gate
            .expect(&call.id, turn.clock, turn.budgets.approval_wait);
        (hooks.emit)(AssistantEvent::ApprovalNeeded {
            call_id: call.id.clone(),
            name: call.name.clone(),
            summary: summary.into(),
            destructive: access.destructive(),
            input: call.input.clone(),
        });
        match turn.gate.wait(
            &call.id,
            turn.cancel,
            turn.clock,
            turn.budgets.approval_wait,
        ) {
            Decision::Approved => {}
            Decision::Denied => {
                return ToolOutput::failed("the user declined this action; it was not run");
            }
            Decision::Cancelled => {
                return ToolOutput::failed("the turn was stopped; the action was not run");
            }
            Decision::TimedOut => {
                return ToolOutput::failed(
                    "the user did not answer in time; the action was not run",
                );
            }
        }
    }
    let mut output = match catch_unwind(AssertUnwindSafe(|| tool.call(&call.input, turn.cancel))) {
        Ok(o) => o,
        Err(_) => ToolOutput::failed(format!("the tool {} failed unexpectedly", call.name)),
    };
    output.text = cap(&output.text, budget.max_output_chars);
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_testkit::FakeClock;

    /// Regression (CI, Linux): the decision can arrive the moment `ApprovalNeeded` is emitted, before the turn is in `wait`.
    #[test]
    fn a_decision_made_between_the_announcement_and_the_wait_is_not_lost() {
        let gate = Gate::default();
        assert!(!gate.decide("c1", true), "nothing is expected yet");
        let clock = FakeClock::new();
        gate.expect("c1", &clock, Duration::from_secs(60));
        assert!(
            !gate.decide("other", true),
            "only the announced call can be decided"
        );
        assert!(
            gate.decide("c1", true),
            "announced, so the click counts even before the wait starts"
        );
        assert!(!gate.decide("c1", false), "one decision per call");
        let cancel = Cancel::default();
        assert_eq!(
            gate.wait("c1", &cancel, &clock, Duration::from_secs(60)),
            Decision::Approved
        );
        assert!(!gate.decide("c1", true), "nothing waits any more");
    }

    /// Regression (CI): the clock moved right after the announcement; the limit still ran from the announcement.
    #[test]
    fn the_limit_runs_from_the_announcement_not_from_the_start_of_the_wait() {
        let gate = Gate::default();
        let clock = FakeClock::new();
        gate.expect("c1", &clock, Duration::from_secs(60));
        clock.advance(Duration::from_secs(61));
        let cancel = Cancel::default();
        assert_eq!(
            gate.wait("c1", &cancel, &clock, Duration::from_secs(60)),
            Decision::TimedOut
        );
    }
}
