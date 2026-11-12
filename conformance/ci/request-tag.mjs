#!/usr/bin/env node
// The checks of .gitea/workflows/tag.yml, before the release bot makes a tag (rules 9 and 54, ER-15, ER-16, ER-31 (c), ER-33).
// A tag is requested for a commit; nothing is pushed here. Each failed check is one `::error::request-tag:` line with the cause
// and the next step, and the exit code is 1. A check that could not look (API unreadable, no run listed) is a failure, never a pass.
//
//   node request-tag.mjs --tag pitcrew-v1.2.3 --sha <40 hex> [--root <trusted checkout>] --api https://host/api/v1 --repo owner/name
//
// "Is there a full green run of the gate workflow on this commit?" is not decided here: the check runs `./dev ci verdict`
// (tools/paddock-dev/src/ci_runs/verdict.rs), the one reader of the Actions runs for that answer. The ./dev of the trusted
// checkout is used, so no code of the tagged tree runs. Its exit code is 0 full green, 5 red or only partial runs, 6 not measured.
//
// This script runs from the DISPATCH ref's checkout (main). The tree being tagged is only read as data (`git show <sha>:path`), never run.
// A guard rail against mistakes, not a security boundary: any repo writer can dispatch from their own ref.
// The token for the Actions API is read from GITEA_TOKEN by ./dev (the job's own read token; the bot's token is used only by the push job).
import { appendFileSync } from 'node:fs';
import { execFileSync, spawnSync } from 'node:child_process';
import { join } from 'node:path';
import { isMain } from '../src/util/is-main.mjs';
import { checkTag } from './check-tag.mjs';

export const TAG_RE = /^(?:([a-z0-9]+(?:-[a-z0-9]+)*)-)?v(\d+\.\d+\.\d+(?:-[0-9A-Za-z.]+)?)$/;
export const SHA_RE = /^[0-9a-f]{40}$/;
const SKIP_CI = /\[(?:skip ci|ci skip|no ci|skip actions|actions skip)\]/i;

/** `{ app, version }` of a tag (`app` is null for a `v*` tag of the shared packages), or throws. */
export function parseTag(tag) {
  const m = TAG_RE.exec(tag);
  if (!m) throw new Error(`the tag "${tag}" is not <app>-vX.Y.Z or vX.Y.Z; request a tag like dyno-v1.2.0`);
  return { app: m[1] ?? null, version: m[2] };
}

/** Throws when the commit message would make CI skip the tag run (ER-16). */
export function checkNotSkipCi(message, sha) {
  if (SKIP_CI.test(message)) throw new Error(`commit ${sha.slice(0, 10)} carries a skip-ci marker, so no CI run built it or will build the tag; tag a commit CI has built`);
}

/** Throws unless the app's single version source says the tag's version. `read(path)` returns the file's text or null. */
export function checkVersion(app, tag, read) {
  const conf = app && `apps/${app}/app/src-tauri/tauri.conf.json`;
  const cargo = app && `apps/${app}/src-tauri/Cargo.toml`;
  if (!app) {
    const pkg = 'ui/projects/paddock-ui/package.json';
    const text = read(pkg);
    if (text == null) throw new Error(`${pkg} is missing at this commit, so the version of ${tag} cannot be read`);
    return checkTag(tag, '', JSON.parse(text));
  }
  const confText = read(conf);
  if (confText != null) return checkTag(tag, `${app}-`, JSON.parse(confText));
  const cargoText = read(cargo);
  if (cargoText != null) {
    const have = /^\[package\][^[]*?^version\s*=\s*"([^"]+)"/m.exec(cargoText)?.[1];
    const want = parseTag(tag).version;
    if (have !== want) throw new Error(`the tag ${tag} says ${want}, ${cargo} says ${have ?? 'nothing'}: bump the version first, then request the tag`);
    return want;
  }
  throw new Error(`no version source for "${app}" (neither ${conf} nor ${cargo}); is "${app}" an app of this repository?`);
}

/** The workflow that gates an app: `workflow` of its paddock.ci.json, else ci.yml (a leg of ci.yml; also the shared packages). */
export function gateWorkflow(app, read) {
  const text = app && read(`apps/${app}/paddock.ci.json`);
  return (text && JSON.parse(text).workflow) || 'ci.yml';
}

/**
 * Throws unless `./dev ci verdict` (spawnSync's result) said full green. Exit 5 is red or partial runs only, 6 is not measured,
 * anything else (the tool missing, killed, unreadable output) is also not measured: nothing here ever turns a failure into a pass.
 * Returns the deciding run's id.
 */
export function checkVerdict(res) {
  let out = null;
  try {
    out = JSON.parse(String(res.stdout ?? '').trim().split('\n').pop() || 'null');
  } catch {
    out = null;
  }
  const detail = out?.detail ?? out?.error;
  if (res.status === 0 && out?.verdict === 'green') return out.run;
  if (res.status === 5 || res.status === 6) throw new Error(`${detail ?? `./dev ci verdict exited ${res.status}`}; then request the tag`);
  const why = res.error?.message ?? detail ?? String(res.stderr ?? '').trim().split('\n').slice(-3).join(' ');
  throw new Error(`not measured: ./dev ci verdict did not answer (exit ${res.status ?? 'none'}${why ? `: ${why}` : ''}); check ./dev builds on main and GITEA_TOKEN can read the Actions runs`);
}

