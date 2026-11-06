import { P, save, link, hub } from './common.mjs';

function deployment(file, os, o) {
  const d = P({ title: `Dyno — deployment on ${os.name}`, kind: 'UML · Deployment diagram', w: 1200, desc: o.desc });
  const NH = 172, CW = 280, G = 120, gx = 48, c0 = gx + 24, c1 = c0 + CW + G, c2 = c1 + CW + G, y0 = 144;
  const r = (i) => y0 + 40 + i * (NH + 32);
  const gw = 24 + 3 * CW + 2 * G + 24, gh = 40 + 2 * NH + 32 + 24;
  d.group('m', gx, y0, gw, gh, `${os.name} [${os.ver}]`);
  d.node('app', c0, r(0), CW, NH, 'accent', o.app);
  d.node('sm', c0, r(1), CW, NH, 'process', o.sm);
  d.node('agent', c1, r(0), CW, 2 * NH + 32, 'accent', o.agent);
  d.node('data', c2, r(0), CW, NH, 'process', o.data);
  d.node('helper', c2, r(1), CW, NH, 'accent', o.helper);
  hub(d, 'agent', 'L', 'app', 'R', { rev: true, label: 'Requests' });
  hub(d, 'agent', 'L', 'sm', 'R', { rev: true, label: 'Starts' });
  hub(d, 'agent', 'R', 'data', 'L', { label: 'Reads, writes' });
  hub(d, 'agent', 'R', 'helper', 'L', { label: 'Asks' });
  d.legend([{ kind: 'accent', label: 'Dyno artefact' }, { kind: 'process', label: 'OS facility · data · process' }, { kind: 'line', label: 'Communication' }]);
  save(file, d, y0 + gh);
}

deployment('uml-deployment-macos', { name: 'macOS', ver: 'macOS 13 or newer, Apple Silicon and Intel' }, {
  desc: 'The app and the agent are separate processes of the same user; a launch agent starts the agent. The optional helper is a launchd system daemon, turned on in Settings.',
  app: { name: 'Dyno.app', tech: 'ad-hoc signed bundle, .dmg', desc: 'Tauri shell and Angular UI on WKWebView; dyno-agent and the dyno command bundled inside.' },
  sm: { name: 'launchd', tech: 'gui/<uid> domain', desc: 'net.decryptology.dyno.agent.plist in ~/Library/LaunchAgents runs dyno-agent run; KeepAlive.' },
  agent: { name: 'dyno-agent', tech: 'user process', desc: 'Samples, keeps history, raises alarms. Log in ~/Library/Logs/Dyno/agent.log.' },
  helper: { name: 'dyno-helper', tech: 'launchd daemon, optional', desc: '/Library/PrivilegedHelperTools/\u200bnet.decryptology.dyno.helper. Reads what needs administrator rights, sets Dyno\'s scheduled wakes (pmset) and installs the pinned disk-health tool on request, with an audit log.' },
  data: { name: 'Data folder', tech: '~/Library/Application Support/Dyno', desc: 'dyno.db, settings.json, agent.json: Dyno\'s own data only.' },
});
deployment('uml-deployment-windows', { name: 'Windows', ver: 'Windows 10 / 11, x64' }, {
  desc: 'The agent runs in each user\'s own session, started by a Run entry at logon or by the app. Dyno installs once per PC in Program Files; the optional helper is its one SCM service.',
  app: { name: 'Dyno', tech: 'NSIS installer, per-machine', desc: 'Tauri shell and Angular UI on WebView2; dyno-agent.exe and dyno.exe beside it.' },
  sm: { name: 'HKCU Run entry', tech: 'value "Dyno agent"', desc: 'Starts dyno-agent.exe run at logon, in the user\'s session.' },
  agent: { name: 'dyno-agent.exe', tech: 'user process', desc: 'Samples, keeps history, raises alarms; reads performance counters.' },
  helper: { name: 'DynoHelper', tech: 'SCM service, optional', desc: 'dyno-helper.exe in Program Files\\Dyno Helper, starts with Windows. Reads what needs administrator rights and installs the pinned disk-health tool on request, with an audit log.' },
  data: { name: 'Data folder', tech: '%LOCALAPPDATA%\\Dyno', desc: 'dyno.db, settings.json, agent.json: Dyno\'s own data only.' },
});
deployment('uml-deployment-linux', { name: 'Linux', ver: 'x86_64 and aarch64 desktops' }, {
  desc: 'A systemd user unit keeps the agent running; the app is the user\'s window. The optional helper is a sandboxed system unit, turned on in Settings.',
  app: { name: 'Dyno', tech: '.deb or AppImage, WebKitGTK', desc: 'Tauri shell and Angular UI; dyno-agent and dyno beside it; the .deb puts dyno on PATH.' },
  sm: { name: 'systemd --user', tech: 'dyno-agent.service', desc: '~/.config/systemd/user; Restart=always, Nice=10, MemoryHigh=256M.' },
  agent: { name: 'dyno-agent', tech: 'user process', desc: 'Samples from /proc and /sys, keeps history, raises alarms.' },
  helper: { name: 'dyno-helper', tech: 'systemd system unit, optional, .deb only', desc: '/usr/bin/dyno-helper serve, sandboxed, no network. Reads what needs administrator rights, sets Dyno\'s scheduled wakes (systemd timers) and installs the pinned disk-health tool on request, with an audit log.' },
  data: { name: 'Data folder', tech: '~/.local/share/dyno', desc: 'dyno.db, settings.json, agent.json: Dyno\'s own data only.' },
});
