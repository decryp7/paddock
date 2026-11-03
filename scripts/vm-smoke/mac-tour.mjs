// The screenshot and GUI tour of Dyno 1.1.0 on a macOS guest: menu-bar items, the pages, the Settings and About dialogs. Each stop takes a
// screenshot (kept as evidence) and runs the shared raw-text rules of paddock-e2e's screenshot check (ui/projects/paddock-e2e/src/
// screenshot-check.ts, the one implementation) over the texts the app really shows, read through the Accessibility API. The rules that
// need a DOM (clipped text, status chips) cannot run here and are not claimed.
import { fileURLToPath } from 'node:url';
import { dirname, join, resolve } from 'node:path';

const HERE = dirname(fileURLToPath(import.meta.url));
const RULES = resolve(HERE, '..', '..', 'ui', 'projects', 'paddock-e2e', 'src', 'screenshot-check.ts');

/** The shared rules, loaded lazily: they need the repo's node_modules (a dev checkout after `npm ci`). Returns null and why when not loadable. */
export async function loadRules(path = RULES) {
  try { return { rawTextProblems: (await import(path)).rawTextProblems }; } catch (e) { return { error: String(e.message ?? e).split('\n')[0] }; }
}

export class MacTour {
  /** @param gui a MacGui, @param rules from loadRules() */
  constructor(guest, gui, rules, { log = () => {} } = {}) { Object.assign(this, { g: guest, gui, rules, log }); }

  /** One stop: wait for `expect` among the app's texts, shoot, run the raw-text rules. */
  async stop(label, { open, expect, app = this.gui.app }) {
    const checks = [];
    const evidence = [];
    const opened = await open();
    checks.push({ ok: opened, detail: opened ? `opened ${label}` : `could not open ${label}` });
    if (!opened) return this.result(`GUI: ${label}`, checks, evidence);
    const seen = await this.gui.waitFor(async () => (await this.gui.texts(app)).some((t) => expect.test(t.text)), 25_000);
    checks.push({ ok: seen.ok, detail: seen.ok ? `${label} shows its content (/${expect.source}/)` : `${label} never showed /${expect.source}/` });
    evidence.push(`screenshot: ${await this.gui.shot(label.replace(/\W+/g, '-').toLowerCase())}`);
    if (this.rules.error) checks.push({ ok: false, detail: `the shared screenshot rules could not be loaded: ${this.rules.error}` });
    else {
      const { texts, problems } = await this.gui.screenCheck(label, this.rules.rawTextProblems);
      checks.push({ ok: problems.length === 0, detail: problems.length ? problems.slice(0, 3).join('; ') : `${texts} visible texts pass the raw-text rules (no raw identifiers, null/NaN, mock or debug strings)` });
    }
    return this.result(`GUI: ${label}`, checks, evidence);
  }

  result(title, checks, evidence) {
    const failed = checks.filter((c) => !c.ok);
    return { title, status: failed.length ? 'fail' : 'pass', summary: failed.length ? failed.map((c) => c.detail).join('; ').slice(0, 240) : checks.at(-1)?.detail ?? 'ok', checks, evidence };
  }

  /** The menu-bar items: Dyno's status items are windows of the menu bar owner named net.decryptology.dyno.menubar.*. */
  async menuBar() {
    const r = await this.g.exec('/usr/local/bin/axdrive windows');
    const items = r.stdout.split('\n').map((l) => l.split('\t')).filter((c) => /net\.decryptology\.dyno\.menubar/.test(c[3] ?? ''));
    const checks = [{ ok: items.length > 0, detail: `${items.length} Dyno menu-bar item(s): ${items.map((c) => `${c[3].split('.').pop()} ${c[4]}`).join(', ') || 'none'}` }];
    const wide = items.filter((c) => Number(c[4].split(',')[2].split('x')[0]) >= 20);
    checks.push({ ok: wide.length === items.length && items.length > 0, detail: 'every item is at least 20 points wide (a number is readable, not a sliver)' });
    const shot = await this.gui.shot('menu-bar');
    return this.result('GUI: menu-bar items', checks, [`screenshot: ${shot}`, ...r.stdout.split('\n').filter((l) => /dyno/.test(l))]);
  }

  async run() {
    const g = this.gui;
    const steps = [await this.menuBar()];
    const page = (name) => async () => { await g.press(g.app, '^Dashboard$', 'AXLink'); return g.press(g.app, `^${name}$`, 'AXLink'); };
    steps.push(await this.stop('Connections map page', { open: page('Connections map'), expect: /Connections map/ }));
    steps.push(await this.stop('IP lists page', { open: page('IP lists'), expect: /IP lists/ }));
    steps.push(await this.stop('Settings dialog', { open: async () => (await g.openSettingsSection('^Appearance$')).ok, expect: /Appearance/ }));
    await g.press(g.app, '^Close$', 'AXButton');
    steps.push(await this.stop('About dialog', { open: () => g.press(g.app, '^About$', 'AXMenuItem'), expect: /Dyno/ }));
    return steps;
  }
}