/** Runs every check; returns `[{ name, ok, detail }]`, stopping nowhere: all failures are reported. */
export async function runChecks({ tag, sha, read, git, verdict }) {
  const results = [];
  const step = async (name, fn) => {
    try {
      results.push({ name, ok: true, detail: String((await fn()) ?? 'ok') });
    } catch (e) {
      results.push({ name, ok: false, detail: e.message });
    }
  };
  let parsed;
  await step('tag name', () => {
    parsed = parseTag(tag);
    return `${parsed.app ?? 'shared packages'} ${parsed.version}`;
  });
  await step('sha format', () => {
    if (!SHA_RE.test(sha)) throw new Error(`"${sha}" is not a full 40-hex commit id; pass the whole sha`);
  });
  if (!parsed || !SHA_RE.test(sha)) return results;
  await step('commit is on origin main or release/*', () => {
    const on = git(['branch', '-r', '--contains', sha]).split('\n').map((l) => l.trim()).filter((b) => b === 'origin/main' || /^origin\/release\/\S+$/.test(b));
    if (!on.length) throw new Error(`commit ${sha.slice(0, 10)} is on neither origin/main nor an origin/release/* branch; land it there and let CI build it first`);
    return on.slice(0, 3).join(', ');
  });
  await step('not a skip-ci commit', () => {
    checkNotSkipCi(git(['log', '-1', '--format=%B', sha]), sha);
  });
  await step('tag does not exist', () => {
    const have = git(['ls-remote', '--tags', 'origin', `refs/tags/${tag}`]).trim();
    if (have) throw new Error(`${tag} already exists on origin; a tag is never moved: bump the version and request a new tag`);
  });
  await step('version matches the tag', () => `version ${checkVersion(parsed.app, tag, read)}`);
  await step('CI is green on exactly this commit', async () => {
    const wf = gateWorkflow(parsed.app, read);
    return `${wf} full run ${checkVerdict(await verdict(wf, sha))}`;
  });
  return results;
}

/** Markdown for the job summary. */
export function summary(tag, sha, results) {
  const bad = results.filter((r) => !r.ok).length;
  return [
    `## Tag request: ${tag}`,
    `Commit \`${sha}\`. ${bad ? `**Refused: ${bad} check(s) failed.** No tag was made.` : 'All checks passed.'}`,
    '',
    '| Check | Result | Detail |',
    '| --- | --- | --- |',
    ...results.map((r) => `| ${r.name} | ${r.ok ? 'pass' : 'FAIL'} | ${r.detail.replace(/\|/g, '/').replace(/\n/g, ' ')} |`),
    '',
  ].join('\n');
}

if (isMain(import.meta.url)) {
  const o = {};
  const argv = process.argv.slice(2);
  for (let i = 0; i < argv.length; i += 2) o[argv[i].replace(/^--/, '')] = argv[i + 1];
  const root = o.root ?? '.';
  const fail = (m) => {
    console.error(`::error::request-tag: ${m}`);
    process.exit(1);
  };
  if (!o.tag || !o.sha || !o.api || !o.repo) fail('usage: request-tag.mjs --tag <tag> --sha <40 hex> --api <url> --repo <owner/name> [--root .]');
  const git = (args) => execFileSync('git', ['-c', 'core.hooksPath=/dev/null', ...args], { cwd: root, encoding: 'utf8', maxBuffer: 64 << 20 });
  // The target tree is data: a file of the commit, read with git, never checked out and never run.
  const read = (p) => {
    try {
      return execFileSync('git', ['-c', 'core.hooksPath=/dev/null', 'show', `${o.sha}:${p}`], { cwd: root, encoding: 'utf8', maxBuffer: 64 << 20, stdio: ['ignore', 'pipe', 'ignore'] });
    } catch {
      return null;
    }
  };
  // The verdict is `./dev ci verdict` of the trusted checkout (cwd = root); the job's read token reaches it as GITEA_TOKEN.
  const server = o.api.replace(/\/+$/, '').replace(/\/api\/v1$/, '');
  const verdict = async (workflow, sha) =>
    spawnSync(join(root, 'dev'), ['ci', 'verdict', '--json', '--workflow', workflow, '--sha', sha, '--repo', o.repo, '--server', server], {
      cwd: root,
      encoding: 'utf8',
      maxBuffer: 16 << 20,
      timeout: 20 * 60 * 1000,
    });
  const results = await runChecks({ tag: o.tag, sha: o.sha, read, git, verdict });
  const md = summary(o.tag, o.sha, results);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, md);
  console.log(md);
  for (const r of results.filter((x) => !x.ok)) console.error(`::error::request-tag: ${r.name}: ${r.detail}`);
  process.exit(results.some((r) => !r.ok) ? 1 : 0);
}
