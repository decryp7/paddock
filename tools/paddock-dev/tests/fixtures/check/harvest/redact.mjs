// Token-shaped test strings out of the frozen tree (ER-14, ER-46): a file the freeze copies may hold an invented token that a test of
// its own needs (heavy's masker test: an invented GitLab token), and the secret scan (gitleaks, .gitleaks.toml) rightly flags every copy of
// it. The copy does not need the value, only its place: each match becomes `REDACTED` padded with `x` to the same length, so every line
// and column the 12 checks answer about stays where it was.
//
//   import { redactSecrets } from './redact.mjs'   freeze.mjs applies it to each file before freezing
//   node redact.mjs <real-tree.json>               rewrites an existing frozen tree in place (its `files` texts only) and prints
//                                                  how many values it replaced; the recorded answer is left alone, since no
//                                                  line or column moves (the Rust parity test, live_*_frozen, holds it to that)
import { readFileSync, writeFileSync } from 'node:fs';
import { isMain } from '../../../../../../conformance/src/util/is-main.mjs';

// The token shapes the secret scan looks for in this repository's fixtures (gitleaks' rules of the same names).
const SHAPES = [
  /glpat-[0-9A-Za-z_-]{20}/g, // gitlab-pat
  /gh[pousr]_[0-9A-Za-z]{36}/g, // github-pat, github-oauth, github-app-token
  /github_pat_[0-9A-Za-z_]{82}/g, // github-fine-grained-pat
  /xox[abprs]-[0-9A-Za-z-]{10,48}/g, // slack tokens
  /(?:A3T[A-Z0-9]|AKIA|ASIA|ABIA|ACCA)[A-Z0-9]{16}/g, // aws-access-token
];

/** `text` with every token-shaped value replaced by `REDACTED` padded with `x` to its own length; and how many were replaced. */
export function redactSecrets(text) {
  let count = 0;
  let out = text;
  for (const shape of SHAPES) {
    out = out.replace(shape, (m) => {
      count += 1;
      return 'REDACTED'.padEnd(m.length, 'x').slice(0, m.length);
    });
  }
  return { text: out, count };
}

if (isMain(import.meta.url)) {
  const [file] = process.argv.slice(2);
  if (!file) {
    console.error('usage: node redact.mjs <real-tree.json>');
    process.exit(2);
  }
  // The file's own text, not a re-serialisation: the frozen tree writes `/Volumes` as `/Volumes` (so the machine-path checks do
  // not read a host path in it), which JSON.stringify would undo. A token holds no character JSON escapes, so it is the same in the
  // file's text as in the value, and a same-length replacement leaves the JSON valid.
  const raw = readFileSync(file, 'utf8');
  const { text, count } = redactSecrets(raw);
  JSON.parse(text);
  writeFileSync(file, text);
  console.log(`${count} token-shaped value(s) redacted in ${file}`);
}
