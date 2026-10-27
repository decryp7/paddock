import { P, save, link, hub } from './common.mjs';

function deployment(file, os, o) {
  const d = P({ title: `Spotter — deployment on ${os.name}`, kind: 'UML · Deployment diagram', w: 1368, desc: o.desc });
  const NH = 112, gx = 48, c1 = 72, c2 = c1 + 280 + 120, gw = 24 + 280 + 120 + 288 + 24, y0 = 144;
  const r = (i) => y0 + 40 + i * (NH + 40);
  const gh = 40 + 4 * NH + 3 * 40 + 24;
  d.group('m', gx, y0, gw, gh, `${os.name} machine [${os.ver}]`);
  d.node('tray', c1, r(0), 280, NH, 'ui', o.tray);
  d.node('app', c1, r(1), 280, NH * 3 + 80, 'accent', o.app);
  d.node('cred', c2, r(1), 288, NH, 'process', o.cred);
  d.node('data', c2, r(2), 288, NH, 'process', o.data);
  d.node('br', c2, r(3), 288, NH, 'process', o.br);
  const rx = gx + gw + 128;
  d.node('hosts', rx, r(1), 248, NH, 'external', { name: 'Release hosts', tech: 'HTTP · JSON · feeds · pages', desc: 'Polled politely; validators and per-host gaps.' });
  d.node('chan', rx, r(2), 248, NH, 'external', { name: 'Notification channels', tech: 'Webhook · ntfy · e-mail', desc: 'Delivered by the engine.' });
  link(d, 'tray', 'B', 'app', 'T', { label: 'Shows' });
  hub(d, 'app', 'R', 'cred', 'L', { label: 'Secrets' });
  hub(d, 'app', 'R', 'data', 'L', { label: 'Reads, writes' });
  hub(d, 'app', 'R', 'br', 'L', { label: 'Drives' });
  hub(d, 'm', 'R', 'hosts', 'L', { label: 'Fetches' });
  hub(d, 'm', 'R', 'chan', 'L', { label: 'Delivers' });
  d.legend([{ kind: 'ui', label: 'Tray and popup' }, { kind: 'accent', label: 'Spotter app' }, { kind: 'process', label: 'OS, data or process' }, { kind: 'external', label: 'External system' }, { kind: 'line', label: 'Communication' }]);
  save(file, d, y0 + gh);
}

const data = { name: 'Data folder', tech: 'spotter.db · spotter.toml · packs', desc: 'SQLite state and history, atomic config, extra packs and a downloaded Chromium. SPOTTER_DATA_DIR moves it.' };

deployment('uml-deployment-windows', { name: 'Windows' , ver: '10 or 11' }, {
  desc: 'One process in the user\'s session. Edge is always there, so rendered pages work without a download.',
  tray: { name: 'Notification-area icon and popup', tech: 'tray.rs', desc: 'Left click opens the popup (acrylic); right click shows the menu. The icon follows the taskbar theme.' },
  app: { name: 'Spotter', tech: 'Tauri 2 · WebView2 · NSIS installer', desc: 'The shell, the engine and the libraries in one process, with the Angular UI in WebView2. Closing the window leaves the tray; Quit is the only exit. Start at login opens it hidden. Windows toast notifications, with no console windows for helper programs.' },
  cred: { name: 'Credential Manager', tech: 'keyring', desc: 'Entries named Spotter and scope/key: tokens, channel credentials, site cookies.' },
  data,
  br: { name: 'Microsoft Edge', tech: 'msedge.exe, a child process', desc: 'Tried first; then Chrome, Chromium or the pinned download. Off-screen, throwaway profile.' },
});
deployment('uml-deployment-macos', { name: 'macOS', ver: 'Apple silicon or Intel' }, {
  desc: 'One process in the user\'s session, a menu-bar icon and a Dock icon. A browser is Chrome, Edge or Chromium, or the pinned download.',
  tray: { name: 'Menu-bar icon and popup', tech: 'tray.rs', desc: 'Left click opens the popup (popover material); right click shows the menu. The Dock icon reopens the window.' },
  app: { name: 'Spotter', tech: 'Tauri 2 · WKWebView · .dmg', desc: 'The shell, the engine and the libraries in one process, with the Angular UI in WKWebView. Closing the window destroys it (hidden web views came back blank); the tray and the Dock icon recreate it. Start at login opens it hidden. Notifications through Notification Center.' },
  cred: { name: 'Keychain', tech: 'keyring', desc: 'Items named Spotter and scope/key: tokens, channel credentials, site cookies.' },
  data,
  br: { name: 'Chrome, Edge or Chromium', tech: 'a child process', desc: 'Chrome first, then Edge and Chromium, else the pinned download (arm64 or x64). Off-screen, throwaway profile.' },
});
deployment('uml-deployment-linux', { name: 'Linux', ver: 'desktop distribution' }, {
  desc: 'One process in the user\'s session. Some desktops have no tray; then closing the window quits.',
  tray: { name: 'Tray icon and popup', tech: 'tray.rs', desc: 'Where the desktop offers a tray. Without one, the window is the only way in and closing it quits.' },
  app: { name: 'Spotter', tech: 'Tauri 2 · WebKitGTK · .deb · AppImage', desc: 'The shell, the engine and the libraries in one process, with the Angular UI in WebKitGTK. Second launches reopen the window of the running copy. Start at login opens it hidden. Notifications through the desktop\'s notification service.' },
  cred: { name: 'Secret Service', tech: 'keyring · GNOME Keyring · KWallet', desc: 'Entries named Spotter and scope/key. Without a service, secrets cannot be saved and the app says so.' },
  data,
  br: { name: 'Chrome, Chromium or Edge', tech: 'a child process', desc: 'Found in /usr/bin, /snap/bin and similar places, else the pinned download. Off-screen, throwaway profile.' },
});
