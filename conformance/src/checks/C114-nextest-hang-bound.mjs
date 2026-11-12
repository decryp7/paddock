// C114: every Cargo workspace bounds every test in nextest's default profile (ER-21: the class "an unbounded wait on a condition that
// never comes" hung runs three times, ADR 0071). A workspace is a folder with a tracked Cargo.lock beside a Cargo.toml that has a
// `[workspace]` table, except fuzz, vendored, spike, research and test-fixture folders. Its nextest config is `<folder>/.config/nextest.toml`,
// or the file a `paddock.ci.json` names in `nextestConfig` (the demos share one). `[profile.default]` must set
// `slow-timeout = { period = "...", terminate-after = N }` and period * N must be at most 120 s. A test that legitimately needs longer is a
// named `[[profile.default.overrides]]` with a `# waiver:` line, never a longer default. Every `[[profile.<p>.overrides]]` that sets a
// `slow-timeout` (the `ci` profile's included) needs a `terminate-after` with period * N at most 480 s and a `# waiver:` comment in the
// comment block directly above its header; the `bench` profile (ignored benchmarks, `--profile bench`) is exempt from the cap but not
// from the `terminate-after`. Workspaces are found among tracked and untracked files (`git ls-files -co --exclude-standard`); outside a git
// checkout the check reports "not measured" (ER-33), which fails.
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join, posix } from 'node:path';
import { error } from '../finding.mjs';

/** The most a test may run in the default profile before nextest stops it and fails it, in seconds. */
export const MAX_DEFAULT_SECONDS = 120;
/** The most an override of a non-bench profile may allow, in seconds (the longest waiver today is 8 periods of 60 s). */
export const MAX_OVERRIDE_SECONDS = 480;
const EXCLUDED = /(?:^|\/)(?:fuzz|vendor|spikes|research|fixture|fixtures|node_modules|target)\//;
const UNITS = { ms: 0.001, s: 1, m: 60, min: 60, h: 3600 };

/** The seconds in a nextest duration such as "30s", "2m" or "500ms" (NaN when it is not one). */
export function durationSeconds(text) {
  const m = /^\s*(\d+(?:\.\d+)?)\s*(ms|s|m|min|h)\s*$/.exec(text);
  return m ? Number(m[1]) * UNITS[m[2]] : NaN;
}

