#!/usr/bin/env node
// The npm audit of an app's production dependencies, with the family's one list of approved exceptions (npm-audit-allow.json beside
// this script). Every app job runs it in its app folder instead of a bare `npm audit --omit=dev --audit-level=high`:
//
//   node ../../../scripts/npm-audit.mjs                 run `npm audit --omit=dev --json` here and judge it
//   node scripts/npm-audit.mjs --report <file.json>      judge a saved report (tests)
//   node scripts/npm-audit.mjs --today 2026-12-18        judge as of that day (tests; default: today, UTC)
//
// A finding is an advisory (an object in a vulnerability's `via`) at `high` or `critical`. It passes only when the allow list names
// its GHSA id with a reason, an approver, the approval day and a review day (ER-46: the list grows only with the user's approval and
// only shrinks otherwise). On or after its review day an entry no longer counts: the step fails and says to renew or remove it, so an
// exception cannot linger. An entry that matches nothing is a warning (remove it). Exit codes: 0 pass, 1 findings or an expired
// entry, 2 usage or a broken list, 6 the audit could not run (not measured, never a pass).
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { isMain } from '../conformance/src/util/is-main.mjs';

export const ALLOW_FILE = join(dirname(fileURLToPath(import.meta.url)), 'npm-audit-allow.json');
const LEVELS = ['high', 'critical'];
/** The severities npm defines; an advisory of any other severity cannot be ranked, so the report is not measured. */
export const SEVERITIES = ['info', 'low', 'moderate', 'high', 'critical'];
const DAY = /^\d{4}-\d{2}-\d{2}$/;
/** The longest an exception may run: its review day is at most this many days after its approval day. */
export const MAX_DAYS = 90;

/** The number of days since 1970-01-01 of a `YYYY-MM-DD` that is a real day, else NaN. */
function dayNumber(s) {
  if (!DAY.test(s)) return NaN;
  const t = Date.parse(`${s}T00:00:00Z`);
  return Number.isNaN(t) || new Date(t).toISOString().slice(0, 10) !== s ? NaN : t / 86_400_000;
}

/** The allow list, parsed and checked: every entry has an id, a reason, an approver, an approval day and a later review day. */
export function parseAllow(text) {
  let doc;
  try {
    doc = JSON.parse(text);
  } catch (e) {
    throw new Error(`the allow list is not JSON: ${e.message}`);
  }
  if (!Array.isArray(doc?.allow)) throw new Error('the allow list has no "allow" array');
  const seen = new Set();
  return doc.allow.map((e, i) => {
    for (const k of ['id', 'reason', 'approvedBy', 'approved', 'reviewBy']) {
      if (typeof e[k] !== 'string' || e[k].trim() === '') throw new Error(`entry ${i + 1} has no ${k}`);
    }
    if (!/^GHSA-[a-z0-9]{4}-[a-z0-9]{4}-[a-z0-9]{4}$/.test(e.id)) throw new Error(`entry ${i + 1}: ${e.id} is not a GHSA id`);
    // The one judge of an approver word is the paddock-approvals crate (user decision 125); this is its copy, and the shared fixtures
    // (npm-audit-fixtures.json) make both answer alike. A new exception grows a shrink-only list (ER-46): the user approves it.
    const by = e.approvedBy.trim();
    if (by !== 'user') {
      const why = by === 'coordinator' ? 'but this needs the user' : 'must be "coordinator" or "user"';
      throw new Error(`entry ${i + 1} (${e.id}): approvedBy = ${JSON.stringify(by)} ${why}`);
    }
    const [from, to] = [dayNumber(e.approved), dayNumber(e.reviewBy)];
    if (Number.isNaN(from) || Number.isNaN(to)) throw new Error(`entry ${i + 1} (${e.id}): days are YYYY-MM-DD`);
    if (to <= from) throw new Error(`entry ${i + 1} (${e.id}): the review day is not after the approval day`);
    if (to - from > MAX_DAYS) throw new Error(`entry ${i + 1} (${e.id}): the review day is more than ${MAX_DAYS} days after the approval day`);
    if (seen.has(e.id)) throw new Error(`entry ${i + 1}: ${e.id} is listed twice`);
    seen.add(e.id);
    return e;
  });
}

/** The high and critical advisories of an `npm audit --json` report: `{ id, name, severity, title, url }`, one per advisory. */
export function advisories(report) {
  const out = new Map();
  for (const v of Object.values(report.vulnerabilities)) {
    for (const via of v.via ?? []) {
      if (typeof via !== 'object' || !LEVELS.includes(via.severity)) continue;
      const id = /GHSA-[a-z0-9]{4}-[a-z0-9]{4}-[a-z0-9]{4}/.exec(via.url ?? '')?.[0] ?? `npm-${via.source}`;
      out.set(id, { id, name: via.name, severity: via.severity, title: via.title ?? '', url: via.url ?? '' });
    }
  }
  return [...out.values()].sort((a, b) => a.id.localeCompare(b.id));
}

/**
 * What `npm audit --json` said, from its text: `{ report }` when it looked (the parsed report, its `vulnerabilities` an object), else
 * `{ notMeasured: why }`. Not JSON, an `error` object, no `vulnerabilities` object, no dependencies at all and an advisory of an unknown
 * severity are all "did not look", never a pass (ER-33). The Rust reader `parse_npm` answers the same way.
 */
