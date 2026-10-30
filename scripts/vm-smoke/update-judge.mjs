// Pure parts of the scripted update test (update.mjs): what the Updates window says, and the local feed's routing. Nothing here
// touches a VM, a socket or the clock, so update-judge.test.mjs covers it all.

/**
 * The state the Updates window is in, from the text it shows (the messages of ui/projects/paddock-ui/shell/about/src/updates-dialog.ts).
 * `version` is the version the window should be talking about. Order matters: a window can show a failure under an old line.
 */
export function updateState(texts, version) {
  const t = texts.join('\n');
  const v = version.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  if (/The last step did not finish\./.test(t)) return { state: 'failed', detail: failureDetail(texts) };
  if (new RegExp(`Version ${v} is downloaded and its signature verified`).test(t)) return { state: 'ready' };
  if (new RegExp(`Installing version ${v}`).test(t)) return { state: 'installing' };
  if (new RegExp(`Downloading version ${v}`).test(t)) return { state: 'downloading' };
  if (new RegExp(`Version ${v} is available`).test(t)) return { state: 'available' };
  if (/You have the latest version\./.test(t)) return { state: 'upToDate' };
  if (/Checking for updates…|Asking the updater…/.test(t)) return { state: 'checking' };
  if (/Updates are not available in this build/.test(t)) return { state: 'unavailable' };
  return { state: 'unknown' };
}

/** The line(s) after "The last step did not finish.", which is where the window puts the reason and the next step. */
export function failureDetail(texts) {
  const i = texts.findIndex((x) => /The last step did not finish\./.test(x));
  return i < 0 ? '' : texts.slice(i + 1, i + 3).join(' ').slice(0, 300);
}

/** The AXStaticText lines of an `axdrive tree` dump, in order. */
export function staticTexts(treeDump) {
  return String(treeDump).split('\n').filter((l) => /AXStaticText {2}/.test(l)).map((l) => l.replace(/^\s*AXStaticText {2}/, '').replace(/ \(.*$/, '').trim()).filter(Boolean);
}

/** The case a feed request belongs to, and what to serve: `/ok/latest.json` is always the CURRENT case's manifest (the old build has
 *  that one URL built in); anything else is a file of the current case's folder. Returns { file } relative to the feed folder, or null. */
export function feedRoute(url, current) {
  const path = decodeURIComponent(new URL(url, 'https://feed.invalid').pathname);
  if (path === '/ok/latest.json') return { file: `${current}/latest.json`, type: 'application/json' };
  const m = /^\/([a-z]+)\/(Spotter_aarch64\.app\.tar\.gz(?:\.sig)?)$/.exec(path);
  if (!m || m[1] !== current) return null;
  return { file: `${m[1]}/${m[2]}`, type: m[2].endsWith('.sig') ? 'text/plain' : 'application/gzip' };
}

/** The version in an Info.plist read by `defaults read`: the app's version, or '' for anything else. */
export function plistVersion(text) {
  const v = String(text ?? '').trim();
  return /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(v) ? v : '';
}

/** The cases of the test, in order, with what each must end with. `tarballRequested`: the feed log must show the download. */
export const CASES = [
  { id: 'ok', title: 'Update installs (signed by the app\'s key)', finalVersion: 'new', tarballRequested: true },
  { id: 'badsig', title: 'Refused: one character of the signature changed', finalVersion: 'old', tarballRequested: true, failed: true },
  { id: 'wrongkey', title: 'Refused: signed by a different key', finalVersion: 'old', tarballRequested: true, failed: true },
  { id: 'down', title: 'Feed unreachable: a clear message, the app stays usable', finalVersion: 'old', tarballRequested: false, failed: true },
];
