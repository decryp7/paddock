#!/usr/bin/env node
// The checks of .gitea/workflows/tag.yml, before the release bot makes a tag (rules 9 and 54, ER-15, ER-16, ER-31 (c), ER-33).
// A tag is requested for a commit; nothing is pushed here. Each failed check is one `::error::request-tag:` line with the cause
// and the next step, and the exit code is 1. A check that could not look (API unreadable, no run listed) is a failure, never a pass.
//
//   node request-tag.mjs --tag pitcrew-v1.2.3 --sha <40 hex> [--root .] --api https://host/api/v1 --repo owner/name
//
// The token for the Actions API is read from GITEA_TOKEN (the job's own read token; the bot's token is used only by the push step).
import { appendFileSync, existsSync, readFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
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
 * Throws unless the newest run of `workflow` on exactly `sha` is completed and successful. `runs` are the API's run objects.
 * An empty list is "not measured" (the API did not show a run), which also fails.
 */
export function checkGreenRun(runs, sha, workflow) {
  const mine = runs
    .filter((r) => (r.head_sha ?? '') === sha && String(r.path ?? '').split('@')[0].split('/').pop() === workflow)
    .sort((a, b) => (b.id ?? 0) - (a.id ?? 0));
  if (!mine.length) throw new Error(`not measured: the Actions API lists no ${workflow} run for ${sha.slice(0, 10)}; push the commit, let ${workflow} finish green on it, then request the tag`);
  const r = mine[0];
  const done = r.status === 'completed' || ['success', 'failure', 'cancelled', 'skipped'].includes(r.status);
  const state = r.conclusion || (done ? r.status : r.status || 'unknown');
  if (!done || state !== 'success') throw new Error(`the newest ${workflow} run (${r.id}) for ${sha.slice(0, 10)} is ${state}, not success; get it green on this commit, then request the tag`);
  return r;
}

/** Runs every check; returns `[{ name, ok, detail }]`, stopping nowhere: all failures are reported. */
export async function runChecks({ tag, sha, read, git, fetchRuns }) {
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
  await step('commit is on origin', () => {
    const on = git(['branch', '-r', '--contains', sha]).split('\n').map((l) => l.trim()).filter(Boolean);
    if (!on.length) throw new Error(`commit ${sha.slice(0, 10)} is on no branch of origin; push it and let CI build it first`);
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
    const r = checkGreenRun(await fetchRuns(sha), sha, wf);
    return `${wf} run ${r.id}`;
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
  const read = (p) => (existsSync(join(root, p)) ? readFileSync(join(root, p), 'utf8') : null);
  const git = (args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 64 << 20 });
  const fetchRuns = async (sha) => {
    const token = process.env.GITEA_TOKEN;
    const res = await fetch(`${o.api.replace(/\/+$/, '')}/repos/${o.repo}/actions/runs?head_sha=${sha}&limit=50`, { headers: token ? { Authorization: `token ${token}` } : {} });
    if (!res.ok) throw new Error(`not measured: the Actions API answered HTTP ${res.status}; check the token can read Actions`);
    const body = await res.json();
    return body.workflow_runs ?? body.runs ?? [];
  };
  const results = await runChecks({ tag: o.tag, sha: o.sha, read, git, fetchRuns });
  const md = summary(o.tag, o.sha, results);
  if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, md);
  console.log(md);
  for (const r of results.filter((x) => !x.ok)) console.error(`::error::request-tag: ${r.name}: ${r.detail}`);
  process.exit(results.some((r) => !r.ok) ? 1 : 0);
}