export function measure(text) {
  let report;
  try {
    report = JSON.parse(text);
  } catch {
    const tail = String(text ?? '').split('\n').map((l) => l.trim()).filter(Boolean).slice(-2).join(' | ');
    return { notMeasured: `npm audit printed no JSON (${tail})` };
  }
  if (report?.error) return { notMeasured: `npm audit answered with ${report.error.summary ?? report.error.code ?? 'an error'}` };
  const v = report?.vulnerabilities;
  if (v === null || typeof v !== 'object' || Array.isArray(v)) return { notMeasured: "npm audit's answer has no `vulnerabilities` object" };
  if (report.metadata?.dependencies?.total === 0) return { notMeasured: 'npm audit saw no dependencies (metadata.dependencies.total is 0)' };
  for (const entry of Object.values(v)) {
    for (const via of Array.isArray(entry?.via) ? entry.via : []) {
      if (via !== null && typeof via === 'object' && !SEVERITIES.includes(via.severity)) {
        return { notMeasured: `npm audit named an unknown severity (${JSON.stringify(via.severity)}) for ${via.name ?? 'an advisory'}` };
      }
    }
  }
  return { report };
}

/** The verdict: findings not allowed, entries that expired, entries that matched nothing. */
export function judge(report, allow, today) {
  const found = advisories(report);
  const live = new Map(allow.filter((e) => e.reviewBy > today).map((e) => [e.id, e]));
  const expired = allow.filter((e) => e.reviewBy <= today);
  const ids = new Set(found.map((a) => a.id));
  return {
    blocked: found.filter((a) => !live.has(a.id)),
    allowed: found.filter((a) => live.has(a.id)),
    expired,
    unused: allow.filter((e) => !ids.has(e.id) && e.reviewBy > today),
  };
}

function main(argv) {
  let report = null;
  let today = new Date().toISOString().slice(0, 10);
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--report') report = argv[++i];
    else if (argv[i] === '--today') today = argv[++i];
    else {
      console.error(`::error::npm-audit: unknown argument ${argv[i]}. Usage: node scripts/npm-audit.mjs [--report <file>] [--today YYYY-MM-DD]`);
      return 2;
    }
  }
  let allow;
  try {
    allow = parseAllow(readFileSync(ALLOW_FILE, 'utf8'));
  } catch (e) {
    console.error(`::error::npm-audit: ${ALLOW_FILE} is broken: ${e.message}. Next step: fix the entry (id, reason, approvedBy, approved, reviewBy).`);
    return 2;
  }
  let text;
  if (report) {
    try {
      text = report === '-' ? readFileSync(0, 'utf8') : readFileSync(report, 'utf8');
    } catch (e) {
      console.error(`::error::npm-audit: the report ${report} cannot be read: ${e.message}. Not measured; next step: pass a saved \`npm audit --json\` report.`);
      return 6;
    }
  } else {
    const r = spawnSync('npm', ['audit', '--omit=dev', '--json'], { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 });
    if (r.error || typeof r.stdout !== 'string') {
      console.error(`::error::npm-audit: npm could not run: ${r.error?.message ?? 'no output'}. Not measured; next step: install npm or fix PATH, then run \`npm audit --omit=dev\` in ${process.cwd()}.`);
      return 6;
    }
    text = r.stdout;
  }
  const m = measure(text);
  if (m.notMeasured) {
    console.error(`::error::npm-audit: NOT MEASURED: ${m.notMeasured}. A missing measurement never counts as a pass. Next step: run \`npm audit --omit=dev\` in ${process.cwd()} where npm and the network work, or pass a saved report.`);
    return 6;
  }
  const parsed = m.report;
  const v = judge(parsed, allow, today);
  for (const a of v.allowed) {
    const e = allow.find((x) => x.id === a.id);
    console.log(`npm-audit: ${a.id} (${a.name}, ${a.severity}) allowed until ${e.reviewBy}: ${e.reason} (approved by ${e.approvedBy}, ${e.approved})`);
  }
  for (const e of v.unused) console.log(`::warning::npm-audit: ${e.id} is allowed but no longer found here; remove it from scripts/npm-audit-allow.json when no app reports it.`);
  for (const e of v.expired) {
    console.log(`::error::npm-audit: the exception for ${e.id} expired on ${e.reviewBy}. Next step: renew it with the user's approval (ER-46) or remove it in scripts/npm-audit-allow.json.`);
  }
  for (const a of v.blocked) {
    console.log(`::error::npm-audit: ${a.id} ${a.severity} in ${a.name}: ${a.title} ${a.url}. Next step: update the dependency, or ask the user to approve an exception in scripts/npm-audit-allow.json (ER-46).`);
  }
  const bad = v.blocked.length + v.expired.length;
  console.log(`npm-audit: ${v.allowed.length + v.blocked.length} high or critical advisory(ies), ${v.allowed.length} allowed, ${v.blocked.length} not allowed, ${v.expired.length} expired exception(s).`);
  return bad ? 1 : 0;
}

if (isMain(import.meta.url)) process.exitCode = main(process.argv.slice(2));
