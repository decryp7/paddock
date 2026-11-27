//! What the gate writes down about a step before it ends it (ER-27): the process list of its tree and, where the OS gives one, a stack
//! sample of each live process, so the log can say where a hung step was (docs/research/2026-11-19-gate-hang-detection.md section 3.6).
//!
//! - macOS: `sample <pid> 1 -file <file>`: every thread's call graph, 1.1 s, no root for our own child (measured 2026-11-19).
//! - Linux: `eu-stack -p <pid>` when it is installed (user stacks; `/proc/<pid>/stack` is the kernel's and needs ptrace rights);
//!   the process list otherwise.
//! - Windows: best effort, the process list only (`tasklist`): there is no built-in stack tool (procdump is a download). Documented
//!   here, not a gap that is hidden: the capture's own note says "no stack tool on this OS".
//!
//! At most [`MAX_PROCESSES`] processes are sampled, the root first. The capture never changes what it looks at (ER-45) and never keeps
//! a kill waiting longer than [`STACK_CAPTURE_LIMIT`]; a failed look is written into the file as the cause, never a silent gap.

use crate::ci_tool::{Output, ToolCall};
use crate::throttle::STACK_CAPTURE_LIMIT;
use paddock_proc::ProcessId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The most processes of a tree that are sampled (the root and the first descendants): a stack of every compiler of a wide build
/// would take minutes and say the same.
pub const MAX_PROCESSES: usize = 8;

/// How long one look (one `sample`, `eu-stack`, `ps`) may take: `sample` takes 1.1 s itself (measured), `eu-stack` of a large
/// process a few seconds; a look that takes longer is written as a timeout.
const LOOK_LIMIT: Duration = Duration::from_secs(15);

/// The most of one look's output that is kept in the file.
const KEEP_BYTES: usize = 64 * 1024;

/// The OS families the capture knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// macOS.
    Mac,
    /// Linux.
    Linux,
    /// Windows.
    Windows,
    /// Anything else: the process list only.
    Other,
}

/// The OS this program runs on.
pub fn this_os() -> Os {
    if cfg!(target_os = "macos") {
        Os::Mac
    } else if cfg!(target_os = "linux") {
        Os::Linux
    } else if cfg!(windows) {
        Os::Windows
    } else {
        Os::Other
    }
}

/// One look: the program, its arguments, and the file it writes by itself (`sample -file`), if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Look {
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// A file the program writes its report to (read back into the capture), when it does not print it.
    pub writes: Option<PathBuf>,
}

/// The looks for `pids` on `os`: first the process list, then a stack of each process where the OS has a tool (`have` says whether a
/// program is installed). `sample_dir` holds the files `sample` writes. Pure.
pub fn looks(os: Os, pids: &[u32], sample_dir: &Path, have: &dyn Fn(&str) -> bool) -> Vec<Look> {
    let pids: Vec<u32> = pids.iter().copied().take(MAX_PROCESSES).collect();
    let list = pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
    let mut out = Vec::new();
    match os {
        Os::Windows => {
            // tasklist takes one filter per pid.
            for p in &pids {
                out.push(Look {
                    program: "tasklist".into(),
                    args: vec!["/FI".into(), format!("PID eq {p}"), "/V".into()],
                    writes: None,
                });
            }
        }
        _ => out.push(Look {
            program: "ps".into(),
            args: ["-o", "pid,ppid,stat,etime,time,command", "-p", &list].map(String::from).to_vec(),
            writes: None,
        }),
    }
    for p in &pids {
        match os {
            Os::Mac if have("sample") => {
                let file = sample_dir.join(format!("sample-{p}.txt"));
                out.push(Look {
                    program: "sample".into(),
                    args: vec![p.to_string(), "1".into(), "-file".into(), file.to_string_lossy().to_string()],
                    writes: Some(file),
                });
            }
            Os::Linux if have("eu-stack") => out.push(Look {
                program: "eu-stack".into(),
                args: vec!["-p".into(), p.to_string()],
                writes: None,
            }),
            _ => {}
        }
    }
    out
}

/// What a capture did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capture {
    /// The file with the process list and the stacks.
    pub file: PathBuf,
    /// One line for the log: how many processes, which tool gave stacks (or that none exists here).
    pub note: String,
}

/// `root` and its descendants, root first, at most [`MAX_PROCESSES`], each by identity (the pid is only read from an identity).
pub fn tree(root: ProcessId) -> Vec<ProcessId> {
    std::iter::once(root)
        .chain(paddock_proc::descendants_of(root))
        .take(MAX_PROCESSES)
        .collect()
}

/// The tool the OS gives for stacks, for the note.
fn stack_tool(os: Os, have: &dyn Fn(&str) -> bool) -> &'static str {
    match os {
        Os::Mac if have("sample") => "sample",
        Os::Linux if have("eu-stack") => "eu-stack",
        Os::Windows => "no stack tool on Windows (process list only)",
        _ => "no stack tool installed (process list only)",
    }
}

