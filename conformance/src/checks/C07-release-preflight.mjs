// C07: the release is consistent before a tag exists and checked once more at the tag. Static: the version agrees in
// Cargo.toml, package.json, tauri.conf.json and the CHANGELOG head; the release job `needs` every gate. Preflight
// (--preflight vX.Y.Z): tag commit is not [skip ci], the Gitea API reports a green run for exactly this commit, no
// fix/* or release/* branch has commits missing from the tag. Catches B10, B11, B12.
import { execFileSync } from 'node:child_process';
import { error, warn } from '../finding.mjs';
import { lineOf } from '../util/fs.mjs';
import { versionSources } from '../util/version.mjs';

/** Jobs that publish: by id, or because they run the shared publish script. */
export function releaseJobs(jobs) {
  return jobs.filter((j) => /release|publish/i.test(j.id) || /publish\.mjs|publish-release/.test(j.text));
}

function transitiveNeeds(jobs, id, seen = new Set()) {
  for (const n of jobs.find((j) => j.id === id)?.needs ?? []) {
    if (!seen.has(n)) {
      seen.add(n);
      transitiveNeeds(jobs, n, seen);
    }
  }
  return seen;
}

export function staticFindings(ctx) {
  const out = [];
  const sources = versionSources(ctx);
  if (!sources.length) out.push(error('C07', 25, ctx.config.tauriDir, 1, 'no version found in tauri.conf.json, package.json, Cargo.toml or CHANGELOG.md', 'Write the version in the places the release reads.'));
  const versions = [...new Set(sources.map((s) => s.version))];
  if (versions.length > 1) {
    out.push(error('C07', 25, sources[0].file, 1, `the version differs: ${sources.map((s) => `${s.file}=${s.version}`).join(', ')}`, 'Bump every place together (one release commit).'));
  }

  for (const w of ctx.workflows()) {
    for (const rel of releaseJobs(w.jobs)) {
      const gates = w.jobs.filter((j) => j.id !== rel.id && !ctx.config.nonGates.includes(j.id) && !transitiveNeeds(w.jobs, j.id).has(rel.id));
      const have = transitiveNeeds(w.jobs, rel.id);
      const missing = gates.filter((j) => !have.has(j.id)).map((j) => j.id);
      if (missing.length) out.push(error('C07', 25, w.file, rel.line, `release job "${rel.id}" does not need: ${missing.join(', ')}`, `Add them to \`needs:\` of ${rel.id} so a tag cannot publish past a red or skipped gate.`));
    }
  }
  return out;
}

/** The preflight: `git(args)` returns stdout, `fetchImpl` is fetch. Findings only; never throws for a failed check. */
export async function preflight(ctx, { tag, git, fetchImpl = fetch, api, repo, token }) {
  const out = [];
  const where = `tag ${tag}`;
  if (!/^v\d+\.\d+\.\d+(-[\w.]+)?$/.test(tag)) return [error('C07', 25, where, 1, `${tag} is not a vX.Y.Z tag`, 'Use a semver tag.')];
  let sha;
  try {
    sha = git(['rev-parse', `${tag}^{commit}`]).trim();
  } catch {
    return [error('C07', 25, where, 1, `tag ${tag} does not exist in this checkout`, 'Create the tag on the release commit first (locally), then run the preflight.')];
  }
  const subject = git(['log', '-1', '--format=%B', sha]);
  if (/\[skip ci\]|\[ci skip\]|\[no ci\]|\[skip actions\]/i.test(subject)) out.push(error('C07', 25, where, 1, `the tagged commit ${sha.slice(0, 10)} is [skip ci]: no run exists for it`, 'Tag a commit CI has built, never a [skip ci] commit.'));

  const version = tag.slice(1);
  for (const s of versionSources(ctx)) if (s.version !== version) out.push(error('C07', 25, s.file, 1, `${s.file} says ${s.version}, the tag says ${version}`, 'Bump the file in the release commit and retag.'));

  if (!api || !repo) out.push(warn('C07', 25, where, 1, 'no --api/--repo: the CI result of the tagged commit was not checked', 'Pass --api https://host/api/v1 --repo owner/name and GITEA_TOKEN.'));
  else {
    try {
      const res = await fetchImpl(`${api.replace(/\/+$/, '')}/repos/${repo}/actions/runs?head_sha=${sha}`, { headers: token ? { Authorization: `token ${token}` } : {} });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      const body = await res.json();
      const runs = (body.workflow_runs ?? body.runs ?? []).filter((r) => (r.head_sha ?? sha) === sha);
      if (!runs.length) out.push(error('C07', 25, where, 1, `the Gitea API lists no run for commit ${sha.slice(0, 10)}`, 'Push the commit without [skip ci] and wait for CI.'));
      for (const r of runs) {
        const state = r.conclusion ?? r.status;
        if (state !== 'success') out.push(error('C07', 25, where, 1, `run "${r.name ?? r.display_title ?? r.id}" for ${sha.slice(0, 10)} is ${state}`, 'Fix the failure and re-run; tag only a green commit.'));
      }
    } catch (e) {
      out.push(error('C07', 25, where, 1, `cannot read the CI result from ${api}: ${e.message}`, 'Check the API URL and token; the preflight fails closed.'));
    }
  }

  let prev = '';
  try {
    prev = git(['describe', '--tags', '--abbrev=0', `${sha}^`]).trim();
  } catch {
    /* first release */
  }
  const branches = git(['for-each-ref', '--format=%(refname:short)', 'refs/heads/fix', 'refs/heads/release', 'refs/remotes/origin/fix', 'refs/remotes/origin/release']).split('\n').filter(Boolean);
  for (const b of branches) {
    const range = prev ? `${prev}..${b}` : b;
    const missing = git(['rev-list', '--no-merges', range, `^${sha}`]).split('\n').filter(Boolean);
    if (missing.length) out.push(error('C07', 25, where, 1, `${b} has ${missing.length} commit(s) the tag does not contain (${missing.slice(0, 3).map((c) => c.slice(0, 8)).join(', ')})`, `Merge ${b} before tagging, or list it in docs/release-waivers.md with a reason.`));
  }
  return out;
}

export const defaultGit = (cwd) => (args) => execFileSync('git', args, { cwd, encoding: 'utf8', maxBuffer: 64 << 20 });

export default {
  id: 'C07',
  title: 'Release consistency and tag preflight',
  rule: 25,
  async run(ctx) {
    const out = staticFindings(ctx);
    if (ctx.options.preflight) {
      out.push(
        ...(await preflight(ctx, {
          tag: ctx.options.preflight,
          git: ctx.options.git ?? defaultGit(ctx.root),
          fetchImpl: ctx.options.fetch,
          api: ctx.options.api,
          repo: ctx.options.repo,
          token: ctx.options.token,
        })),
      );
    }
    return out;
  },
};
