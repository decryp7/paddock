#!/usr/bin/env node
// Pre-tag branch audit (ER-27 style output): lists remote branches with commits that touch an app's paths and are not in
// the release ref, and says for each whether the work is applied (same patch-id, or its added lines are in the ref) or missing.
//
//   node scripts/branch-audit.mjs --app dyno --ref origin/dyno/wp-int [--repo <dir>] [--remote origin]
//        [--allow <file>] [--since <iso date>] [--path <extra path>]...
//
// Why: on 2026-11-05 four Dyno branches had second-round commits that were never merged, and nothing noticed until a VM row failed.
// Branches: refs/remotes/<remote>/*, updated since the app's last `<app>-v*` tag (or --since). Allow-list: one `branch  reason`
// per line (default apps/<app>/docs/branch-audit-allow.txt); an entry without a reason is a usage error.
// Exit 0 nothing missing, 1 missing (one ::error:: line each), 2 usage or git error. Patch-ids are cached in <git-dir>/branch-audit-cache.json.
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const MIN_LEN = 4; // blank lines, braces: never evidence

export function parseArgs(argv) {
  const o = { repo: process.cwd(), remote: 'origin', paths: [] };
  for (let i = 0; i < argv.length; i++) {
    const k = argv[i];
    const v = () => argv[++i] ?? usage(`${k} needs a value`);
    if (k === '--app') o.app = v();
    else if (k === '--ref') o.ref = v();
    else if (k === '--repo') o.repo = v();
    else if (k === '--remote') o.remote = v();
    else if (k === '--allow') o.allow = v();
    else if (k === '--since') o.since = v();
    else if (k === '--path') o.paths.push(v());
    else usage(`unknown argument ${k}`);
  }
  if (!o.app || !o.ref) usage('--app and --ref are required');
  o.paths.unshift(`apps/${o.app}/`);
  return o;
}

function usage(msg) {
  console.error(`::error::branch-audit: ${msg}. Usage: node scripts/branch-audit.mjs --app <app> --ref <release ref> [--repo dir] [--allow file] [--since date]`);
  process.exit(2);
}

export function parseAllow(text) {
  const out = new Map();
  for (const raw of text.split('\n')) {
    const line = raw.trim();
    if (!line || line.startsWith('#')) continue;
    const m = /^(\S+)\s+(.+)$/.exec(line);
    if (!m) usage(`allow-list entry "${line}" has no reason; write "<branch>  <why it is dropped or postponed>"`);
    out.set(m[1], m[2]);
  }
  return out;
}

/** Added and removed lines per file of a `-U0` diff. */
export function parseDiff(diff) {
  const files = new Map();
  let cur = null;
  for (const l of diff.split('\n')) {
    if (l.startsWith('+++ ')) {
      cur = l === '+++ /dev/null' ? null : l.slice(6);
      if (cur) files.set(cur, { added: [], removed: [] });
    } else if (l.startsWith('--- ')) {
      if (l === '--- /dev/null') continue;
      if (!files.has(l.slice(6)) && cur === null) cur = l.slice(6), files.set(cur, { added: [], removed: [] });
    } else if (cur && l.startsWith('+')) files.get(cur).added.push(l.slice(1));
    else if (cur && l.startsWith('-')) files.get(cur).removed.push(l.slice(1));
  }
  return files;
}

