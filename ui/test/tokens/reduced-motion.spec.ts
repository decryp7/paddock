import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

// Under reduced motion every element gets `transition-duration`. A non-zero value, even 0.01ms, makes WebKit (WKWebView) start a
// real transition on every property that changes, and the inherited colours of the page then arrive in steps over ~250 ms: after a
// theme change the headings were still the old scheme's colour, which axe measured as a contrast failure (shell-demo-theme).
// Zero means no transition at all: the state changes instantly, as the design guide says.
describe('reduced motion', () => {
  const base = readFileSync(new URL('../../projects/paddock-ui/scss/_base.scss', import.meta.url), 'utf8');
  const rule = /@mixin reduced-motion-everywhere \{([\s\S]*?)\n\}/.exec(base)?.[1] ?? '';

  it('finds the rule', () => expect(rule).toContain('animation-duration'));

  it('turns transitions off instead of making them near-zero', () => {
    expect(rule).toMatch(/transition-duration:\s*0s\s*!important/);
    expect(rule).not.toMatch(/transition-duration:\s*0\.0*1ms/);
  });
});
