// C03: the app's Tauri icons equal Paddock's (icons/<app>/tauri, by hash), the artwork fills the canvas, and CI runs an icon check.
// Catches B08 (Pitwall 1.0.0 shipped a blank icon).
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { error, warn } from '../finding.mjs';
import { ICONS, decodePng, problems } from '../util/png.mjs';

const sha = (buf) => createHash('sha256').update(buf).digest('hex');

export default {
  id: 'C03',
  scope: 'apps',
  mode: 'gate',
  needs: 'rust',
  title: 'Icons equal Paddock\'s and fill the canvas; CI checks them',
  rule: 22,
  run(ctx) {
    const out = [];
    const dir = `${ctx.config.tauriDir}/icons`;
    if (!ctx.tree.isDir(dir)) return [error('C03', 22, dir, 1, `${dir} does not exist`, 'Install the app icons from paddock/icons/<app>/tauri.')];

    for (const name of ICONS) {
      const file = `${dir}/${name}`;
      if (!ctx.exists(file)) {
        out.push(error('C03', 22, file, 1, `${name} is missing`, 'Copy it from paddock/icons/<app>/tauri.'));
        continue;
      }
      try {
        for (const p of problems(name, decodePng(ctx.tree.readBuffer(file)))) out.push(error('C03', 22, file, 1, p, 'Regenerate the icons from the Paddock artwork; the drawing must fill the canvas.'));
      } catch (e) {
        out.push(error('C03', 22, file, 1, `${name}: ${e.message}`, 'Use an 8-bit RGBA PNG from paddock/icons/<app>/tauri.'));
      }
    }

    const ref = ctx.options.paddock ? join(ctx.options.paddock, 'icons', ctx.app, 'tauri') : null;
    if (!ref) out.push(warn('C03', 22, dir, 1, 'no Paddock checkout given (--paddock): the icons were not compared with the reference', 'Run with --paddock <checkout of paddock>.'));
    else if (!existsSync(ref)) out.push(error('C03', 22, dir, 1, `the reference ${ref} does not exist`, `Add icons/${ctx.app}/tauri to Paddock, or fix --app / --paddock.`));
    else {
      for (const name of readdirSync(ref).sort()) {
        const file = `${dir}/${name}`;
        if (!ctx.exists(file)) out.push(error('C03', 22, file, 1, `${name} is in the Paddock icon set but missing here`, 'Copy the whole icons/<app>/tauri folder.'));
        else if (sha(ctx.tree.readBuffer(file)) !== sha(readFileSync(join(ref, name)))) out.push(error('C03', 22, file, 1, `${name} differs from paddock/icons/${ctx.app}/tauri/${name}`, 'Copy the file from Paddock (the artwork is changed there first).'));
      }
    }

    const wf = ctx.workflows();
    if (!wf.some((w) => /check-icons|paddock-conformance|\.gitea\/actions\/conformance/.test(w.text))) {
      out.push(error('C03', 22, ctx.config.workflowsDir, 1, 'no CI step checks the icons (check-icons or paddock-conformance)', 'Add a step that runs paddock-conformance (C03) before the bundle job.'));
    }
    return out;
  },
};
