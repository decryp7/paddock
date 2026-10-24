// C16: no spec, e2e file or mock hard-codes the current app version (rule 6). Catches T04, U08 (Debrief's 0.1.0, Dyno's spec).
import { error } from '../finding.mjs';
import { appVersion } from '../util/version.mjs';

const isSpec = (f) => /\.(?:spec|test)\.[mc]?[jt]sx?$/.test(f) || /(^|\/)(?:e2e|tests?)\/.*\.[mc]?[jt]sx?$/.test(f);
const isMock = (f) => /(^|\/)[^/]*mock[^/]*(?:\/|\.[mc]?[jt]s$)/.test(f) && /\.[mc]?[jt]s$/.test(f);

export default {
  id: 'C16',
  title: 'No test or mock hard-codes the current app version',
  rule: 6,
  run(ctx) {
    const version = appVersion(ctx);
    if (!version) return [error('C16', 6, ctx.config.tauriDir, 1, 'cannot read the app version, so nothing can be compared', 'Write the version in tauri.conf.json or package.json.')];
    const esc = version.replace(/\./g, '\\.');
    const quoted = new RegExp(`['"\`]${esc}['"\`]|\\bv?${esc}\\b`);
    const out = [];
    const roots = [ctx.config.appDir, ctx.config.tauriDir].map((d) => `${d}/`);
    for (const f of ctx.files()) {
      if (!roots.some((r) => f.startsWith(r)) || !/\.[mc]?[jt]sx?$/.test(f) || /\.generated\./.test(f)) continue;
      const spec = isSpec(f);
      const mock = !spec && isMock(f);
      if (!spec && !mock) continue;
      const lines = (ctx.read(f) ?? '').split('\n');
      lines.forEach((l, i) => {
        if (!quoted.test(l) || /^\s*(?:\/\/|\*)/.test(l)) return;
        // Specs: a mention next to app info / about / update status or as `Version x`. Mocks: an assignment to an app-version key, or to `version` next to app info / update status.
        const near = lines.slice(Math.max(0, i - 3), i + 1).join(' ');
        const appKey = /\b(?:currentVersion|appVersion|app_version)\s*:\s*['"`]/.test(l);
        const plainKey = /\bversion\s*:\s*['"`]/.test(l) && /app.?info|about|updat|appStatus/i.test(near);
        const specAbout = spec && (new RegExp(`\\bVersion\\s+v?${esc}\\b`).test(l) || /app.?info|about|currentVersion|appVersion|app_version|\.info\(\)|appUpdate|updatesAvailable|appStatus/i.test(near));
        if ((spec && (specAbout || appKey)) || (!spec && (appKey || plainKey))) {
          out.push(error('C16', 6, f, i + 1, `hard-codes the app version ${version}: ${l.trim().slice(0, 90)}`, 'Read the version from its single source (tauri.conf.json / package.json) and assert only its shape.'));
        }
      });
    }
    return out;
  },
};
