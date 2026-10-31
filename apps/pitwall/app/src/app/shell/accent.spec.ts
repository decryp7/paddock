import { accentProblems, cssRules } from '@decryp7/paddock-ui/shell';
import { ACCENT } from './appearance';

/** Node's `fs`, loaded at run time (the spec programme has no Node types). */
const nodeFs = (): Promise<{ readFileSync(path: string, encoding: 'utf8'): string }> => import(/* @vite-ignore */ ['node', 'fs'].join(':')) as never;

/** Pitwall's accent keeps the contrast, focus-ring and colour-vision targets of the shell's tokens (C33, WCAG 1.4.3, 1.4.11, 2.4.11). */
describe('Pitwall accent', () => {
  it('passes the shell accent checks in both schemes', async () => {
    const tokensCss = (await nodeFs()).readFileSync('../../../ui/dist/paddock-ui/css/tokens.css', 'utf8');
    expect(accentProblems(cssRules(tokensCss), ACCENT)).toEqual([]);
  });
});
