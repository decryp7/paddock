import { P, save, link, hub } from './common.mjs';

function deployment(file, os, o) {
  const d = P({ title: `Dyno — deployment on ${os.name}`, kind: 'UML · Deployment diagram', w: 1200, desc: o.desc });
  const NH = 128, CW = 280, G = 120, gx = 48, c0 = gx + 24, c1 = c0 + CW + G, c2 = c1 + CW + G, y0 = 144;
  const r = (i) => y0 + 40 + i * (NH + 32);
  const gw = 24 + 3 * CW + 2 * G + 24, gh = 40 + 2 * NH + 32 + 24;
  d.group('m', gx, y0, gw, gh, `${os.name} [${os.ver}]`);
  d.node('app', c0, r(0), CW, NH, 'accent', o.app);
  d.node('sm', c0, r(1), CW, NH, 'process', o.sm);
  d.node('agent', c1, r(0), CW, 2 * NH + 32, 'accent', o.agent);
  d.node('data', c2, r(0), CW, NH, 'process', o.data);
  d.node('helper', c2, r(1), CW, NH, 'process', { ...o.helper, planned: true });
  hub(d, 'agent', 'L', 'app', 'R', { rev: true, label: 'Requests' });
  hub(d, 'agent', 'L', 'sm', 'R', { rev: true, label: 'Starts' });
  hub(d, 'agent', 'R', 'data', 'L', { label: 'Reads, writes' });
  hub(d, 'agent', 'R', 'helper', 'L', { label: 'On demand', dash: true });
  d.legend([{ kind: 'accent', label: 'Dyno artefact' }, { kind: 'process', label: 'OS facility · data · process' }, { kind: 'planned', label: 'Planned' }, { kind: 'line', label: 'Communication' }]);
  save(file, d, y0 + gh);
}

deployment('uml-deployment-macos', { name: 'macOS', ver: 'macOS 13 or newer, Apple Silicon and Intel' }, {
  desc: 'The app and the agent are separate processes of the same user; a launch agent starts the agent. The helper would be a third, elevated on demand.',
  app: { name: 'Dyno.app', tech: 'ad-hoc signed bundle, .dmg', desc: 'Tauri shell and Angular UI on WKWebView; dyno-agent, dyno-helper and the dyno command bundled inside.' },
  sm: { name: 'launchd', tech: 'gui/<uid> domain', desc: 'net.decryptology.dyno.agent.plist in ~/Library/LaunchAgents runs dyno-agent run; KeepAlive.' },
  agent: { name: 'dyno-agent', tech: 'user process', desc: 'Samples, keeps history, runs rules. Log in ~/Library/Logs/Dyno/agent.log.' },
  data: { name: 'Data folder', tech: '~/Library/Application Support/Dyno', desc: 'dyno.db, settings.json, rules.json, process-control.json, agent.json.' },
  helper: { name: 'dyno-helper', tech: 'daemon, on demand', desc: 'Approved in System Settings or by an authorisation prompt.' },
});
deployment('uml-deployment-windows', { name: 'Windows', ver: 'Windows 10 / 11, x64' }, {
  desc: 'No SCM service on purpose: the agent runs in the user\'s own session, started by a Run entry at logon or by the app.',
  app: { name: 'Dyno', tech: 'NSIS installer, per-user', desc: 'Tauri shell and Angular UI on WebView2; dyno-agent.exe, dyno-helper.exe and dyno.exe beside it.' },
  sm: { name: 'HKCU Run entry', tech: 'value "Dyno"', desc: 'Starts dyno-agent.exe run at logon, in the user\'s session.' },
  agent: { name: 'dyno-agent.exe', tech: 'user process', desc: 'Samples, keeps history, runs rules; named job objects for CPU caps.' },
  data: { name: 'Data folder', tech: '%LOCALAPPDATA%\\Dyno', desc: 'dyno.db, settings.json, rules.json, process-control.json, agent.json.' },
  helper: { name: 'dyno-helper.exe', tech: 'LocalSystem, on demand', desc: 'Started after a UAC prompt; exits when idle.' },
});
deployment('uml-deployment-linux', { name: 'Linux', ver: 'x86_64 and aarch64 desktops' }, {
  desc: 'A systemd user unit keeps the agent running; the app is the user\'s window. The helper would run through pkexec when needed.',
  app: { name: 'Dyno', tech: '.deb or AppImage, WebKitGTK', desc: 'Tauri shell and Angular UI; dyno-agent, dyno-helper and dyno beside it; the .deb puts dyno on PATH.' },
  sm: { name: 'systemd --user', tech: 'dyno-agent.service', desc: '~/.config/systemd/user; Restart=always, Nice=10, MemoryHigh=256M.' },
  agent: { name: 'dyno-agent', tech: 'user process', desc: 'Samples from /proc and /sys, keeps history, runs rules.' },
  data: { name: 'Data folder', tech: '~/.local/share/dyno', desc: 'dyno.db, settings.json, rules.json, agent.json; state in ~/.local/state/dyno.' },
  helper: { name: 'dyno-helper', tech: 'via pkexec, on demand', desc: 'A polkit prompt; exits when idle.' },
});
