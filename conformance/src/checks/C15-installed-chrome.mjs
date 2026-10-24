// C15: no test or script uses the installed Google Chrome; CI requires the pinned test browser, so a skip is a failure.
// Catches T02, T12 (441 GB of Chrome clones).
import { error, warn } from '../finding.mjs';

const CHROME = /Google Chrome\.app|Google\\Chrome\\Application|Program Files[^'"`]*chrome\.exe|\/usr\/bin\/google-chrome|google-chrome(?:-stable)?\b|channel\s*:\s*['"]chrome['"]|channel=['"]chrome['"]/;

/** Test and script files: specs, e2e folders, test dirs, scripts, Playwright config, workflows. */
export function isTestOrScript(f) {
  if (f.startsWith('conformance/') || /(^|\/)docs\//.test(f) || f.endsWith('.md')) return false;
  if (/\.(?:ts|mjs|js|cjs|sh|ps1|ya?ml|json)$/.test(f) === false) return false;
  return /\.(?:spec|test)\.[mc]?[jt]s$/.test(f) || /(^|\/)(?:e2e|tests?|scripts|\.gitea)\//.test(f) || /playwright\.config\./.test(f);
}

export default {
  id: 'C15',
  title: 'No installed Chrome in tests; the pinned test browser is required in CI',
  rule: 7,
  run(ctx) {
    const out = [];
    for (const f of ctx.files().filter(isTestOrScript)) {
      const text = ctx.read(f) ?? '';
      text.split('\n').forEach((l, i) => {
        if (CHROME.test(l) && !/^\s*(?:\/\/|#|\*)/.test(l)) out.push(error('C15', 7, f, i + 1, `references the installed Chrome: ${l.trim().slice(0, 100)}`, 'Use the pinned Chrome for Testing (install-test-browser) or an explicit env var, never the installed browser.'));
      });
    }
    const usesPlaywright = ctx.files().some((f) => /playwright\.config\./.test(f));
    if (usesPlaywright && !ctx.workflows().some((w) => /[A-Z_]*REQUIRE_TEST_BROWSER/.test(w.text))) {
      out.push(warn('C15', 7, ctx.config.workflowsDir, 1, 'Playwright is used but no workflow sets <APP>_REQUIRE_TEST_BROWSER=1, so a missing browser skips instead of failing', 'Set the REQUIRE_TEST_BROWSER env var in the CI test jobs.'));
    }
    return out;
  },
};
