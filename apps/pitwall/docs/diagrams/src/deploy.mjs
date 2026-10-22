import { P, save, link, hub, hubAt } from './common.mjs';

function deployment(file, os, o) {
  const d = P({ title: `Pitwall — deployment on ${os.name}`, kind: 'UML · Deployment diagram', w: 1368, desc: o.desc });
  const NH = 104, gx = 48, c1 = 72, c2 = c1 + 296 + 112, gw = 24 + 296 + 112 + 288 + 24, y0 = 144;
  const ya = y0 + 40, ys = ya + 128 + 40, yr = ys + 264 + 56;
  const gh = yr + 104 + 24 - y0;
  d.group('m', gx, y0, gw, gh, `${os.name} computer [${os.ver}]`);
  d.node('app', c1, ya, 296, 128, 'accent', o.app);
  d.node('cred', c2, ya, 288, 128, 'process', o.cred);
  d.node('svc', c1, ys, 296, 264, 'accent', o.svc);
  d.node('sm', c2, ys, 288, 128, 'process', o.sm);
  d.node('data', c2, ys + 128 + 40, 288, 112, 'process', o.data);
  d.node('run', c1, yr, 296, 104, 'process', o.run);
  const rx = gx + gw + 128;
  d.node('hosts', rx, Math.round(y0 + gh / 2 - 68), 248, 136, 'external', { name: 'Download hosts', tech: 'HTTPS, allow-listed', desc: 'Hugging Face for models (plus the opt-in, read-only update check); GitHub releases for llama.cpp builds. Only what you ask for.' });
  link(d, 'app', 'B', 'svc', 'T', { label: 'Manages' });
  link(d, 'app', 'R', 'cred', 'L', { label: 'Admin key' });
  hub(d, 'svc', 'R', 'sm', 'L', { rev: true, label: 'Starts' });
  hub(d, 'svc', 'R', 'data', 'L', { label: 'Reads, writes' });
  link(d, 'svc', 'B', 'run', 'T', { label: 'Starts' });
  hubAt(d, 'm', 'R', 'hosts', 'L', 0.3, { label: 'Downloads' });
  hubAt(d, 'm', 'R', 'hosts', 'L', 0.75, { label: 'check only, opt-in', dash: true });
  d.legend([{ kind: 'accent', label: 'Pitwall artefact' }, { kind: 'process', label: 'OS, process or data' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Communication' }]);
  save(file, d, y0 + gh);
}

deployment('uml-deployment-windows', { name: 'Windows', ver: 'Windows 10 / 11, x64' }, {
  desc: 'A thin Windows service starts the real server in the user’s session; the app and tray only manage it. Everything listens on 127.0.0.1 only.',
  app: { name: 'Pitwall.exe', tech: 'Tauri 2 · WebView2', desc: 'Window and tray. Closing the window leaves the server running; a second launch focuses it.' },
  cred: { name: 'Credential Manager', tech: 'OS', desc: 'The app’s copy of the admin key.' },
  svc: { name: 'pitwall-server.exe', tech: 'runs as the logged-on user · 127.0.0.1:11435', desc: 'Started by the Windows service “Pitwall” (pitwall-server --service) in the active session and again when it exits. Engines, GPU access and data belong to the user, not to the service.' },
  sm: { name: 'Service Control Manager', tech: 'OS · LocalSystem supervisor', desc: 'Registered by the NSIS installer; unsigned until signing secrets exist.' },
  data: { name: '%APPDATA%\\Pitwall', tech: 'data folder', desc: 'settings.toml, apps.json, usage.json, presets.json, engine-pins.json, model-updates.json, admin.key, models, engines.' },
  run: { name: 'llama-server.exe children', tech: 'Vulkan, CUDA or CPU build', desc: 'One per loaded model; no console window, below-normal priority, scrubbed environment.' },
});
deployment('uml-deployment-macos', { name: 'macOS', ver: 'Apple Silicon and Intel' }, {
  desc: 'A launch agent registered through SMAppService keeps the server alive; the app and menu-bar item only manage it. Everything listens on 127.0.0.1 only.',
  app: { name: 'Pitwall.app', tech: 'Tauri 2 · WKWebView · menu bar', desc: 'Window and menu-bar item; the window is rebuilt on Dock reopen. A second launch focuses it.' },
  cred: { name: 'Keychain', tech: 'OS', desc: 'The app’s copy of the admin key.' },
  svc: { name: 'pitwall-server', tech: 'runs as the logged-on user · 127.0.0.1:11435', desc: 'Inside the app bundle; gets the login shell’s PATH for external engines. KeepAlive restarts it after a crash or an app update.' },
  sm: { name: 'launchd', tech: 'launch agent via SMAppService', desc: 'Plist net.decryptology.pitwall.service ships in the bundle; the user approves it under Login Items.' },
  data: { name: '~/Library/Application Support/Pitwall', tech: 'data folder', desc: 'settings.toml, apps.json, usage.json, presets.json, engine-pins.json, model-updates.json, admin.key, models, engines.' },
  run: { name: 'llama-server children', tech: 'Metal or CPU build', desc: 'One per loaded model; scrubbed environment, killed with the server.' },
});
deployment('uml-deployment-linux', { name: 'Linux', ver: 'x64 and arm64' }, {
  desc: 'A systemd user service keeps the server alive without administrator rights; the app only manages it. Everything listens on 127.0.0.1 only.',
  app: { name: 'Pitwall app', tech: 'Tauri 2 · WebKitGTK · appindicator', desc: 'Window and tray. With no tray, closing the window quits the app. A second launch focuses it.' },
  cred: { name: 'Secret Service', tech: 'OS', desc: 'The app’s copy of the admin key, or admin.key when the store is unavailable.' },
  svc: { name: 'pitwall-server', tech: 'runs as the user · 127.0.0.1:11435', desc: 'Gets the login shell’s PATH; restarts itself when its binary changed and it is idle. Packaged as .deb and AppImage; not run on a real desktop yet.' },
  sm: { name: 'systemd (user)', tech: '~/.config/systemd/user/pitwall.service', desc: 'Written by pitwall-server install, Restart=always. Running without a login needs loginctl enable-linger.' },
  data: { name: '~/.local/share/pitwall', tech: 'data folder · XDG_DATA_HOME', desc: 'settings.toml, apps.json, usage.json, presets.json, engine-pins.json, model-updates.json, admin.key, models, engines.' },
  run: { name: 'llama-server children', tech: 'CPU, Vulkan or CUDA build', desc: 'One per loaded model; scrubbed environment, killed with the server.' },
});
