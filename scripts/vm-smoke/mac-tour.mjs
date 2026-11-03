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
  constructor(guest, gui, rules, { log = () => {}, check = null } = {}) { Object.assign(this, { g: guest, gui, rules, log, check }); }

  /** One stop: wait for `expect` among the app's texts, shoot, run the raw-text rules. */
  async stop(label, { open, expect, app = this.gui.app }) {
    const checks = [];
    const evidence = [];
    const opened = await open();
    checks.push({ ok: opened, detail: opened ? `opened ${label}` : `could not open ${label}` });
    if (!opened) return this.result(`GUI: ${label}`, checks, evidence);
    const seen = await this.gui.waitFor(async () => (await this.gui.texts(app)).some((t) => expect.test(t.text)), 25_000);
    checks.push({ ok: seen.ok, detail: seen.ok ? `${label} shows its content (/${expect.source}/)` : `${label} never showed /${expect.source}/` });
    await this.gui.answerSystemPrompts();
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

  /** Widget titles in the order the dashboard lists them (the AX groups of the web area whose name is a widget title). */
  async widgetOrder() {
    const rows = await this.gui.texts();
    const heads = new Set(['Health', 'Why slow', 'Issues', 'CPU', 'Memory', 'Disk activity', 'Network down', 'GPU', 'Battery']);
    return rows.filter((r) => r.role === 'AXGroup' && heads.has(r.text)).map((r) => r.text);
  }

  /** Customize, drag (a real pointer drag through the HID event tap), Done, then Quit and relaunch: the layout must be kept. */
  async dashboard() {
    const g = this.gui;
    const title = 'GUI: dashboard customize, drag, Done, relaunch keeps the layout';
    const checks = [];
    const evidence = [];
    await g.press(g.app, '^Dashboard$', 'AXLink');
    if (await g.has(g.app, '^Collapse sidebar')) await g.press(g.app, '^Collapse sidebar');
    const edit = await g.waitFor(() => g.press(g.app, '^Edit layout$'), 20_000);
    checks.push({ ok: edit.ok, detail: edit.ok ? 'pressed "Edit layout"' : 'no "Edit layout" button (the window may be too narrow)' });
    if (!edit.ok) return this.result(title, checks, evidence);
    const editing = await g.waitFor(() => g.has(g.app, '^Done$'), 10_000);
    checks.push({ ok: editing.ok, detail: editing.ok ? 'editing mode shows Done, Undo, Add widget' : 'no Done button in editing mode' });
    const before = await this.widgetOrder();
    const [a, b] = before;
    const centre = async (name) => (await this.g.exec(`/usr/local/bin/axdrive center ${JSON.stringify(g.app)} '^${name}' AXGroup`)).stdout.trim();
    const [from, to] = [await centre(a), await centre(b)];
    const drag = await this.g.exec(`/usr/local/bin/axdrive drag ${from} ${to}`);
    checks.push({ ok: drag.code === 0, detail: `dragged "${a}" onto "${b}" (${from} to ${to})` });
    const moved = await g.waitFor(async () => { const o = await this.widgetOrder(); return o[0] !== before[0] ? o : null; }, 15_000);
    checks.push({ ok: moved.ok, detail: moved.ok ? `the order changed: ${moved.value.slice(0, 3).join(', ')} ...` : 'the widget order did not change after the drag' });
    evidence.push(`screenshot: ${await g.shot('dashboard-dragged')}`);
    const after = moved.ok ? moved.value : before;
    const done = await g.press(g.app, '^Done$');
    checks.push({ ok: done, detail: done ? 'pressed Done' : 'no Done button' });
    // Relaunch: the layout is the one the person left.
    if (this.check) {
      await this.check.quit('Quit before the layout relaunch');
      const s = await this.check.startAndJudge({ title: 'Relaunch for the layout check' });
      void s; // the start judgement is its own step elsewhere; here only the layout matters
      await g.waitFor(() => g.has(g.app, '^Dashboard$', 'AXLink'), 60_000);
      await g.press(g.app, '^Dashboard$', 'AXLink');
      const kept = await g.waitFor(async () => { const o = await this.widgetOrder(); return o.length ? o : null; }, 30_000);
      const same = kept.ok && kept.value.slice(0, 3).join('|') === after.slice(0, 3).join('|');
      checks.push({ ok: same, detail: same ? `after the relaunch the layout is kept (${kept.value.slice(0, 3).join(', ')} ...)` : `after the relaunch the order is ${kept.value?.slice(0, 3).join(', ') ?? 'unreadable'}, expected ${after.slice(0, 3).join(', ')}` });
      evidence.push(`screenshot: ${await g.shot('dashboard-after-relaunch')}`);
    }
    return this.result(title, checks, evidence);
  }

  /** Benchmarks > Custom: Processor and Disk (256 MB, the internal drive only), Run; a row must appear in History. */
  async benchmark() {
    const g = this.gui;
    const title = 'GUI: short benchmark run (CPU and Disk, 256 MB)';
    const checks = [];
    const evidence = [];
    if (await g.has(g.app, '^Expand sidebar')) await g.press(g.app, '^Expand sidebar');
    await g.waitFor(() => g.press(g.app, '^Benchmarks$', 'AXLink'), 20_000);
    const custom = await g.waitFor(() => g.press(g.app, '^Custom', 'AXRadioButton'), 15_000);
    checks.push({ ok: custom.ok, detail: custom.ok ? 'chose Custom' : 'no Custom profile' });
    if (!custom.ok) return this.result(title, checks, evidence);
    // The profile starts with Processor and Memory on, Disk off: one press each makes it Processor and Disk.
    await g.press(g.app, '^Memory Bandwidth', 'AXCheckBox');
    await g.press(g.app, '^Disk Reads', 'AXCheckBox');
    const unchecked = await g.has(g.app, '^My Shared Files .*value=0', 'AXCheckBox');
    checks.push({ ok: unchecked, detail: unchecked ? 'only the internal drive is selected' : 'the shared folder drive is selected too' });
    const small = await g.waitFor(() => g.press(g.app, '^256 MB', 'AXRadioButton'), 10_000);
    checks.push({ ok: small.ok, detail: small.ok ? 'Disk test file: 256 MB' : 'no 256 MB size (Disk was not selected)' });
    const run = await g.press(g.app, '^Run benchmarks$');
    checks.push({ ok: run, detail: run ? 'pressed Run benchmarks' : 'no Run benchmarks button' });
    const done = await g.waitFor(async () => { const t = await g.texts(); return !t.some((r) => /No runs yet/.test(r.text)) && !t.some((r) => /^Running|Cancel run/.test(r.text)); }, 600_000);
    checks.push({ ok: done.ok, detail: done.ok ? 'a run appeared in History and nothing is still running' : 'no finished run within 10 minutes' });
    evidence.push(`screenshot: ${await g.shot('benchmark-done')}`);
    if (!this.rules.error) {
      const { texts, problems } = await g.screenCheck('benchmark result', this.rules.rawTextProblems);
      checks.push({ ok: problems.length === 0, detail: problems.length ? problems.slice(0, 3).join('; ') : `${texts} visible texts pass the raw-text rules` });
    }
    return this.result(title, checks, evidence);
  }

  async run() {
    const g = this.gui;
    await g.closeSettings();
    const prompts = await g.answerSystemPrompts();
    this.log(`system prompts answered: ${prompts}`);
    const steps = [await this.menuBar(), await this.dashboard(), await this.benchmark()];
    const page = (name) => async () => { await g.press(g.app, '^Dashboard$', 'AXLink'); return g.press(g.app, `^${name}$`, 'AXLink'); };
    steps.push(await this.stop('Connections map page', { open: page('Connections map'), expect: /Connections map/ }));
    steps.push(await this.stop('IP lists page', { open: page('IP lists'), expect: /IP lists/ }));
    steps.push(await this.stop('Settings dialog', { open: async () => (await g.openSettingsSection('^Appearance$')).ok, expect: /Appearance/ }));
    await g.press(g.app, '^Close$', 'AXButton');
    steps.push(await this.stop('About dialog', { open: () => g.press(g.app, '^About( |$)', 'AXMenuItem'), expect: /Dyno/ }));
    return steps;
  }
}
