import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compile } from 'sass';
import { cssRules, resolveTokens } from '../../projects/paddock-ui/shell/src/theme/token-check';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'projects', 'paddock-ui');

export type Vars = Record<string, string>;

/** Compiles the token stylesheet (the same source the apps consume) and returns every rule's declarations by selector. */
export function readRules(): Map<string, Vars> {
  return cssRules(compile(join(root, 'scss', 'entry-tokens.scss'), { style: 'expanded', quietDeps: true, silenceDeprecations: [] }).css);
}

// The accents the apps select (one per app): the same list `applyPaddockAppearance` accepts.
export { SU_ACCENTS as ACCENTS } from '../../projects/paddock-ui/src/lib/appearance';
import type { SuAccent } from '../../projects/paddock-ui/src/lib/appearance';
export type Accent = SuAccent;
export type Scheme = 'dark' | 'light';
/** `more`: the high-contrast mode (`data-contrast="more"` or the OS's Increase Contrast). */
export type Contrast = 'standard' | 'more';

/**
 * The resolved custom properties of one scheme + accent (+ contrast mode), as the browser would compute them (var()
 * chains resolved); the one implementation is the shell's, which apps use on their own accents.
 */
export function resolve(rules: Map<string, Vars>, scheme: Scheme, accent: Accent, contrast: Contrast = 'standard'): Vars {
  return resolveTokens(rules, scheme, accent, contrast);
}

export function readFileText(rel: string): string {
  return readFileSync(join(root, rel), 'utf8');
}
