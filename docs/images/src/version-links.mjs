// Cache-busting for image links. Servers send long max-age and Cloudflare keys its cache on the query string, so a regenerated
// image only shows up once its link changes. This rewrites image references in Markdown/HTML files to `<path>?v=<hash>`, where
// hash is the first 8 hex characters of the SHA-256 of the image file. Plain Node, no dependencies.
//   node docs/images/src/version-links.mjs <file.md> [more.md ...]
// Only `src`/`srcset` attributes and Markdown links `[text](path)` / `![alt](path)` whose path ends in .png/.svg/.jpg/.jpeg/.gif/.webp, has no
// scheme, and resolves (relative to the Markdown file) to an existing file inside a directory named `images` are touched.
// Everything else (screenshots, external URLs, ordinary links) is left alone. Idempotent: an unchanged image keeps its link.
import { readFileSync, writeFileSync, existsSync, statSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { dirname, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const EXT = /\.(png|svg|jpe?g|gif|webp)$/i;

/** First 8 hex characters of the SHA-256 of a file's bytes. */
export const shortHash = (file) => createHash('sha256').update(readFileSync(file)).digest('hex').slice(0, 8);

/** `ref` with its version replaced (or added), or `ref` unchanged when it does not point at a generated image next to `mdDir`. */
export function versionRef(ref, mdDir) {
  const m = ref.match(/^([^?#]+)(?:\?[^#]*)?(#.*)?$/);
  if (!m || /^[a-z][a-z0-9+.-]*:|^\/\//i.test(m[1]) || !EXT.test(m[1])) return ref;
  const file = resolve(mdDir, decodeURIComponent(m[1]));
  if (!file.split(sep).includes('images') || !existsSync(file) || !statSync(file).isFile()) return ref;
  return `${m[1]}?v=${shortHash(file)}${m[2] ?? ''}`;
}

/** Rewrites the image references in Markdown text; `srcset` may hold several comma-separated candidates. */
export function versionText(text, mdDir) {
  const attr = text.replace(/\b(src|srcset)=(["'])([^"']*)\2/g, (all, name, q, val) => {
    const out = name === 'src' ? versionRef(val, mdDir)
      : val.split(',').map((c) => { const t = c.trim().split(/\s+/); t[0] = versionRef(t[0], mdDir); return t.join(' '); }).join(', ');
    return `${name}=${q}${out}${q}`;
  });
  return attr.replace(/(\]\()([^)\s]+)((?:\s+"[^"]*")?\))/g, (all, a, ref, b) => a + versionRef(ref, mdDir) + b);
}

/** Rewrites a file in place; returns true when it changed. Missing files are skipped. */
export function versionFile(file) {
  if (!existsSync(file)) return false;
  const before = readFileSync(file, 'utf8');
  const after = versionText(before, dirname(resolve(file)));
  if (after === before) return false;
  writeFileSync(file, after);
  return true;
}

/** Versions Paddock's own README and docs/ecosystem.md, plus the profile repo README when $PADDOCK_PROFILE points at its checkout. */
export function versionPaddockDocs(paddockRoot) {
  const files = [resolve(paddockRoot, 'README.md'), resolve(paddockRoot, 'docs/ecosystem.md')];
  if (process.env.PADDOCK_PROFILE) files.push(resolve(process.env.PADDOCK_PROFILE, 'README.md'));
  for (const f of files) if (versionFile(f)) console.log(`versioned image links in ${f}`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  for (const f of process.argv.slice(2)) console.log(`${versionFile(f) ? 'updated  ' : 'unchanged'} ${f}`);
}
