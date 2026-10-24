//! [`FakeRunner`]: a scripted stand-in for the seam that runs OS commands.
//!
//! An app's seam is its own trait (Dyno's `Runner`, a service manager's `launchctl` calls); the app implements it for
//! `FakeRunner` in its test module and forwards to [`FakeRunner::run`]. Calls are matched by program and a prefix of the
//! arguments; the first rule that matches answers, replies queued with [`Rule::then`] are used in order and the last one
//! repeats. A call no rule matches is an error reply (and is recorded), so a test never passes by accident on an
//! unscripted command. Nothing is started.

use std::sync::{Arc, Mutex, MutexGuard};

/// What a command answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The command ran: its success, standard output and standard error.
    Ran {
        /// Whether it exited with status 0.
        success: bool,
        /// Standard output.
        stdout: String,
        /// Standard error.
        stderr: String,
    },
    /// The command could not be started or timed out (the message an app's seam turns into its error).
    Failed(String),
}

impl Reply {
    /// Exit status 0 with `stdout`.
    pub fn ok(stdout: &str) -> Reply {
        Reply::Ran {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    /// A non-zero exit with `stderr`.
    pub fn fails(stderr: &str) -> Reply {
        Reply::Ran {
            success: false,
            stdout: String::new(),
            stderr: stderr.to_string(),
        }
    }

    /// The command could not run.
    pub fn cannot_start(message: &str) -> Reply {
        Reply::Failed(message.to_string())
    }
}

/// One recorded call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The program as asked for.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
}

impl Call {
    /// `program arg arg`, for assertions and messages.
    pub fn line(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug)]
struct RuleData {
    program: String,
    args: Vec<String>,
    replies: Vec<Reply>,
    used: usize,
}

#[derive(Debug, Default)]
struct State {
    rules: Vec<RuleData>,
    calls: Vec<Call>,
}

/// The scripted runner. Clones share the script and the recorded calls, so a test keeps one clone to assert with.
#[derive(Debug, Clone, Default)]
pub struct FakeRunner(Arc<Mutex<State>>);

/// A rule being scripted by [`FakeRunner::on`].
#[derive(Debug)]
pub struct Rule<'a> {
    runner: &'a FakeRunner,
    index: usize,
}

impl FakeRunner {
    /// A runner with no rules.
    pub fn new() -> FakeRunner {
        FakeRunner::default()
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Scripts `program` called with arguments that start with `args_prefix` (an empty prefix matches any arguments);
    /// the reply is given with [`Rule::reply`]. Earlier rules win, so script the specific ones first.
    pub fn on(&self, program: &str, args_prefix: &[&str]) -> Rule<'_> {
        let mut s = self.state();
        s.rules.push(RuleData {
            program: program.to_string(),
            args: args_prefix.iter().map(|a| a.to_string()).collect(),
            replies: Vec::new(),
            used: 0,
        });
        Rule {
            runner: self,
            index: s.rules.len() - 1,
        }
    }

    /// Answers a call like the real seam would: the reply of the first matching rule, or an error naming the unscripted command.
    pub fn run(&self, program: &str, args: &[&str]) -> Reply {
        let mut s = self.state();
        s.calls.push(Call {
            program: program.to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
        });
        let hit = s.rules.iter_mut().find(|r| {
            r.program == program
                && !r.replies.is_empty()
                && args.len() >= r.args.len()
                && r.args.iter().zip(args).all(|(want, got)| want == got)
        });
        match hit {
            Some(rule) => {
                let reply = rule.replies[rule.used.min(rule.replies.len() - 1)].clone();
                rule.used += 1;
                reply
            }
            None => Reply::Failed(format!(
                "FakeRunner: unscripted command `{}`",
                s.calls.last().map(Call::line).unwrap_or_default()
            )),
        }
    }

    /// Every call so far, in order.
    pub fn calls(&self) -> Vec<Call> {
        self.state().calls.clone()
    }

    /// Every call so far as `program arg arg` lines.
    pub fn lines(&self) -> Vec<String> {
        self.state().calls.iter().map(Call::line).collect()
    }

    /// How many calls started with `program` and `args_prefix`.
    pub fn count(&self, program: &str, args_prefix: &[&str]) -> usize {
        self.state()
            .calls
            .iter()
            .filter(|c| c.program == program && c.args.len() >= args_prefix.len() && c.args.iter().zip(args_prefix).all(|(a, b)| a == b))
            .count()
    }
}

impl Rule<'_> {
    /// The reply of the next call that matches; call it again to queue further replies (the last one repeats).
    pub fn reply(self, reply: Reply) -> Self {
        self.runner.state().rules[self.index].replies.push(reply);
        self
    }

    /// Alias of [`reply`](Rule::reply) that reads well for a second answer: `.reply(a).then(b)`.
    pub fn then(self, reply: Reply) -> Self {
        self.reply(reply)
    }
}
