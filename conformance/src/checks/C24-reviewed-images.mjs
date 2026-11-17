// C24: every documentation image was reviewed (rule 22), and the review still applies. Under `docs/screenshots` (or the folders
// named in `screenshotDirs` of paddock.conformance.json, e.g. `ui/docs/screenshots`) each image
// has an entry in `hashes.json` (written by @decryp7/paddock-e2e's captureDocScreens) equal to the file's SHA-256, and a row in
// `docs/screenshots/REVIEWED.md` (| image | sha256 | reviewer | date |) whose hash equals the current one: a regenerated image
// that changed needs a new review. Images under `docs/images` (diagrams) need only the REVIEWED.md row, in `docs/images/REVIEWED.md`.
import { createHash } from 'node:crypto';
import { error } from '../finding.mjs';

const IMAGE = /\.(?:png|jpe?g|webp|gif|svg)$/i;
/** The folders checked: the screenshot folders (config `screenshotDirs`, default `docs/screenshots`) and `docs/images`. */
const roots = (config) => [...(config.screenshotDirs ?? ['docs/screenshots']).map((dir) => ({ dir, hashes: true })), { dir: 'docs/images', hashes: false }];

/** Rows of a REVIEWED.md: Map image -> { hash, reviewer, date, line }. Later rows win; rows that are not 4 cells with a 64-digit hash are ignored. */
export function parseReviewed(text) {
  const rows = new Map();
  (text ?? '').split('\n').forEach((raw, i) => {
    if (!raw.trim().startsWith('|')) return;
    const cells = raw.trim().replace(/^\||\|$/g, '').split('|').map((c) => c.trim().replace(/^`|`$/g, ''));
    if (cells.length < 4 || !/^[0-9a-f]{64}$/i.test(cells[1])) return;
    rows.set(cells[0], { hash: cells[1].toLowerCase(), reviewer: cells[2], date: cells[3], line: i + 1 });
  });
  return rows;
}

export default {
  id: 'C24',
  scope: 'both',
  mode: 'gate',
  title: 'Documentation images are reviewed, and the review matches the current image',
  rule: 22,
  run(ctx) {
    const out = [];
    for (const { dir, hashes } of roots(ctx.config)) {
      const images = ctx.files().filter((f) => f.startsWith(`${dir}/`) && IMAGE.test(f)).sort();
      if (!images.length) continue;
      const rel = (f) => f.slice(dir.length + 1);
      let manifest = {};
      if (hashes) {
        const text = ctx.read(`${dir}/hashes.json`);
        if (text === null) out.push(error('C24', 22, `${dir}/hashes.json`, 1, 'missing: the screenshots have no recorded hashes', 'Regenerate with SCREENSHOTS=1 (captureDocScreens writes hashes.json).'));
        else {
          try {
            const parsed = JSON.parse(text);
            if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) throw new Error('not an object');
            manifest = parsed;
          } catch (e) {
            out.push(error('C24', 22, `${dir}/hashes.json`, 1, `cannot be read: ${e.message}`, 'Regenerate with SCREENSHOTS=1.'));
          }
        }
      }
      const reviewedFile = `${dir}/REVIEWED.md`;
      const reviewed = parseReviewed(ctx.read(reviewedFile));
      if (!ctx.exists(reviewedFile)) out.push(error('C24', 22, reviewedFile, 1, `missing: ${images.length} image(s) in ${dir} have no review record`, 'Create REVIEWED.md with a row per image: | image | sha256 | reviewer | date |.'));
      for (const f of images) {
        const name = rel(f);
        const actual = createHash('sha256').update(ctx.tree.readBuffer(f)).digest('hex');
        if (hashes && ctx.exists(`${dir}/hashes.json`)) {
          if (!(name in manifest)) out.push(error('C24', 22, f, 1, 'has no entry in hashes.json', 'Regenerate with SCREENSHOTS=1.'));
          else if (String(manifest[name]).toLowerCase() !== actual) out.push(error('C24', 22, f, 1, 'differs from its hashes.json entry (edited or stale)', 'Regenerate with SCREENSHOTS=1 and have the new image reviewed.'));
        }
        if (!ctx.exists(reviewedFile)) continue;
        const row = reviewed.get(name);
        if (!row) out.push(error('C24', 22, f, 1, `has no review in ${reviewedFile}`, 'Have the image reviewed independently, then add | image | sha256 | reviewer | date |.'));
        else if (row.hash !== actual) out.push(error('C24', 22, reviewedFile, row.line, `${name} changed since it was reviewed (reviewed ${row.hash.slice(0, 12)}, now ${actual.slice(0, 12)})`, 'Review the new image and update its row.'));
        else if (!row.reviewer || !/^\d{4}-\d{2}-\d{2}$/.test(row.date)) out.push(error('C24', 22, reviewedFile, row.line, `${name}: the row needs a reviewer and an ISO date`, 'Fill in the reviewer and the date (YYYY-MM-DD).'));
      }
      if (hashes) {
        const have = new Set(images.map(rel));
        for (const name of Object.keys(manifest)) if (!have.has(name)) out.push(error('C24', 22, `${dir}/hashes.json`, 1, `lists ${name}, which does not exist`, 'Regenerate with SCREENSHOTS=1.'));
      }
      for (const [name, row] of reviewed) if (!images.some((f) => rel(f) === name)) out.push(error('C24', 22, reviewedFile, row.line, `reviews ${name}, which does not exist`, 'Remove the stale row.'));
    }
    return out;
  },
};
