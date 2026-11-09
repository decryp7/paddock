#!/usr/bin/env node
// Counts the step outcomes of VM run reports (the `.json` beside each `.md`, report.mjs) per OS, full and partial runs apart, and
// sorts the reasons of check-broken and missing required steps into the closed reason union of ADR 0042's amendment of
// 2026-11-08 (§9): precondition:<id>, probe-timeout, target-not-found, undo-not-undone, kit-error, parse. Reports written
// before the kit carry free text, so the sort is by keywords and every reason it cannot place is counted as `unclassified`
// and listed, never guessed. Rule 48: the numbers quoted in the ADR come from this script.
//
//   node scripts/vm-smoke/round-stats.mjs [--since YYYY-MM-DD] [--list-unclassified] <report.json ...>
import { readFileSync } from 'node:fs';
import { isMain } from '../../conformance/src/util/is-main.mjs';

/** Ordered keyword rules; the first match wins. Pure. */
export const REASON_RULES = [
  ['precondition:install', /install did not (succeed|finish)|no installed tool|per-user Dyno is in this snapshot|paddock-setup\.exe is missing/i],
  ['precondition:login-item-off', /was not switched off before the test/i],
  ['precondition:reboot-done', /the reboot was not tried/i],
  ['precondition:agent-stopped', /agent was still running when|supervisor had already started a new agent|supervisor's new agent could not be found/i],
  ['precondition:second-session', /second console logon|fast-user-switch/i],
  ['precondition:scriptable', /no scripted toggle/i],
  ['precondition:build-commit', /no expected build commit/i],
  ['precondition:helper-on', /helper was not on|helper is not (on|active)/i],
  ['precondition:job-running', /no job could be started|benchmark .*not (start|running)/i],
  ['precondition:agent-steady', /agent was not (killed|running)|no pid found|agent is not running/i],
  ['precondition:old-installer', /old and new build report the same version|no old installer/i],
  ['precondition:vm-capability', /no sleep state|this VM has no|VM exposes no|virtio|no SSD wear|cannot be observed because/i],
  ['precondition:definition', /its definition .* lands with|no code|not implemented|no code path|cannot be updated in place|no control adds one/i],
  ['precondition:old-installer', /old \(.*\) and new \(.*\) versions differ/i],
  ['undo-not-undone', /could not be put back|restoration was|was not restored|not saved \(Save changes\)/i],
  ['kit-error', /command not found|exit 127|frontend lock was locked|could not be run in the guest|credential store failed|config folder was not found|ssh .*(refused|timed out)/i],
  ['parse', /unreadable|could not (read|parse)|could not be read \(nothing\)|reads unknown|cannot read|TypeError|page\.evaluate/i],
  ['target-not-found', /could not be triggered|could not be (found|opened|stopped)|was not running when the kill was due|no .{0,40}(menu|item|window|endpoint|button|entry|page)|sees no menu|did not see|not found|positive control failed|cannot be read here/i],
  ['probe-timeout', /within|timed? ?out|never (showed|appeared|came)|did not (appear|come|clear|finish|move)|after \d+ s|still there \d+ s after/i],
];

/** The reason class of a free-text reason; `unclassified` when no rule matches. Pure. */
export function reasonClass(text) {
  const hit = REASON_RULES.find(([, re]) => re.test(String(text ?? '')));
  return hit ? hit[0] : 'unclassified';
}

/** The OS of a report: its `os` field. */
const osOf = (r) => r.os ?? 'unknown';

const keyOf = (r) => `${osOf(r)} ${r.partial ? 'partial' : 'full'}`;

/** Aggregates parsed reports. Pure. */
export function roundStats(reports) {
  const per = {};
  const reasons = { 'check-broken': {}, missing: {} };
  const byOs = {}; // "<os> <full|partial>" -> { harness, precondition, steps }: the V9 measure (harness-caused check-broken per step)
  const unclassified = [];
  for (const r of reports) {
    const key = keyOf(r);
    const p = (per[key] ??= { runs: 0, steps: 0, status: {} });
    p.runs += 1;
    for (const s of r.steps ?? []) { p.steps += 1; p.status[s.status] = (p.status[s.status] ?? 0) + 1; }
    const key2 = keyOf(r);
    for (const m of r.missing ?? []) {
      const why = String(m.why ?? '');
      const kind = why.startsWith('the step ended check-broken') ? 'check-broken' : 'missing';
      const text = why.replace(/^the step ended [a-z-]+: /, '');
      const cls = /^the step did not run$/.test(why) ? (r.partial ? 'not-selected (partial run)' : 'not-driven') : reasonClass(text);
      reasons[kind][cls] = (reasons[kind][cls] ?? 0) + 1;
      if (kind === 'check-broken') {
        const b = (byOs[key2] ??= { harness: 0, precondition: 0 });
        b[cls.startsWith('precondition:') ? 'precondition' : 'harness'] += 1;
      }
      if (cls === 'unclassified') unclassified.push({ os: osOf(r), id: m.id, kind, text: text.slice(0, 200) });
    }
  }
  for (const [k, b] of Object.entries(byOs)) b.harnessPerStep = per[k].steps ? Math.round((b.harness / per[k].steps) * 1000) / 10 : 0; // percent
  return { per, reasons, byOs, unclassified };
}

if (isMain(import.meta.url)) {
  const args = process.argv.slice(2);
  const since = args.includes('--since') ? args[args.indexOf('--since') + 1] : null;
  const files = args.filter((a, i) => !a.startsWith('--') && args[i - 1] !== '--since');
  const reports = [];
  for (const f of files) {
    const day = (f.match(/(\d{4}-\d\d-\d\d)T/) ?? [])[1];
    if (since && day && day < since) continue;
    try { reports.push(JSON.parse(readFileSync(f, 'utf8'))); } catch (e) { console.error(`skipped ${f}: ${e.message}`); }
  }
  const out = roundStats(reports);
  if (!args.includes('--list-unclassified')) delete out.unclassified;
  console.log(JSON.stringify({ reports: reports.length, ...out }, null, 2));
}
