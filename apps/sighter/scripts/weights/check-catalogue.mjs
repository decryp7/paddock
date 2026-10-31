// Checks Sighter's model-weights catalogue (catalogue/model-weights.toml) before the shared asset parser of SW11 exists:
// it parses, every entry has a 64-hex sha256, a size above 0, a URL under the Gitea sighter-weights/<version>/ path that
// ends in the entry's file name, and a licence in the allow-list. Any problem refuses the whole file. Usage:
// node check-catalogue.mjs [path]
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export const ALLOWED_LICENCES = ['Apache-2.0', 'MIT'];
export const URL_PREFIX = 'https://dev.decryptology.net/api/packages/decryp7/generic/sighter-weights/';

/** Parses the small TOML subset the catalogue uses: `key = "string"`, `key = 123`, `[[model]]`, comments. */
export function parse(text) {
  const top = {};
  const models = [];
  let cur = top;
  text.split(/\r?\n/).forEach((raw, i) => {
    const line = raw.trim();
    if (line === '' || line.startsWith('#')) return;
    if (line === '[[model]]') {
      cur = {};
      models.push(cur);
      return;
    }
    const m = /^([a-z_0-9]+) = ("(?:[^"\\]|\\.)*"|\d+)$/.exec(line);
    if (!m) throw new Error(`line ${i + 1}: not a key = "string" or key = number: ${line.slice(0, 60)}`);
    if (Object.hasOwn(cur, m[1])) throw new Error(`line ${i + 1}: duplicate key ${m[1]}`);
    cur[m[1]] = m[2].startsWith('"') ? JSON.parse(m[2]) : Number(m[2]);
  });
  return { top, models };
}

/** Returns the list of problems (empty when the catalogue is fine). */
export function check(text) {
  let parsed;
  try {
    parsed = parse(text);
  } catch (e) {
    return [`does not parse: ${e.message}`];
  }
  const { top, models } = parsed;
  const problems = [];
  if (top.format !== 1) problems.push('format must be 1');
  if (typeof top.version !== 'string' || top.version === '') problems.push('version must be a non-empty string');
  if (!ALLOWED_LICENCES.includes(top.license)) problems.push(`top-level licence ${top.license} is not in ${ALLOWED_LICENCES}`);
  if (models.length === 0) problems.push('no [[model]] entries');
  const seen = new Set();
  for (const e of models) {
    const id = e.key ?? '(no key)';
    if (typeof e.key !== 'string' || e.key === '') problems.push(`${id}: key missing`);
    else if (seen.has(e.key)) problems.push(`${id}: duplicate key`);
    seen.add(e.key);
    if (typeof e.file !== 'string' || !/^[A-Za-z0-9._-]+$/.test(e.file)) problems.push(`${id}: file must be a plain file name`);
    if (!/^[0-9a-f]{64}$/.test(e.sha256 ?? '')) problems.push(`${id}: sha256 must be 64 lowercase hex characters`);
    if (!Number.isInteger(e.size) || e.size <= 0) problems.push(`${id}: size must be above 0`);
    if (!ALLOWED_LICENCES.includes(e.license)) problems.push(`${id}: licence ${e.license} is not in ${ALLOWED_LICENCES}`);
    const want = `${URL_PREFIX}${top.version}/${e.file}`;
    if (e.url !== want) problems.push(`${id}: url must be ${want}`);
  }
  return problems;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const path = process.argv[2] ?? fileURLToPath(new URL('../../catalogue/model-weights.toml', import.meta.url));
  const problems = check(readFileSync(path, 'utf8'));
  if (problems.length) {
    console.error(problems.map((p) => `model-weights: ${p}`).join('\n'));
    process.exit(1);
  }
  console.log('model-weights catalogue: ok');
}
