import { P, save, link, hub } from './common.mjs';

// ---------- C4 level 1 ----------
{
  const d = P({ title: 'Spotter — system context', kind: 'C4 · Level 1 — System context', w: 1344,
    desc: 'Spotter watches release places for new versions and tells you. It is local only: no cloud, no account, no telemetry.' });
  const W = 288, y0 = 144;
  d.node('user', 48, y0, W, null, 'process', { name: 'User', tech: 'Person', desc: 'Adds watches, reads the dashboard and the tray popup, acknowledges releases, deals with pages that need a login or a captcha.', person: true });
  d.node('cred', 48, y0 + d.rect.user[3] + 40, W, null, 'process', { name: 'OS credential store', tech: 'Credential Manager · Keychain · Secret Service', desc: 'Source tokens, channel credentials, site cookies.' });
  d.node('chan', 48, y0 + d.rect.user[3] + 40 + d.rect.cred[3] + 40, W, null, 'external', { name: 'Notification channels', tech: 'Webhook · ntfy · e-mail', desc: 'Extra places a notification is sent to, besides the OS.' });
  const left = d.rect.chan[1] + d.rect.chan[3] - y0;
  d.node('sys', 528, y0, 288, left, 'accent', { name: 'Spotter', tech: 'Tauri 2 desktop app + Rust engine',
    desc: 'Checks each watch on its schedule, orders versions, applies your filters and soak time, keeps a local history, and notifies you through the OS (digests and quiet hours included). Runs actions on a new release: open, download with a checksum, run a program. A pure Rust core holds every rule; a supervised Chromium renders pages that need JavaScript.' });
  const rx = 1008, rw = 288, rh = 128, rg = (left - rh * 3) / 2;
  const ext = [
    ['fs', 'Folders and shares', 'Local disks · mounted drives · UNC', 'Release folders and installer drops, listed on the machine itself.'],
    ['web', 'Release hosts', 'HTTP listings · JSON APIs · forges · feeds · web pages', 'Other people\'s servers; polled politely, with validators and per-host gaps.'],
    ['cr', 'Chromium', 'Edge · Chrome · Chromium', 'A separate browser process that renders pages which only show their content after JavaScript runs.'],
  ];
  ext.forEach(([id, name, tech, desc], i) => d.node(id, rx, y0 + i * (rh + rg), rw, rh, 'external', { name, tech, desc }));
  hub(d, 'sys', 'L', 'user', 'R', { rev: true, label: 'Controls' });
  hub(d, 'sys', 'L', 'cred', 'R', { label: 'Stores secrets' });
  hub(d, 'sys', 'L', 'chan', 'R', { label: 'Notifies' });
  hub(d, 'sys', 'R', 'fs', 'L', { label: 'Lists' });
  hub(d, 'sys', 'R', 'web', 'L', { label: 'Fetches' });
  hub(d, 'sys', 'R', 'cr', 'L', { label: 'Renders with' });
  d.legend([{ kind: 'process', label: 'Person · OS' }, { kind: 'accent', label: 'Spotter' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Uses' }]);
  save('c4-1-context', d, y0 + left);
}

// ---------- C4 level 2 ----------
{
  const d = P({ title: 'Spotter — containers', kind: 'C4 · Level 2 — Containers', w: 1512,
    desc: 'One desktop process holds the shell, the engine and the libraries; the web view shows the UI; Chromium is the only child process.' });
  const NH = 112, gx = 48, ax = 72, AW = 264, bx = ax + AW + 96, BW = 288, cx = bx + BW + 96, CW = 264;
  const gw = cx + CW + 24 - gx, rx = gx + gw + 96, RW = 264;
  const gy = 144, rows = NH * 3 + 80, gh = 40 + rows + 24;
  const row = (i) => gy + 40 + i * (NH + 40);
  d.group('g', gx, gy, gw, gh, 'Spotter app [Tauri 2 · one process, logged-on user]', { os: true });
  d.node('ui', ax, row(0), AW, NH, 'ui', { name: 'Angular UI', tech: 'Angular 22 in the OS web view', desc: 'Main window and tray popup; talks only to the backend service.' });
  d.node('shell', ax, row(1), AW, NH * 2 + 40, 'accent', { name: 'Tauri shell', tech: 'app/src-tauri: Rust', desc: 'Async commands that validate every argument; tray, popup and lifecycle; ShellNotifier and the keyring SecretStore.' });
  d.node('eng', bx, row(0), BW, rows, 'accent', { name: 'spotter-engine', tech: 'Rust library',
    desc: 'The Engine facade, scheduler, polite fetcher with the loopback guard, check pipeline, notification delivery and actions. Owns every watch and its state; writes the database and spotter.toml.' });
  d.node('core', cx, row(0), CW, NH, 'core', { name: 'spotter-core', tech: 'Rust library, no I/O', desc: 'Versions, filters, soak, schedule, notification policy, packs, ports.' });
  d.node('src', cx, row(1), CW, NH, 'accent', { name: 'spotter-sources', tech: 'Rust library', desc: 'Folder listing, bounded HTTP, downloads with resume and checksums.' });
  d.node('rend', cx, row(2), CW, NH, 'accent', { name: 'spotter-render', tech: 'Rust library', desc: 'A Chromium driver over the DevTools protocol, behind PageRenderer.' });
  const R = (id, y, role, o) => d.node(id, rx, y, RW, NH, role, o);
  R('packs', row(0), 'process', { name: 'Packs', tech: 'TOML', desc: 'Bundled presets and version schemes; more in the data folder.' });
  R('hosts', row(1), 'external', { name: 'Release hosts and shares', tech: 'External systems', desc: 'Folders, listings, APIs, forges, feeds, pages.' });
  R('cr', row(2), 'external', { name: 'Chromium', tech: 'Installed or pinned download', desc: 'Off-screen, throwaway profile, caps.' });
  // bottom row, outside the app
  const by = gy + gh + 56;
  d.node('cred', ax, by, AW, NH, 'process', { name: 'OS credential store', tech: 'Keychain · Credential Manager', desc: 'Tokens and cookies.' });
  d.node('data', bx, by, 200, NH, 'process', { name: 'Data folder', tech: 'spotter.db · spotter.toml', desc: 'SQLite state and history; atomic config.' });
  d.node('chan', cx, by, CW, NH, 'external', { name: 'Notification channels', tech: 'Webhook · ntfy · e-mail', desc: 'Delivered by the engine.' });
  link(d, 'ui', 'B', 'shell', 'T', { label: 'Commands' });
  hub(d, 'eng', 'L', 'shell', 'R', { rev: true, label: 'Calls' });
  hub(d, 'eng', 'R', 'core', 'L', { label: 'Decides with' });
  hub(d, 'eng', 'R', 'src', 'L', { label: 'Reads with' });
  hub(d, 'eng', 'R', 'rend', 'L', { label: 'Renders' });
  link(d, 'core', 'R', 'packs', 'L', { label: 'Loads' });
  link(d, 'src', 'R', 'hosts', 'L', { label: 'Reads' });
  link(d, 'rend', 'R', 'cr', 'L', { label: 'Drives' });
  link(d, 'shell', 'B', 'cred', 'T', { label: 'Keeps secrets' });
  link(d, 'eng', 'B', 'data', 'T', { label: 'Saves', t: [100 / 288, 0.5] });
  const ex = d.rect.eng[0] + Math.round(d.rect.eng[2] * 0.9), cy = by + NH / 2;
  d.edge('eng', 'chan', d.port('eng', 'B', 0.9), d.port('chan', 'L', 0.5), { via: [[ex, cy]] });
  d.lbl('Delivers', (ex + cx) / 2, cy, 'a');
  d.legend([{ kind: 'ui', label: 'Angular UI' }, { kind: 'accent', label: 'Spotter container' }, { kind: 'core', label: 'Pure core' }, { kind: 'process', label: 'Data · OS' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Uses' }]);
  save('c4-2-containers', d, by + NH);
}
