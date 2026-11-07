// Finishing a run for every runner: the Markdown result, a JSON sidecar next to it (what release-verdict.mjs reads), and the exit code from the
// derived verdict (verdict.mjs). The result folder is stable: <out>/<ci run id>/ when the installer came from a CI run, so the files of one release
// check sit together and `release-verdict.mjs <folder>` can tell what is missing.
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { renderResult } from './judge.mjs';
import { EXIT, deriveVerdict, exitCodeFor, expectedCommit } from './verdict.mjs';

/** The result folder: `<out>/<ci run id>` or `<out>/local-<installer sha256 prefix>`, so a release check's files stay together. */
export function resultFolder(out, { run, installerSha256 }) {
  return join(out, run ? `run-${run}` : `local-${(installerSha256 ?? 'unknown').slice(0, 12)}`);
}

export const sha256File = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');

/** The commit of a CI run (Gitea API `head_sha`), shortened to the 12 digits the build embeds; null when it cannot be read (the step is then not measured). */
export function commitOfRun(runId, { token = process.env.GITEA_TOKEN, host = process.env.PADDOCK_GITEA ?? 'https://dev.decryptology.net' } = {}) {
  if (!token || !/^\d+$/.test(String(runId))) return null;
  const r = spawnSync('curl', ['-sS', '-L', '--fail', '--config', '-'], { encoding: 'utf8', timeout: 30_000, input: `header = "Authorization: token ${token}"\nurl = "${host}/api/v1/repos/decryp7/paddock/actions/runs/${runId}"\n` });
  if (r.status !== 0) return null;
  try { const j = JSON.parse(r.stdout); return expectedCommit(j.head_sha ?? j.commit_sha ?? j.HeadSha); } catch { return null; }
}

/** The JSON sidecar of a run. */
export function sidecar(run, meta) {
  const v = deriveVerdict({ steps: run.steps, required: run.required });
  return {
    label: meta.label, os: meta.os, suite: meta.suite ?? null, ciRun: meta.run ?? null, installerSha256: run.installerSha256 ?? null,
    expectedCommit: run.expectedCommit ?? null, partial: Boolean(run.partial), verdict: v.verdict, missing: v.missing,
    failed: v.failed.map((s) => s.title), broken: v.broken.map((s) => s.title), steps: run.steps.map((s) => ({ title: s.title, status: s.status })),
  };
}

/** Writes `<folder>/<stamp>-<label>.md` and `.json`; returns { file, exit } with the exit code of the derived verdict. */
export function finalizeRun({ out, run, meta, started }) {
  const folder = resultFolder(out, { run: meta.run, installerSha256: run.installerSha256 });
  mkdirSync(folder, { recursive: true });
  const stem = join(folder, `${started.replace(/[:.]/g, '-')}-${meta.label}`);
  writeFileSync(`${stem}.md`, renderResult(run));
  const side = sidecar(run, meta);
  writeFileSync(`${stem}.json`, `${JSON.stringify(side, null, 2)}\n`);
  return { file: `${stem}.md`, exit: exitCodeFor({ verdict: side.verdict }), verdict: side.verdict };
}

/** The runs a release check consists of (chain-release.sh): every one must have a PASS sidecar for the same CI run. */
export const RELEASE_RUNS = [
  { label: 'windows-ui', os: 'windows' }, { label: 'windows-restart', os: 'windows' }, { label: 'linux-reboot', os: 'linux' },
  { label: 'macos-start', os: 'macos' }, { label: 'macos-power', os: 'macos' }, { label: 'macos-helper', os: 'macos' }, { label: 'macos-helper-restart', os: 'macos' },
];

/**
 * The verdict over a folder's sidecars: PASS only when each of RELEASE_RUNS has a non-partial PASS for the same CI run and the runs of one OS
 * used the same installer. Anything else lists what is missing, in a way a script can exit on (EXIT.INCOMPLETE / EXIT.FAIL).
 */
export function judgeRelease(sidecars, expected = RELEASE_RUNS) {
  const problems = [];
  let fail = false;
  const latest = new Map();
  for (const s of sidecars) latest.set(s.label, s); // later files replace earlier ones with the same label
  for (const e of expected) {
    const s = latest.get(e.label);
    if (!s) { problems.push(`${e.label}: no result file`); continue; }
    if (s.partial) problems.push(`${e.label}: a partial run is not a release verdict`);
    if (s.verdict === 'FAIL') { fail = true; problems.push(`${e.label}: FAIL (${s.failed.join('; ')})`); }
    else if (s.verdict !== 'PASS') problems.push(`${e.label}: ${s.verdict} (${s.missing.map((m) => m.id).concat(s.broken).join(', ')})`);
  }
  const runs = new Set([...latest.values()].map((s) => s.ciRun));
  if (runs.size > 1) problems.push(`the results come from different CI runs: ${[...runs].join(', ')}`);
  for (const os of ['windows', 'linux', 'macos']) {
    const hashes = new Set([...latest.values()].filter((s) => s.os === os).map((s) => s.installerSha256));
    if (hashes.size > 1) problems.push(`${os}: the runs used different installers (${[...hashes].map((h) => String(h).slice(0, 12)).join(', ')})`);
  }
  const verdict = fail ? 'FAIL' : problems.length ? 'INCOMPLETE' : 'PASS';
  return { verdict, problems, exit: EXIT[verdict] };
}

/** Whether a binary holds the build commit, from `grep -c -a -F` (macOS, Linux): exit 0 found, 1 not found, anything else unreadable (null). */
export const foundFromGrep = (code) => (code === 0 ? true : code === 1 ? false : null);

/** The same from PowerShell's `...Contains('<sha>')` output: "True", "False", or anything else (unreadable, null). */
export const foundFromPs = (text) => (/^True$/m.test(text) ? true : /^False$/m.test(text) ? false : null);

/** The PowerShell that prints True or False for "this file contains the build commit" (the 12 digits `option_env!` embeds in the binary). */
export const psHasCommit = (path, sha) => `try { [Text.Encoding]::GetEncoding(28591).GetString([IO.File]::ReadAllBytes('${path}')).Contains('${sha}') } catch { 'unreadable' }`;

/** Inserts the build-binding step right after the install step (or first), so the header and the steps read in run order. */
export function withBuildStep(steps, step) {
  const i = steps.findIndex((s) => /^Install/.test(s.title));
  return [...steps.slice(0, i + 1), step, ...steps.slice(i + 1)];
}

/**
 * The macOS base image must exist before a clone is tried (another agent may be rebuilding it): a missing base is a broken check with the reason, not a
 * tool error and not a product failure. `listText` is `tart list --quiet`.
 */
export function preflightBase(listText, base) {
  const have = String(listText ?? '').split('\n').map((l) => l.trim()).filter(Boolean);
  const ok = have.includes(base);
  return {
    title: 'Preflight: the Tart base image exists', status: ok ? 'pass' : 'check-broken', cls: ok ? 'product' : 'check',
    summary: ok ? `base ${base} exists` : `base ${base} does not exist at this moment (another agent may be rebuilding it); local VMs: ${have.join(', ') || 'none'}`,
    checks: [{ ok, ...(ok ? {} : { kind: 'check' }), detail: ok ? `base ${base} exists` : `tart list has no VM named ${base}` }], evidence: have,
  };
}
