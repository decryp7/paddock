/**
 * The start-up splash (ADR 0058; Paddock lesson "never show an empty window while the UI loads"): plain markup inside the
 * window's root element in `index.html`, styled by `paddock.css` (`.pd-splash`, fading in after 400 ms so a fast start
 * never flashes it), replaced by Angular when the root component renders. No inline style or script (CSP).
 *
 * ```html
 * <app-root>
 *   <div class="pd-splash" role="status" data-testid="boot-splash"><div class="pd-splash__logo"></div><span>Starting Pitwall&hellip;</span></div>
 * </app-root>
 * ```
 */

/** The splash's class (styled by `paddock.css`). */
export const SPLASH_CLASS = 'pd-splash';

/**
 * What is wrong with a window's `index.html` for the start-up contract (empty: nothing): the root element `rootSelector`
 * must hold the splash with `role="status"` and a "Starting …" text, and the page must have no inline style or script.
 * Apps run it on their built page in a test or CI step.
 */
export function splashProblems(indexHtml: string, rootSelector: string): string[] {
  const problems: string[] = [];
  const root = new RegExp(`<${rootSelector}[^>]*>([\\s\\S]*?)</${rootSelector}>`).exec(indexHtml);
  if (!root) return [`no <${rootSelector}> root element`];
  const inside = root[1] ?? '';
  const splash = new RegExp(`<div[^>]*class="[^"]*\\b${SPLASH_CLASS}\\b[^"]*"[^>]*>`).exec(inside);
  if (!splash) problems.push(`<${rootSelector}> has no .${SPLASH_CLASS} splash`);
  else {
    if (!/role="status"/.test(splash[0])) problems.push('the splash needs role="status"');
    if (!/Starting\s[^<]+/.test(inside)) problems.push('the splash needs a "Starting …" text');
  }
  if (/\sstyle="/i.test(indexHtml) || /<style[\s>]/i.test(indexHtml)) problems.push('inline styles break the CSP');
  if (/<script(?![^>]*\ssrc=)[^>]*>/i.test(indexHtml)) problems.push('inline scripts break the CSP');
  return problems;
}