/// Writes `<dir>/<name>.stacks.txt`: why, the process list and the stacks of `pids` (read before the kill by the caller; a process that
/// has gone since is written as the look's own failure). Never fails the caller: a look that fails is part of the file.
pub fn capture(
    os: Os,
    pids: &[u32],
    why: &str,
    dir: &Path,
    name: &str,
    env: &HashMap<String, String>,
    have: &dyn Fn(&str) -> bool,
) -> Capture {
    let file = dir.join(format!("{name}.stacks.txt"));
    let mut text = format!("# {why}\n# processes: {pids:?}\n# stacks: {}\n", stack_tool(os, have));
    let started = std::time::Instant::now();
    for look in looks(os, pids, dir, have) {
        if started.elapsed() > STACK_CAPTURE_LIMIT {
            text.push_str("\n$ (the capture's own time limit ran out; the remaining looks were not made)\n");
            break;
        }
        text.push_str(&format!("\n$ {} {}\n", look.program, look.args.join(" ")));
        let ran = ToolCall::new(look.program.clone())
            .args(look.args.clone())
            .pass(crate::child_env::PASS.iter().copied())
            .timeout(LOOK_LIMIT)
            .output(Output::Capture)
            .run(env, &mut std::io::sink());
        match ran {
            Ok(r) => {
                text.push_str(&format!("({})\n", r.how));
                let mut body = r.stdout;
                if let Some(f) = &look.writes {
                    body.push_str(
                        &std::fs::read_to_string(f).unwrap_or_else(|e| format!("(the report {} was not written: {e})\n", f.display())),
                    );
                }
                if body.len() > KEEP_BYTES {
                    let mut cut = KEEP_BYTES;
                    while !body.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    body.truncate(cut);
                    body.push_str("\n(cut)\n");
                }
                text.push_str(&body);
                if !r.stderr.trim().is_empty() {
                    text.push_str(&format!("(stderr) {}\n", r.stderr.trim()));
                }
            }
            Err(e) => text.push_str(&format!("(could not run: {e})\n")),
        }
    }
    let _ = std::fs::create_dir_all(dir);
    let note = match std::fs::write(&file, &text) {
        Ok(()) => format!(
            "{} process(es) looked at, stacks by {}; written to {}",
            pids.len(),
            stack_tool(os, have),
            file.display()
        ),
        Err(e) => format!("the stack capture could not be written to {}: {e}", file.display()),
    };
    Capture { file, note }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have(names: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |p| names.contains(&p)
    }

    #[test]
    fn every_os_gets_the_process_list_and_stacks_where_it_has_a_tool() {
        let dir = Path::new("/logs");
        let mac = looks(Os::Mac, &[7, 8], dir, &have(&["sample"]));
        assert_eq!(mac[0].program, "ps");
        assert!(mac[0].args.contains(&"7,8".to_string()));
        let samples: Vec<_> = mac.iter().filter(|l| l.program == "sample").collect();
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[0].writes.as_deref(), Some(Path::new("/logs/sample-7.txt")));
        // Linux: eu-stack when installed, the process list alone otherwise.
        let with = looks(Os::Linux, &[7], dir, &have(&["eu-stack"]));
        assert_eq!(with.iter().map(|l| l.program.as_str()).collect::<Vec<_>>(), ["ps", "eu-stack"]);
        let without = looks(Os::Linux, &[7], dir, &have(&[]));
        assert_eq!(without.iter().map(|l| l.program.as_str()).collect::<Vec<_>>(), ["ps"]);
        // Windows: best effort, tasklist per process, no stacks.
        let win = looks(Os::Windows, &[7, 8], dir, &have(&["sample", "eu-stack"]));
        assert_eq!(win.iter().map(|l| l.program.as_str()).collect::<Vec<_>>(), ["tasklist", "tasklist"]);
    }

    #[test]
    fn no_more_than_the_first_processes_are_looked_at() {
        let pids: Vec<u32> = (1..=40).collect();
        let l = looks(Os::Linux, &pids, Path::new("/x"), &have(&["eu-stack"]));
        assert_eq!(l.iter().filter(|l| l.program == "eu-stack").count(), MAX_PROCESSES);
        assert!(
            l[0].args.last().is_some_and(|a| a.split(',').count() == MAX_PROCESSES),
            "{:?}",
            l[0]
        );
    }

    /// A positive control: the capture of a live process (a stand-in that sleeps) writes the cause, the process list with its pid and
    /// the stack tool's note, and says so when the OS has no stack tool.
    #[cfg(unix)]
    #[test]
    fn a_capture_of_a_live_process_writes_its_list_and_says_what_gave_the_stacks() {
        let d = tempfile::tempdir().unwrap();
        let step = paddock_testkit::StandIn::new(d.path(), "hangs")
            .sleeps(Duration::from_secs(30))
            .install()
            .unwrap();
        let mut child = paddock_proc::command(step.program()).spawn().unwrap();
        let id = ProcessId::of(child.id()).expect("the child's identity");
        let env: HashMap<String, String> = paddock_env::vars().collect();
        let on_path = |p: &str| crate::throttle::on_path(&env, p);
        let c = capture(this_os(), &[id.pid()], "stand-in hung", d.path(), "hangs", &env, &on_path);
        paddock_proc::kill_tree(id);
        let _ = child.wait();
        let text = std::fs::read_to_string(&c.file).unwrap();
        assert!(text.starts_with("# stand-in hung"), "{text}");
        assert!(
            text.contains(&id.pid().to_string()) && text.contains("hangs"),
            "the process list names the process:\n{text}"
        );
        assert!(c.note.contains("written to"), "{}", c.note);
        let none = capture(Os::Other, &[id.pid()], "x", d.path(), "none", &env, &|_| false);
        assert!(
            std::fs::read_to_string(none.file).unwrap().contains("no stack tool installed"),
            "{}",
            none.note
        );
    }
}
