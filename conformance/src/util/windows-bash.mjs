import { parseJobs, parseSteps } from './workflows.mjs';

const GIT_BASH_STEP = /Git bash on PATH/;
const NOT_WINDOWS = /runner\.os\s*(==\s*'(Linux|macOS)'|!=\s*'Windows')/;

/** A job may run on a Windows host when its `runs-on` names Windows or comes from a matrix or the plan (any os). */
function runsOnWindows(jobText) {
  const runsOn = /^\s*runs-on:\s*(.*)$/m.exec(jobText.replace(/^\s*#.*$/gm, ''))?.[1] ?? '';
  return /windows|matrix|fromJson|\$\{\{/i.test(runsOn);
}

/**
 * Steps that run bash on a Windows host job before Git's bash is put on PATH. There `bash` resolves to WSL's
 * C:\Windows\System32\bash.exe (run 7880: "wsl: Nested virtualization is not supported", the script never ran).
 * A step is exempt when its `if` keeps it off Windows. A Windows job with bash steps and no "Git bash on PATH" step is flagged
 * at its first bash step. Returns [{ job, step, line, why }].
 */
export function bashBeforeGitBash(workflowText) {
  const out = [];
  const workflowBash = /^defaults:\s*\n\s+run:\s*\n\s+shell:\s*bash\b/m.test(workflowText);
  for (const job of parseJobs(workflowText)) {
    if (!runsOnWindows(job.text)) continue;
    const jobBash = workflowBash || /defaults:\s*\n\s+run:\s*\n\s+shell:\s*bash\b/.test(job.text);
    for (const step of parseSteps(job.text)) {
      if (GIT_BASH_STEP.test(step.name ?? '')) break;
      const ifLine = /^\s*if:\s*(.*)$/m.exec(step.text)?.[1] ?? '';
      if (NOT_WINDOWS.test(ifLine)) continue;
      const shell = /^\s*shell:\s*(\S+)/m.exec(step.text)?.[1];
      const bashRun = step.run !== null && (shell === 'bash' || (shell === undefined && jobBash));
      const localAction = typeof step.uses === 'string' && step.uses.startsWith('./.gitea/actions/');
      if (bashRun || localAction) {
        out.push({ job: job.id, step: step.name ?? step.uses ?? `line ${step.line}`, line: step.line, why: bashRun ? 'run step in bash' : 'composite action (bash)' });
        break;
      }
    }
  }
  return out;
}
