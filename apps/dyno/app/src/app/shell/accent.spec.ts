import { readFileSync } from 'node:fs';
import { accentProblems, cssRules } from '@decryp7/paddock-ui/shell';
import { ACCENT } from './appearance';

/** Dyno's accent keeps the contrast, focus-ring and colour-vision targets of the shell's tokens (C33, WCAG 1.4.3, 1.4.11, 2.4.11). */
describe('Dyno accent', () => {
  it('passes the shell accent checks in both schemes', () => {
    const tokensCss = readFileSync('../../../ui/dist/paddock-ui/css/tokens.css', 'utf8');
    expect(accentProblems(cssRules(tokensCss), ACCENT)).toEqual([]);
  });
});
