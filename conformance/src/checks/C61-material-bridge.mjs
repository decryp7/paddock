// C61 (report-only: warnings): an app loads the family's Material bridge and no Material theme of its own. Angular Material's
// `mat.theme` (22.2.4) emits density tokens only when the scale is not 0, so an app that loads no `paddock-ui/css/material.css`
// gets Material's own sizes (fields 56px tall, large type, pill buttons: Pitcrew's Appearance page), and an app with its own
// `mat.theme` at another density (Pitwall: -2 on :root) fights the bridge. The shared surfaces carry the family geometry on
// `body` (`_material.scss`), but an app should still not carry a second theme. Warns for now; the app fixes are routed separately.
import { warn } from '../finding.mjs';

/** Style entries of every build target of an `angular.json`. */
export function styleEntries(angularJson) {
  let json;
  try {
    json = JSON.parse(angularJson);
  } catch {
    return null;
  }
  const out = [];
  for (const project of Object.values(json.projects ?? {})) {
    const options = project?.architect?.build?.options;
    for (const s of options?.styles ?? []) out.push(typeof s === 'string' ? s : (s?.input ?? ''));
  }
  return out;
}

/** Whether a stylesheet source declares a Material theme of its own (`mat.theme`, `mat.all-component-themes`, `mat.core`). */
export function ownTheme(scss) {
  return /\bmat\.(theme|all-component-themes|all-component-typographies|core|density|define-theme)\s*\(/.test(scss) || /@include\s+mat\.(theme|all-component-\w+|core)\b/.test(scss);
}

export default {
  id: 'C61',
  title: 'The app loads the family Material bridge and has no Material theme of its own',
  rule: 47,
  run(ctx) {
    const angular = `${ctx.config.appDir}/angular.json`;
    const text = ctx.read(angular);
    if (text === null || text === undefined) return [];
    const findings = [];
    const styles = styleEntries(text);
    if (styles && !styles.some((s) => /paddock-ui\/css\/material\.css$/.test(s))) {
      findings.push(
        warn('C61', 47, angular, 1, `${angular} loads no paddock-ui css/material.css: Material draws with its own sizes (56px fields, large type, pill buttons)`, 'Add "../../../ui/dist/paddock-ui/css/material.css" (after paddock.css and controls.css) to the build styles, as Dyno does.'),
      );
    }
    for (const file of ctx.files()) {
      if (!file.startsWith(`${ctx.config.appDir}/src/`) || !file.endsWith('.scss')) continue;
      const scss = ctx.read(file) ?? '';
      if (ownTheme(scss)) {
        findings.push(
          warn('C61', 47, file, 1, `${file} declares a Material theme of its own (mat.theme or another mat.* theme mixin): it competes with the family bridge`, 'Remove it and load paddock-ui css/material.css, which is the one Material theme (Material 3 mapped to the --pd-* tokens).'),
        );
      }
    }
    return findings;
  },
};
