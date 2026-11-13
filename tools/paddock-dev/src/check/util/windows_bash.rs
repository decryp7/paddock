//! Steps that run bash on a Windows host job before Git's bash is put on PATH. There `bash` resolves to WSL's
//! `C:\Windows\System32\bash.exe` (run 7880: "wsl: Nested virtualization is not supported", the script never ran). A step is
//! exempt when its `if` keeps it off Windows. A Windows job with bash steps and no "Git bash on PATH" step is flagged at its first
//! bash step. The port of `util/windows-bash.mjs`.

use super::workflows::{parse_jobs, parse_steps};
use crate::check::jsre::{DOT, rx};
use regex::Regex;
use std::sync::LazyLock;

/// A flagged step.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BashStep {
    /// The job id.
    pub job: String,
    /// The step's name, its `uses`, or `line N`.
    pub step: String,
    /// The 1-based line.
    pub line: usize,
    /// Why it is flagged.
    pub why: &'static str,
}

static GIT_BASH_STEP: LazyLock<Regex> = LazyLock::new(|| rx(r"Git bash on PATH"));
static NOT_WINDOWS: LazyLock<Regex> = LazyLock::new(|| rx(r"runner\.os\s*(==\s*'(Linux|macOS)'|!=\s*'Windows')"));
static COMMENT_LINE: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"(?mR)^\s*#{DOT}*$")));
static RUNS_ON: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"(?mR)^\s*runs-on:\s*({DOT}*)$")));
static WINDOWS_LIKE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?i)windows|matrix|fromJson|\$\{\{"));
static WORKFLOW_BASH: LazyLock<Regex> = LazyLock::new(|| rx(r"(?mR)^defaults:\s*\n\s+run:\s*\n\s+shell:\s*bash\b"));
static JOB_BASH: LazyLock<Regex> = LazyLock::new(|| rx(r"defaults:\s*\n\s+run:\s*\n\s+shell:\s*bash\b"));
static IF_LINE: LazyLock<Regex> = LazyLock::new(|| rx(&format!(r"(?mR)^\s*if:\s*({DOT}*)$")));
static SHELL: LazyLock<Regex> = LazyLock::new(|| rx(r"(?mR)^\s*shell:\s*(\S+)"));

/// A job may run on a Windows host when its `runs-on` names Windows or comes from a matrix or the plan (any os).
fn runs_on_windows(job_text: &str) -> bool {
    let cleaned = COMMENT_LINE.replace_all(job_text, "");
    let runs_on = RUNS_ON.captures(&cleaned).map_or(String::new(), |c| c[1].to_string());
    WINDOWS_LIKE.is_match(&runs_on)
}

/// The bash steps of Windows jobs before Git's bash is on PATH.
pub fn bash_before_git_bash(workflow_text: &str) -> Vec<BashStep> {
    let mut out = Vec::new();
    let workflow_bash = WORKFLOW_BASH.is_match(workflow_text);
    for job in parse_jobs(workflow_text) {
        if !runs_on_windows(&job.text) {
            continue;
        }
        let job_bash = workflow_bash || JOB_BASH.is_match(&job.text);
        for step in parse_steps(&job.text) {
            if GIT_BASH_STEP.is_match(step.name.as_deref().unwrap_or("")) {
                break;
            }
            let if_line = IF_LINE.captures(&step.text).map_or(String::new(), |c| c[1].to_string());
            if NOT_WINDOWS.is_match(&if_line) {
                continue;
            }
            let shell = SHELL.captures(&step.text).map(|c| c[1].to_string());
            let bash_run = step.run.is_some() && (shell.as_deref() == Some("bash") || (shell.is_none() && job_bash));
            let local_action = step.uses.as_deref().is_some_and(|u| u.starts_with("./.gitea/actions/"));
            if bash_run || local_action {
                out.push(BashStep {
                    job: job.id.clone(),
                    step: step
                        .name
                        .clone()
                        .or(step.uses.clone())
                        .unwrap_or_else(|| format!("line {}", step.line)),
                    line: step.line,
                    why: if bash_run { "run step in bash" } else { "composite action (bash)" },
                });
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runs_on_line_ends_before_a_carriage_return_as_in_javascript_multiline_mode() {
        assert!(runs_on_windows("  runs-on: windows-latest\r\n  steps:\r\n"));
        assert!(!runs_on_windows("  runs-on: ubuntu-latest\r\n  steps:\r\n"));
        assert!(runs_on_windows("  # c\r\n  runs-on: ${{ matrix.os }}\r\n"));
    }

    #[test]
    fn a_windows_job_running_bash_before_git_bash_is_flagged_once_and_a_guard_clears_it() {
        let bad = "jobs:\n  w:\n    runs-on: windows-latest\n    steps:\n      - name: build\n        shell: bash\n        run: make\n";
        let f = bash_before_git_bash(bad);
        assert_eq!(f.len(), 1, "{f:?}");
        assert_eq!(
            (f[0].job.as_str(), f[0].step.as_str(), f[0].why),
            ("w", "build", "run step in bash")
        );
        let good = "jobs:\n  w:\n    runs-on: windows-latest\n    steps:\n      - name: Git bash on PATH\n        run: x\n      - name: build\n        shell: bash\n        run: make\n";
        assert!(bash_before_git_bash(good).is_empty());
        let linux = "jobs:\n  w:\n    runs-on: ubuntu-latest\n    steps:\n      - shell: bash\n        run: make\n";
        assert!(bash_before_git_bash(linux).is_empty());
    }
}
