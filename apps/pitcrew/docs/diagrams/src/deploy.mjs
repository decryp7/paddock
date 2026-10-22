import { P, save, link, hub } from './common.mjs';

function deployment(file, os, o) {
  const d = P({ title: `Pitcrew — deployment on ${os.name}`, kind: 'UML · Deployment diagram', w: 1368, desc: o.desc });
  const NH = 96, gx = 48, c1 = 72, c2 = c1 + 280 + 120, gw = 24 + 280 + 120 + 288 + 24, y0 = 144;
  const r = (i) => y0 + 40 + i * (NH + 40);
  const gh = r(3) + 112 + 24 - y0;
  d.group('m', gx, y0, gw, gh, `${os.name} build machine [${os.ver}]`);
  d.node('app', c1, r(0), 280, NH, 'accent', o.app);
  d.node('cred', c2, r(0), 288, NH, 'process', o.cred);
  d.node('svc', c1, r(1), 280, NH * 2 + 40, 'accent', o.svc);
  d.node('sm', c2, r(1), 288, NH, 'process', o.sm);
  d.node('data', c2, r(2), 288, NH, 'process', o.data);
  d.node('run', c1, r(3), 280, 112, 'process', o.run);
  const rx = gx + gw + 128;
  d.node('rel', rx, r(0), 248, NH, 'external', { name: 'Pitcrew releases', tech: 'Gitea release of Pitcrew', desc: 'latest.json and installers built by CI on tags.' });
  d.node('hosts', rx, r(1), 248, NH, 'external', { name: 'Runner release hosts', tech: 'dl.gitea.com · gitea.com', desc: 'Versions, binaries, SHA-256 checksums.' });
  d.node('gitea', rx, r(3), 248, NH, 'external', { name: 'Gitea / Forgejo server', tech: 'your server', desc: 'Runners poll for jobs; the app reads outcomes.' });
  link(d, 'app', 'B', 'svc', 'T', { label: 'Requests' });
  link(d, 'app', 'R', 'cred', 'L', { label: 'Tokens' });
  hub(d, 'svc', 'R', 'sm', 'L', { rev: true, label: 'Starts' });
  hub(d, 'svc', 'R', 'data', 'L', { label: 'Reads, writes' });
  link(d, 'svc', 'B', 'run', 'T', { label: 'Starts' });
  hub(d, 'm', 'R', 'rel', 'L', { label: 'Checks' });
  hub(d, 'm', 'R', 'hosts', 'L', { label: 'Downloads' });
  hub(d, 'm', 'R', 'gitea', 'L', { label: 'Polls, reads' });
  d.legend([{ kind: 'accent', label: 'Pitcrew artefact' }, { kind: 'process', label: 'OS, process or data' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Communication' }]);
  save(file, d, y0 + gh);
}

const hostsLbl = 'Requests';
deployment('uml-deployment-windows', { name: 'Windows', ver: 'Windows 10 / 11' }, {
  desc: 'The service is the long-lived process; the app is the user\'s window. Requests travel over a named pipe with an ACL.',
  app: { name: 'Pitcrew app', tech: 'Pitcrew.exe: Tauri + WebView2', desc: 'Window and notification-area icon; runs as the logged-on user; installed by the NSIS setup.' },
  cred: { name: 'Credential Manager', tech: 'OS', desc: 'Gitea read token and remembered registration tokens.' },
  svc: { name: 'Pitcrew service', tech: 'pitcrew-service.exe: Windows service, LocalSystem', desc: 'Automatic (delayed) start; recovery restarts after 10 s, 60 s, 5 min. Named pipe \\\\.\\pipe\\pitcrew, ACL.' },
  sm: { name: 'Service Control Manager', tech: 'OS', desc: 'Registered by the app through a UAC prompt.' },
  data: { name: 'Data folder', tech: '%ProgramData%\\Pitcrew', desc: 'pitcrew.toml, state.json, logs; administrators write.' },
  run: { name: 'Runner processes', tech: 'gitea-runner.exe', desc: 'In the logged-on user\'s session by default (desktop jobs work) or in session 0.' },
});
deployment('uml-deployment-macos', { name: 'macOS', ver: 'macOS 13 or newer' }, {
  desc: 'The service is a user launch agent; runners are its children in the user\'s GUI session. Requests travel over a 0600 Unix socket.',
  app: { name: 'Pitcrew app', tech: 'Pitcrew.app: Tauri + WKWebView', desc: 'Window and menu-bar icon; runs as the user; ad-hoc signed, not notarised.' },
  cred: { name: 'Keychain', tech: 'OS', desc: 'Gitea read token and remembered registration tokens.' },
  svc: { name: 'Pitcrew service', tech: 'pitcrew-service run: user launch agent', desc: 'Label net.decryptology.pitcrew.service; RunAtLoad, KeepAlive; runners get the login shell\'s PATH. Unix socket, mode 0600.' },
  sm: { name: 'launchd', tech: 'OS, SMAppService', desc: 'The user approves the agent under Login Items; no administrator rights.' },
  data: { name: 'Data folder', tech: '~/Library/Application Support/Pitcrew', desc: 'pitcrew.toml, state.json, pitcrew.sock; logs in ~/Library/Logs/Pitcrew.' },
  run: { name: 'Runner processes', tech: 'gitea-runner', desc: 'Children of the agent in the user\'s GUI session: keychain, Xcode and simulators work.' },
});
deployment('uml-deployment-linux', { name: 'Linux', ver: 'x86-64 or ARM64 with systemd' }, {
  desc: 'The service is a systemd user service, no root needed. Requests travel over a 0600 Unix socket in the runtime directory.',
  app: { name: 'Pitcrew app', tech: 'Tauri + WebKitGTK; .deb or AppImage', desc: 'Window and status-notifier tray icon; with no tray, closing the window quits the app.' },
  cred: { name: 'Secret Service', tech: 'OS', desc: 'Gitea read token and remembered registration tokens.' },
  svc: { name: 'Pitcrew service', tech: 'pitcrew-service run: systemd user service', desc: '~/.config/systemd/user/pitcrew.service; Restart=always, KillMode=mixed; no root. Unix socket, mode 0600.' },
  sm: { name: 'systemd (user)', tech: 'OS', desc: 'Enabled with systemctl --user; loginctl enable-linger keeps it running when logged out.' },
  data: { name: 'Data folder', tech: '~/.local/share/Pitcrew', desc: 'pitcrew.toml, state.json, logs; socket in $XDG_RUNTIME_DIR/pitcrew.' },
  run: { name: 'Runner processes', tech: 'gitea-runner', desc: 'Children of the service in the user\'s session, with the login shell\'s PATH.' },
});
