// C110: a recorded test fixture holds no real identity. API recordings (CI runs, users, runners) carry full names, e-mail
// addresses, avatar links, logins and runner host names; a fixture in git keeps them in history for good. Every JSON file under a
// `tests/` or `fixtures/` folder is read, and a string under a key such as `full_name`, `email`, `avatar_url`, `login`,
// `username`, `login_name`, `website`, `location`, `hostname` or `runner_name` must be a placeholder (`user`, `example`, `runner-1`, `*@example.com`...).
// Record with `./dev ci record-fixture`, which scrubs these fields on capture. The finding names the file and the path, never the value.
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { error } from '../finding.mjs';

/** Keys whose string value identifies a person, an account or a machine. */
export const IDENTITY_KEYS = new Set(['full_name', 'email', 'avatar_url', 'login', 'login_name', 'username', 'hostname', 'runner_name', 'website', 'location']);

/** Recordings made before this check that still hold CI runner names (infrastructure, not a person). The list may only shrink: re-record a
 * file with `./dev ci record-fixture` and remove it here; never add one. */
export const BASELINE = [
  'tools/paddock-dev/tests/ci-runs/run-7880-jobs.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-701.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-702.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-703.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-704.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-705.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-706.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-707.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-708.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-709.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-710.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-711.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-712.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-713.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-714.json',
  'tools/paddock-dev/tests/ci-runs/timings/jobs-715.json',
];

const PLACEHOLDERS = [
  /^$/,
  /^(user|example|ghost|undefined|localhost)$/,
  /^Example User$/,
  /^runner-\d+$/,
  /^[^@\s]*@example\.(com|invalid)$/,
  /^https:\/\/gitea\.invalid(\/|$)/,
  /^example\/[\w.-]+$/,
];

/** Whether `value` is one of the invented placeholders. */
export function isPlaceholder(value) {
  return PLACEHOLDERS.some((re) => re.test(value));
}

/** The identity keys of a parsed JSON value that hold a non-placeholder string: `{ path, key }`. */
export function identityHits(value, path = '') {
  const out = [];
  if (Array.isArray(value)) {
    value.forEach((v, i) => out.push(...identityHits(v, `${path}[${i}]`)));
  } else if (value && typeof value === 'object') {
    for (const [k, v] of Object.entries(value)) {
      if (IDENTITY_KEYS.has(k) && typeof v === 'string' && !isPlaceholder(v)) out.push({ path: `${path}.${k}`, key: k });
      else out.push(...identityHits(v, `${path}.${k}`));
    }
  }
  return out;
}

const isFixturePath = (p) => p.endsWith('.json') && (/(^|\/)tests\//.test(p) || /(^|\/)fixtures\//.test(p));

export default {
  id: 'C110',
  scope: 'root',
  mode: 'gate',
  title: 'Recorded test fixtures hold no real identity',
  rule: 40,
  run(ctx) {
    const git = ctx.options.git ?? ((args) => execFileSync('git', args, { cwd: ctx.root, encoding: 'utf8', maxBuffer: 64 << 20 }));
    let files;
    try {
      git(['rev-parse', '--show-toplevel']);
      files = git(['ls-files', '--', '.']).split('\n').filter(isFixturePath);
    } catch {
      return []; // not a git checkout (an export): nothing to check
    }
    const findings = [];
    for (const f of files) {
      if (BASELINE.includes(f)) continue;
      let json;
      try {
        json = JSON.parse(readFileSync(join(ctx.root, f), 'utf8'));
      } catch {
        continue; // not JSON (a JSON-lines recording, a template): nothing to read
      }
      const hits = identityHits(json);
      if (hits.length === 0) continue;
      const keys = [...new Set(hits.map((h) => h.key))].join(', ');
      findings.push(error('C110', 40, f, 1, `${f} has ${hits.length} real-looking value(s) under ${keys} (first at ${hits[0].path})`, 'Re-record it with `./dev ci record-fixture` (it scrubs identities), or replace the values with placeholders: user, example, runner-1, a@example.com.'));
    }
    return findings;
  },
};
