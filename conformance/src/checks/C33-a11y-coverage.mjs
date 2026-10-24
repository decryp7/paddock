// C33: accessibility is checked for every window (plan X49, WCAG 2.2 AA). Static, from the app's tests:
//   - some e2e spec runs axe (`axeViolations`, `axeInThemes`, `axeTargets`, `axeEveryDialog` of @decryp7/paddock-e2e);
//   - every dialog in `dialog-contracts.json` is covered: either one spec calls `axeEveryDialog(` (all dialogs, both themes)
//     or each dialog id is named in a spec that runs axe; a dialog that cannot open alone is listed in "a11y.skip" with why;
//   - warnings: no keyboard-only flow (`expectTabOrder`, `expectReachableByKeyboard`, `expectEscapeReturnsFocus`, `tabWalk`),
//     and no unit test runs `accentProblems` on the app's accent.
//   "a11y": { "contracts": "app/src-tauri/dialog-contracts.json", "skip": { "db.confirm-reset": "opens only after a reset" } }
import { error, warn } from '../finding.mjs';

const isSpec = (f) => /\.(?:spec|test)\.[mc]?[jt]sx?$/.test(f) || /(^|\/)e2e\/.*\.[mc]?[jt]sx?$/.test(f);
const AXE = /\b(?:axeViolations|axeInThemes|axeTargets|axeEveryDialog|AxeBuilder)\b/;
const KEYBOARD = /\b(?:expectTabOrder|expectReachableByKeyboard|unreachableByKeyboard|expectEscapeReturnsFocus|tabWalk)\b/;

export default {
  id: 'C33',
  title: 'Every window is checked with axe; keyboard flows and the accent are tested',
  rule: 40,
  run(ctx) {
    const appDir = `${ctx.config.appDir}/`;
    const specs = ctx.files().filter((f) => f.startsWith(appDir) && isSpec(f) && !f.includes('node_modules/'));
    const texts = new Map(specs.map((f) => [f, ctx.read(f) ?? '']));
    const axeSpecs = [...texts].filter(([, t]) => AXE.test(t));
    if (!axeSpecs.length) {
      return [error('C33', 40, `${ctx.config.appDir}/e2e`, 1, 'no e2e spec runs axe: nothing checks the windows for accessibility', "Use axeViolations / axeEveryDialog of @decryp7/paddock-e2e in the app's specs, in both themes.")];
    }
    const out = [];
    const contractsFile = ctx.config.a11y?.contracts ?? ctx.files().find((f) => f.startsWith(appDir) && f.endsWith('/dialog-contracts.json') && !f.includes('node_modules/'));
    const contractsText = contractsFile ? ctx.read(contractsFile) : null;
    if (contractsText !== null) {
      let ids;
      try {
        ids = Object.keys(JSON.parse(contractsText).dialogs ?? {});
      } catch (e) {
        return [...out, error('C33', 40, contractsFile, 1, `${contractsFile} is not readable (${e.message})`, 'Regenerate the dialog contracts.')];
      }
      const all = axeSpecs.some(([, t]) => /\baxeEveryDialog\s*\(/.test(t));
      const skip = ctx.config.a11y?.skip ?? {};
      if (!all) {
        for (const id of ids.filter((i) => !(i in skip))) {
          if (!axeSpecs.some(([, t]) => t.includes(id))) {
            out.push(error('C33', 40, contractsFile, 1, `the dialog ${id} is not checked with axe`, `Call axeEveryDialog(page, contractsPath) in an e2e spec (covers every dialog, both themes), or name ${id} in a spec that runs axe.`));
          }
        }
      }
      for (const [id, why] of Object.entries(skip)) {
        if (!ids.includes(id)) out.push(warn('C33', 40, 'paddock.conformance.json', 1, `a11y.skip names ${id}, which is not a dialog any more`, 'Remove the entry.'));
        else if (typeof why !== 'string' || !why.trim()) out.push(warn('C33', 40, 'paddock.conformance.json', 1, `a11y.skip for ${id} gives no reason`, 'Say why the dialog cannot be opened by its own window.'));
      }
    }
    if (![...texts.values()].some((t) => KEYBOARD.test(t))) {
      out.push(warn('C33', 40, `${ctx.config.appDir}/e2e`, 1, 'no spec walks the keyboard-only flow (Tab order, reachability, Escape returns focus)', 'Use expectTabOrder / expectReachableByKeyboard / expectEscapeReturnsFocus of @decryp7/paddock-e2e (WCAG 2.1.1, 2.1.2).'));
    }
    if (![...texts.values()].some((t) => /\baccentProblems\s*\(/.test(t))) {
      out.push(warn('C33', 40, ctx.config.appDir, 1, "no test runs accentProblems on the app's accent colour", "Add a unit test: expect(accentProblems(cssRules(tokensCss), '<accent>')).toEqual([]) (@decryp7/paddock-ui/shell)."));
    }
    return out;
  },
};
