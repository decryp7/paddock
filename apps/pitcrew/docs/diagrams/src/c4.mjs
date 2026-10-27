import { P, save, link, hub, tOf } from './common.mjs';

// ---------- C4 level 1 ----------
{
  const d = P({ title: 'Pitcrew — system context', kind: 'C4 · Level 1 — System context', w: 1344,
    desc: 'Pitcrew keeps CI runners for Gitea and Forgejo going on a build machine. The network is used only to reach the server and the release hosts.' });
  const W = 288, y0 = 144;
  d.node('owner', 48, y0, W, null, 'person', { name: 'Build-machine owner', tech: 'Person', desc: 'Registers runners, watches the dashboard and tray, drains a runner for maintenance, approves updates.', person: true });
    d.node('sm', 48, y0 + d.rect.owner[3] + 40, W, null, 'process', { name: 'OS service manager', tech: 'SCM · launchd · systemd', desc: 'Starts the service at boot or log-in and restarts it.' });
  d.node('cred', 48, y0 + d.rect.owner[3] + 40 + d.rect.sm[3] + 40, W, null, 'process', { name: 'OS credential store', tech: 'Credential Manager · Keychain', desc: 'Holds the read token and registration tokens.' });
  const left = d.rect.cred[1] + d.rect.cred[3] - y0;
  d.node('sys', 528, y0, 288, left, 'accent', { name: 'Pitcrew', tech: 'Tauri app + background service + Rust core',
    desc: 'Starts runners, restarts one that crashed or stalled with back-off and crash-loop protection, updates them between jobs with roll-back, cleans their disks, and shows it all in a window and a tray. The window is a Tauri app in the user\'s session; the service is a separate Rust process that owns the runners; a pure Rust core holds every rule.' });
  const rx = 1008, rw = 288, rh = 96, rg = (left - rh * 3) / 2;
  const rhs = [0, 1, 2].map(() => 0);
  const ext = [
    ['gitea', 'Gitea / Forgejo server', 'External system', 'Runners register and poll for jobs; the app reads outcomes with a read-only token.'],
    ['hosts', 'Runner release hosts', 'dl.gitea.com · gitea.com', 'Latest runner version, binaries and SHA-256 checksums; two hosts must agree.'],
    ['rel', 'Pitcrew releases', 'Gitea release (opt-in)', 'latest.json and signed installers for Pitcrew\'s own updates.'],
  ];
  ext.forEach(([id, name, tech, desc], i) => d.node(id, rx, y0 + i * (rh + rg), rw, rh, 'external', { name, tech, desc }));
  hub(d, 'sys', 'L', 'owner', 'R', { rev: true, label: 'Controls' });
  hub(d, 'sys', 'L', 'sm', 'R', { label: 'Registers' });
  hub(d, 'sys', 'L', 'cred', 'R', { label: 'Stores tokens', dash: false });
  hub(d, 'sys', 'R', 'gitea', 'L', { label: 'Reads outcomes' });
  hub(d, 'sys', 'R', 'hosts', 'L', { label: 'Downloads' });
  hub(d, 'sys', 'R', 'rel', 'L', { label: 'Checks' });
  d.legend([{ kind: 'process', label: 'Person · OS' }, { kind: 'accent', label: 'Pitcrew' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Uses' }]);
  save('c4-1-context', d, y0 + left);
}

// ---------- C4 level 2 ----------
{
  const d = P({ title: 'Pitcrew — containers', kind: 'C4 · Level 2 — Containers', w: 1368,
    desc: 'The window and tray run in the user\'s session; the service owns the runners; the runner processes are the service\'s children.' });
  const NH = 96, ax = 72, bx = ax + 248 + 96, gx = 48, gw = 24 + 248 + 96 + 280 + 24, rx = gx + gw + 120;
  const rows = NH * 3 + 80, gh = 40 + rows + 24, g1 = 144, g2 = g1 + gh + 56;
  const row = (g, i) => g + 40 + i * (NH + 40);
  d.group('g1', gx, g1, gw, gh, 'App process [Tauri 2 · logged-on user]');
  d.node('ui', ax, row(g1, 0), 248, NH, 'ui', { name: 'Angular UI', tech: 'Angular 22 in the OS webview', desc: 'Dashboard, runners, activity, logs, updates, settings.' });
  d.node('tray', ax, row(g1, 2), 248, NH, 'accent', { name: 'Tray icon and menu', tech: 'tray.rs', desc: 'Worst state of all runners; quick actions.' });
  d.node('shell', bx, g1 + 40, 280, rows, 'accent', { name: 'Rust shell', tech: 'app/src-tauri: Tauri commands', desc: 'Validates every argument from the UI, then asks the service through the ServiceClient trait. One subscription thread pushes status events to the window and the tray. Secrets never reach the logs.' });
  d.group('g2', gx, g2, gw, gh, 'Service process [launch agent · Windows service · systemd user service]', { os: true });
  d.node('core', ax, row(g2, 1), 248, NH, 'core', { name: 'pitcrew-core', tech: 'Rust library, no I/O', desc: 'Controller, watchdog, update machine, history, metrics.' });
  d.node('svc', bx, g2 + 40, 280, rows, 'accent', { name: 'Pitcrew service', tech: 'pitcrew-service: Rust', desc: 'One supervisor thread owns every runner; an async IPC server answers the app; worker threads do downloads, swaps and cleanup. State is written atomically to pitcrew.toml, state.json and the logs.' });
  const R = (id, g, i, role, o) => d.node(id, rx, row(g, i), 248, NH, role, o);
  R('rel', g1, 0, 'external', { name: 'Pitcrew releases', tech: 'External system', desc: 'latest.json and signed installers.' });
  R('cred', g1, 1, 'process', { name: 'OS credential store', tech: 'Credential Manager · Keychain', desc: 'Read token, registration tokens.' });
  R('gitea', g1, 2, 'external', { name: 'Gitea / Forgejo server', tech: 'External system', desc: 'Gives runners their jobs.' });
  R('run', g2, 0, 'process', { name: 'Runner processes', tech: 'gitea-runner, one per runner', desc: 'Children of the service.' });
  R('hosts', g2, 1, 'external', { name: 'Runner release hosts', tech: 'dl.gitea.com · gitea.com', desc: 'Version, binary, checksums.' });
  R('sm', g2, 2, 'process', { name: 'OS service manager', tech: 'SCM · launchd · systemd', desc: 'Starts the service.' });
  hub(d, 'shell', 'L', 'ui', 'R', { rev: true, label: 'Commands' });
  hub(d, 'shell', 'L', 'tray', 'R', { label: 'Updates' });
  hub(d, 'shell', 'R', 'rel', 'L', { label: 'Checks' });
  hub(d, 'shell', 'R', 'cred', 'L', { label: 'Tokens' });
  hub(d, 'shell', 'R', 'gitea', 'L', { label: 'Reads outcomes' });
  link(d, 'shell', 'B', 'svc', 'T', { label: 'Requests' });
  hub(d, 'svc', 'L', 'core', 'R', { label: 'Decisions' });
  hub(d, 'svc', 'R', 'run', 'L', { label: 'Starts, signals' });
  hub(d, 'svc', 'R', 'hosts', 'L', { label: 'Downloads' });
  hub(d, 'svc', 'R', 'sm', 'L', { rev: true, label: 'Starts' });
  link(d, 'run', 'T', 'gitea', 'B', { label: 'Polls for jobs', rev: true });
  d.legend([{ kind: 'ui', label: 'Angular UI' }, { kind: 'accent', label: 'Pitcrew container' }, { kind: 'core', label: 'Rust core' }, { kind: 'process', label: 'Process · OS' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Uses' }]);
  save('c4-2-containers', d, g2 + gh);
}