/** The text of the `[profile.default]` table (up to the next table header), or null when there is none. */
export function defaultProfile(toml) {
  const lines = toml.split('\n');
  const at = lines.findIndex((l) => /^\s*\[profile\.default\]\s*(#.*)?$/.test(l));
  if (at < 0) return null;
  const rest = lines.slice(at + 1);
  const end = rest.findIndex((l) => /^\s*\[/.test(l));
  return (end < 0 ? rest : rest.slice(0, end)).join('\n');
}

/** What is wrong with a config's default profile (a sentence), or null when it bounds every test within MAX_DEFAULT_SECONDS. */
export function boundProblem(toml) {
  const profile = defaultProfile(toml);
  if (profile === null) return 'has no [profile.default] table';
  const line = profile.split('\n').find((l) => /^\s*slow-timeout\s*=/.test(l));
  if (!line) return '[profile.default] sets no slow-timeout';
  const after = /terminate-after\s*=\s*(\d+)/.exec(line);
  if (!after) return `[profile.default] slow-timeout has no terminate-after (nextest only reports a slow test, it never stops it): ${line.trim()}`;
  const period = /period\s*=\s*"([^"]*)"/.exec(line);
  const seconds = period ? durationSeconds(period[1]) * Number(after[1]) : NaN;
  if (!(seconds > 0)) return `[profile.default] slow-timeout is not period * terminate-after: ${line.trim()}`;
  if (seconds > MAX_DEFAULT_SECONDS) return `[profile.default] slow-timeout stops a test only after ${seconds} s (more than ${MAX_DEFAULT_SECONDS} s): ${line.trim()}`;
  return null;
}

/** What is wrong with each `[[profile.<p>.overrides]]` that sets a slow-timeout: [line number, sentence] pairs. */
export function overrideProblems(toml) {
  const lines = toml.split('\n');
  const problems = [];
  lines.forEach((header, at) => {
    const h = /^\s*\[\[profile\.([^.\]]+)\.overrides\]\]/.exec(header);
    if (!h) return;
    const rest = lines.slice(at + 1);
    const end = rest.findIndex((l) => /^\s*\[/.test(l));
    const body = end < 0 ? rest : rest.slice(0, end);
    const line = body.find((l) => /^\s*slow-timeout\s*=/.test(l));
    if (!line) return;
    const profile = h[1];
    let above = at - 1;
    while (above >= 0 && /^\s*#/.test(lines[above])) above--;
    const comment = lines.slice(above + 1, at).join('\n');
    if (!/#\s*waiver:\s*\S/.test(comment)) problems.push([at + 1, `the ${profile} override sets a slow-timeout without a "# waiver: <why>" comment directly above it`]);
    const after = /terminate-after\s*=\s*(\d+)/.exec(line);
    if (!after) { problems.push([at + 1, `the ${profile} override's slow-timeout has no terminate-after: ${line.trim()}`]); return; }
    const period = /period\s*=\s*"([^"]*)"/.exec(line);
    const seconds = period ? durationSeconds(period[1]) * Number(after[1]) : NaN;
    if (!(seconds > 0)) problems.push([at + 1, `the ${profile} override's slow-timeout is not period * terminate-after: ${line.trim()}`]);
    else if (profile !== 'bench' && seconds > MAX_OVERRIDE_SECONDS) problems.push([at + 1, `the ${profile} override stops a test only after ${seconds} s (more than ${MAX_OVERRIDE_SECONDS} s): ${line.trim()}`]);
  });
  return problems;
}

export default {
  id: 'C114',
  scope: 'root',
  mode: 'gate',
  title: 'Every Cargo workspace bounds every test in nextest',
  rule: 5,
  run(ctx) {
    const git = ctx.options.git ?? ((args) => execFileSync('git', args, { cwd: ctx.root, encoding: 'utf8', maxBuffer: 64 << 20 }));
    let tracked;
    try {
      git(['rev-parse', '--show-toplevel']);
      tracked = git(['ls-files', '-co', '--exclude-standard', '--', '.']).split('\n').filter(Boolean);
    } catch {
      return [error('C114', 5, '.', 1, 'not measured: this folder is not a git checkout, so its Cargo workspaces and nextest configs cannot be listed (ER-33: a check that did not look does not pass)', 'Run the check in a git checkout (clone the repository; an export or a tarball has no file list).')];
    }
    const files = new Set(tracked);
    const named = []; // [config path, the paddock.ci.json that names it]
    for (const f of tracked.filter((p) => /(?:^|\/)paddock\.ci\.json$/.test(p))) {
      try {
        const cfg = JSON.parse(readFileSync(join(ctx.root, f), 'utf8')).nextestConfig;
        if (typeof cfg === 'string') named.push([posix.normalize(cfg), f]);
      } catch { /* a broken paddock.ci.json is another check's finding */ }
    }
    const workspaces = tracked
      .filter((p) => /(?:^|\/)Cargo\.lock$/.test(p) && !EXCLUDED.test(p))
      .map((p) => posix.dirname(p))
      .filter((d) => {
        const manifest = join(ctx.root, d, 'Cargo.toml');
        return existsSync(manifest) && /^\[workspace\]/m.test(readFileSync(manifest, 'utf8'));
      });
    const configs = new Map(); // config path -> the workspaces it serves
    const findings = [];
    const next = 'Add `[profile.default]` with `slow-timeout = { period = "30s", terminate-after = 4 }` (stops a hung test after 120 s) to the config, and give a test that needs longer a named `[[profile.default.overrides]]` with a `# waiver:` line saying why (see tools/paddock-dev/.config/nextest.toml); do not raise the default.';
    for (const ws of workspaces) {
      const own = posix.join(ws, '.config/nextest.toml');
      const shared = named.find(([c, ci]) => files.has(c) && ws.startsWith(`${dirname(ci)}/`))?.[0];
      const config = files.has(own) ? own : shared;
      if (!config) {
        findings.push(error('C114', 5, ws, 1, `Cargo workspace ${ws} has no nextest config (${own}), so its tests have no time bound`, `Create ${own} (copy tools/wiki/.config/nextest.toml) or name a shared config in this app's paddock.ci.json \`nextestConfig\`. ${next}`));
        continue;
      }
      configs.set(config, [...(configs.get(config) ?? []), ws]);
    }
    for (const config of new Set([...configs.keys(), ...tracked.filter((p) => /(?:^|\/)\.config\/nextest[^/]*\.toml$/.test(p))])) {
      if (!existsSync(join(ctx.root, config))) {
        findings.push(error('C114', 5, config, 1, `${config}: file is gone from the working tree but still listed by git, so its workspace's tests have no time bound`, `Restore it (\`git checkout -- ${config}\`) or remove it from git (\`git rm ${config}\`) together with the workspace it served.`));
        continue;
      }
      const text = readFileSync(join(ctx.root, config), 'utf8');
      for (const [line, what] of overrideProblems(text)) {
        findings.push(error('C114', 5, config, line, `${config}: ${what}`, 'Put a `# waiver: <why this test needs longer>` comment directly above the `[[profile.<p>.overrides]]` header and give its slow-timeout a `terminate-after` (period * N at most 480 s); a benchmark goes in the `bench` profile. Never raise the default.'));
      }
      const problem = boundProblem(text);
      if (problem) findings.push(error('C114', 5, config, 1, `${config} ${problem}: a test that waits on something that never comes hangs the run`, next));
    }
    return findings;
  },
};
