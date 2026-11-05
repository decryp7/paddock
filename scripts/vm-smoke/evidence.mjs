// Where evidence files (UAC screenshots, actor screenshots) go: the run's evidence folder next to its result, never the host's /tmp, where the
// next run overwrote them. `PADDOCK_EVIDENCE_DIR` is set by the runner (run-win.mjs) before any step runs; each file name is unique per call.
import { mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

let counter = 0;

/** The evidence folder (created), the runner's or, without one, a private temp folder. */
export function evidenceDir() {
  const dir = process.env.PADDOCK_EVIDENCE_DIR || join(tmpdir(), `vmsmoke-evidence-${process.pid}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

/** A unique file path for `label` in the evidence folder: `NN-label.ext` (NN counts up within the process). */
export function evidenceFile(label, ext = 'png') {
  const slug = String(label).replace(/[^A-Za-z0-9._-]+/g, '-').replace(/^-+|-+$/g, '') || 'evidence';
  return join(evidenceDir(), `${String(++counter).padStart(2, '0')}-${slug}.${ext}`);
}
