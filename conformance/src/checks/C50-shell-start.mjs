// C50: every app on the shared shell lets the shell open its start, and tests it (ADR 0029; rule 43; ER-21). Five apps
// opened their main window from the shell's `setup`, inside Tauri's plugin set-up, where building a webview deadlocks the
// main thread (Spotter 1.0 started with no window, 2026-10-29). The shell now opens the tray and the main window itself
// (`Builder::tray`, `Builder::launch`, `Builder::add_to`); an app (a `src-tauri/Cargo.toml` depending on `paddock-shell`)
// must not open its main window itself (`paddock_window::tauri::start` or `create_main`, by path or `use`) nor give
// `tauri::Builder` a `setup` of its own (it replaces the shell's), must install `paddock_log` (else every shared crate's
// log line is lost), and one of its Rust tests must run its composed start through `paddock_shell::testing::start` and
// check `main_window`.
import { error } from '../finding.mjs';

const code = (text) => text.replace(/\/\/.*$/gm, '');

/** Whether `src` opens the main window itself: `paddock_window::tauri::start` or `create_main`, by path or imported. */
function opensItsOwnWindow(src) {
  if (/paddock_window::tauri::(start|create_main)\b/.test(src)) return true;
  for (const m of src.matchAll(/use\s+paddock_window::tauri::\{([^}]*)\}/g)) {
    if (/(^|[,\s])(start|create_main)\b/.test(m[1])) return true;
  }
  return false;
}

/** Whether `src` gives a `tauri::Builder` a `setup` of its own (it would replace the one `Builder::add_to` gives it). */
function setsUpTauri(src) {
  for (const m of src.matchAll(/tauri::Builder::(default|new)\s*\(\s*\)/g)) {
    const end = src.indexOf(';', m.index);
    if (/\.setup\s*\(/.test(src.slice(m.index, end < 0 ? undefined : end))) return true;
  }
  return false;
}

/** What is wrong with one app: `tests` and `sources` are the texts of its Rust tests and its other Rust sources. */
export function startProblems({ tests, sources }) {
  const out = [];
  const starts = tests.map(code).filter((c) => /paddock_shell::testing::start\b/.test(c) || (/paddock_shell::testing::\{[^}]*\bstart\b/.test(c) && /\bstart\s*\(/.test(c)));
  if (!starts.length) out.push('no start test: none of its Rust tests runs the composed start through paddock_shell::testing::start');
  else if (!starts.some((c) => /\bmain_window\b/.test(c))) out.push('the start test never checks main_window (that the main window opened)');
  const src = sources.map(code);
  if (src.some(opensItsOwnWindow)) {
    out.push('opens its own main window (paddock_window::tauri::start or create_main): the shell opens it (Builder::launch and Builder::add_to)');
  }
  if (src.some(setsUpTauri)) {
    out.push("gives tauri::Builder a setup of its own, which replaces the start Builder::add_to gives it: put the app's set-up in the shell's setup");
  }
  const logs = (c) => /paddock_shell::logging::install\s*\(/.test(c) || (/paddock_log::/.test(c) && /\.install\s*\(/.test(c));
  if (!src.some(logs)) {
    out.push("installs no paddock_log: the shared crates' log lines (the start's error among them) go nowhere");
  }
  return out;
}

export default {
  id: 'C50',
  title: 'Every app on the shell lets the shell open its start, and tests it',
  rule: 43,
  run(ctx) {
    const files = ctx.files();
    const out = [];
    for (const cargoPath of files.filter((f) => /(^|\/)src-tauri\/Cargo\.toml$/.test(f))) {
      if (!/^\s*paddock-shell\s*[.=]/m.test(ctx.read(cargoPath) ?? '')) continue;
      const dir = cargoPath.replace(/Cargo\.toml$/, '');
      const rust = files.filter((f) => f.startsWith(dir) && f.endsWith('.rs'));
      const tests = rust.filter((f) => f.startsWith(`${dir}tests/`)).map((f) => ctx.read(f) ?? '');
      const sources = rust.filter((f) => f.startsWith(`${dir}src/`)).map((f) => ctx.read(f) ?? '');
      for (const message of startProblems({ tests, sources })) {
        out.push(error('C50', 43, cargoPath, 1, message, 'Compose the app in one function (`pub fn app(builder, shell, launch, tray_backend)` with `shell.tray(..).launch(launch).add_to(builder)`), use it in `run`, and test it with `paddock_shell::testing::start` and a `HeadlessTray` (see templates/repo).'));
      }
    }
    return out;
  },
};
