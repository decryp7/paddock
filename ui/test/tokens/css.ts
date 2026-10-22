import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { compile } from 'sass';

const root = join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'projects', 'paddock-ui');

export type Vars = Record<string, string>;

/** Compiles the token stylesheet (the same source the apps consume) and returns every rule's declarations by selector. */
export function readRules(): Map<string, Vars> {
  const css = compile(join(root, 'scss', 'entry-tokens.scss'), { style: 'expanded', quietDeps: true, silenceDeprecations: [] }).css;
  const rules = new Map<string, Vars>();
  // Flatten @media wrappers: the selector keeps a "@media ..." prefix so the test can ask for the exact context.
  const walk = (text: string, prefix: string): void => {
    let i = 0;
    while (i < text.length) {
      const open = text.indexOf('{', i);
      if (open < 0) break;
      const head = text.slice(i, open).replace(/\/\*[\s\S]*?\*\//g, '').trim();
      let depth = 1;
      let j = open + 1;
      while (j < text.length && depth > 0) {
        if (text[j] === '{') depth++;
        else if (text[j] === '}') depth--;
        j++;
      }
      const body = text.slice(open + 1, j - 1);
      if (head.startsWith('@media')) walk(body, `${head} | `);
      else {
        const key = prefix + head.replace(/['"]/g, '').replace(/\s+/g, ' ');
        const vars = rules.get(key) ?? {};
        for (const m of body.matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) vars[m[1] as string] = (m[2] as string).trim();
        rules.set(key, vars);
      }
      i = j;
    }
  };
  walk(css, '');
  return rules;
}

export const ACCENTS = ['debrief', 'pitcrew', 'pitwall', 'dyno', 'spotter'] as const;
export type Accent = (typeof ACCENTS)[number];
export type Scheme = 'dark' | 'light';

/** The resolved custom properties of one scheme + accent, as the browser would compute them (var() chains resolved). */
export function resolve(rules: Map<string, Vars>, scheme: Scheme, accent: Accent): Vars {
  const base = { ...rules.get(':root') };
  const forScheme = rules.get(`:root[data-theme=${scheme}]`) ?? {};
  const forAccent = rules.get(`:root[data-theme=${scheme}][data-accent=${accent}]`) ?? {};
  const merged: Vars = { ...base, ...forScheme, ...forAccent };
  const deref = (v: string, depth = 0): string => {
    const m = /^var\((--[\w-]+)\)$/.exec(v);
    return m && depth < 8 ? deref(merged[m[1] as string] ?? v, depth + 1) : v;
  };
  return Object.fromEntries(Object.entries(merged).map(([k, v]) => [k, deref(v)]));
}

export function readFileText(rel: string): string {
  return readFileSync(join(root, rel), 'utf8');
}