function run(o) {
  const git = (args, input) => {
    const r = spawnSync('git', ['-C', o.repo, ...args], { encoding: 'utf8', maxBuffer: 512 * 1024 * 1024, input });
    if (r.status !== 0) {
      console.error(`::error::branch-audit: git ${args.slice(0, 3).join(' ')} failed: ${r.stderr.trim()}. Next step: fetch ${o.remote} and check --repo, --ref.`);
      process.exit(2);
    }
    return r.stdout;
  };
  const tryGit = (args) => {
    const r = spawnSync('git', ['-C', o.repo, ...args], { encoding: 'utf8', maxBuffer: 512 * 1024 * 1024 });
    return r.status === 0 ? r.stdout : null;
  };
  const refSha = git(['rev-parse', '--verify', `${o.ref}^{commit}`]).trim();
  const P = ['--', ...o.paths];

  // since: last app tag (no tag: every branch)
  let since = o.since;
  if (!since) {
    const tag = git(['tag', '--list', `${o.app}-v*`, '--sort=-creatordate']).split('\n')[0];
    if (tag) since = git(['log', '-1', '--format=%cI', `${tag}^{commit}`]).trim();
  }
  const sinceMs = since ? Date.parse(since) : 0;
  if (since && Number.isNaN(sinceMs)) usage(`--since "${since}" is not a date`);

  const allow = parseAllow(existsSync(o.allow ?? join(o.repo, `apps/${o.app}/docs/branch-audit-allow.txt`))
    ? readFileSync(o.allow ?? join(o.repo, `apps/${o.app}/docs/branch-audit-allow.txt`), 'utf8') : '');

  const cachePath = join(o.repo, git(['rev-parse', '--git-path', 'branch-audit-cache.json']).trim());
  let cache = {};
  try { cache = JSON.parse(readFileSync(cachePath, 'utf8')); } catch { /* none yet */ }
  const key = o.paths.join(',');
  const pid = (cache[key] ??= {});
  /** Fills the patch-id cache for commits, in one git log and one patch-id process. */
  const fillPatchIds = (shas) => {
    const todo = shas.filter((s) => !(s in pid));
    if (!todo.length) return;
    const diffs = git(['log', '--no-walk=unsorted', '--stdin', '--no-merges', '-p', '--format=commit %H', ...P], todo.join('\n') + '\n');
    const r = spawnSync('git', ['-C', o.repo, 'patch-id', '--stable'], { encoding: 'utf8', input: diffs, maxBuffer: 512 * 1024 * 1024 });
    for (const l of r.stdout.split('\n')) { const [p, c] = l.split(' '); if (p && c) pid[c] = p; }
    for (const s of todo) pid[s] ??= ''; // empty diff on these paths: no id
  };

  const refCommits = () => git(['rev-list', '--no-merges', refSha, ...(since ? [`--since=${since}`] : []), ...P]).split('\n').filter(Boolean);
  let refIds = null;
  const refFileCache = new Map();
  const refFile = (f) => {
    if (!refFileCache.has(f)) {
      const t = tryGit(['show', `${refSha}:${f}`]);
      refFileCache.set(f, t === null ? null : new Set(t.split('\n')));
    }
    return refFileCache.get(f);
  };
  const inContent = (sha) => {
    const files = parseDiff(git(['show', '--format=', '-U0', '--no-color', sha, ...P]));
    for (const [f, { added, removed }] of files) {
      const have = refFile(f);
      const real = (a) => a.filter((l) => l.trim().length >= MIN_LEN);
      if (have === null) { if (added.length) return false; continue; } // added file absent in the ref; a deleted file is gone, as asked
      if (real(added).some((l) => !have.has(l))) return false;
      if (!added.length && real(removed).some((l) => have.has(l))) return false; // removal not applied
    }
    return true;
  };

  const rows = [];
  const refs = git(['for-each-ref', `refs/remotes/${o.remote}`, '--format=%(refname:short)\t%(committerdate:iso-strict)']).split('\n').filter(Boolean);
  for (const line of refs) {
    const [name, date] = line.split('\t');
    if (name === o.remote || name === `${o.remote}/HEAD` || name === o.ref) continue;
    if (sinceMs && Date.parse(date) < sinceMs) continue;
    const commits = git(['rev-list', '--no-merges', `${refSha}..${name}`, ...P]).split('\n').filter(Boolean);
    if (!commits.length) continue;
    refIds ??= (fillPatchIds(refCommits()), new Set(Object.values(pid).filter(Boolean)));
    fillPatchIds(commits);
    const missing = [];
    let byPatch = 0, byContent = 0;
    for (const c of commits) {
      if (pid[c] && refIds.has(pid[c])) byPatch++;
      else if (inContent(c)) byContent++;
      else missing.push(c);
    }
    const status = !missing.length ? 'applied' : allow.has(name) || allow.has(name.replace(`${o.remote}/`, '')) ? 'allowed' : 'missing';
    rows.push({ name, date: date.slice(0, 10), status, total: commits.length, byPatch, byContent, missing,
      reason: allow.get(name) ?? allow.get(name.replace(`${o.remote}/`, '')) ?? '' });
  }
  try { writeFileSync(cachePath, JSON.stringify(cache)); } catch { /* cache is optional */ }

  const note = (r) => r.reason || (r.status === 'applied' ? `${r.byPatch} by patch, ${r.byContent} by content` : `${r.missing.length} of ${r.total} commits not in the ref`);
  console.log(`Branch audit: ${o.paths.join(' ')} against ${o.ref} (${refSha.slice(0, 9)}), branches updated since ${since ?? 'ever'}`);
  console.log('| branch | status | updated | commits | note |\n|---|---|---|---|---|');
  for (const r of rows.sort((a, b) => a.name.localeCompare(b.name))) console.log(`| ${r.name} | ${r.status} | ${r.date} | ${r.total} | ${note(r)} |`);
  const bad = rows.filter((r) => r.status === 'missing');
  for (const r of bad) {
    const subj = git(['log', '-1', '--format=%h %s', r.missing[0]]).trim();
    console.log(`::error::branch-audit: ${r.name} has ${r.missing.length} commit(s) touching ${o.paths[0]} that are not in ${o.ref}, first ${subj}. ` +
      `Next step: merge ${r.name} into the release branch, or add "${r.name.replace(`${o.remote}/`, '')}  <reason>" to apps/${o.app}/docs/branch-audit-allow.txt.`);
  }
  console.log(`${rows.length} branch(es) with commits outside the ref, ${bad.length} missing.`);
  return bad.length ? 1 : 0;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) process.exit(run(parseArgs(process.argv.slice(2))));
