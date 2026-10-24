import { appendFileSync, writeFileSync } from 'node:fs';

const where = (f) => (f.file ? `${f.file}${f.line ? `:${f.line}` : ''}` : '-');

/** Plain log lines; `::error` annotations make Gitea show them on the run. */
export function formatText(result) {
  const out = [];
  for (const f of result.findings) {
    out.push(`${f.severity === 'error' ? '::error' : '::warning'} file=${f.file ?? ''},line=${f.line ?? 1}::${f.check} (rule ${f.rule}) ${f.message} Fix: ${f.fix}`);
    out.push(`${f.severity.padEnd(5)} ${f.check} ${where(f)}  ${f.message}\n        fix: ${f.fix}`);
  }
  for (const w of result.waived) out.push(`waived ${w.check} ${where(w)}  ${w.message} (until ${w.waiver.expires}: ${w.waiver.reason})`);
  out.push(`${result.app}: ${result.checks.length} checks, ${result.errors} errors, ${result.warnings} warnings, ${result.waived.length} waived`);
  return out.join('\n');
}

const esc = (s) => String(s).replace(/\|/g, '\\|').replace(/\n/g, ' ');

/** Markdown for the Gitea job summary. */
export function formatMarkdown(result) {
  const head = `## Conformance: ${result.app}\n\n${result.errors} errors, ${result.warnings} warnings, ${result.waived.length} waived (checks: ${result.checks.join(', ')})\n`;
  if (!result.findings.length && !result.waived.length) return `${head}\nAll checks passed.\n`;
  const rows = result.findings.map((f) => `| ${f.severity} | ${f.check} | ${f.rule} | \`${esc(where(f))}\` | ${esc(f.message)} | ${esc(f.fix)} |`);
  let md = `${head}\n| Severity | Check | Rule | Where | Finding | Fix |\n|---|---|---|---|---|---|\n${rows.join('\n')}\n`;
  if (result.waived.length) {
    md += `\n### Waived\n\n| Check | Where | Reason | Expires |\n|---|---|---|---|\n`;
    md += `${result.waived.map((w) => `| ${w.check} | \`${esc(where(w))}\` | ${esc(w.waiver.reason)} | ${w.waiver.expires} |`).join('\n')}\n`;
  }
  return md;
}

/** Writes conformance.json and the job summary where asked. */
export function writeOutputs(result, { out, summary, env = process.env }) {
  if (out) writeFileSync(out, `${JSON.stringify(result, null, 2)}\n`);
  const md = formatMarkdown(result);
  if (summary) writeFileSync(summary, md);
  if (env.GITHUB_STEP_SUMMARY) appendFileSync(env.GITHUB_STEP_SUMMARY, `${md}\n`);
}
