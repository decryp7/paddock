// C50: every app on the shared shell lets the shell open its start, and tests it (ADR 0029; rule 43; ER-21). Five apps
// opened their main window from the shell's `setup`, inside Tauri's plugin set-up, where building a webview deadlocks the
// main thread (Spotter 1.0 started with no window, 2026-10-29). The shell now opens the tray and the main window itself
// (`Builder::tray`, `Builder::launch`, `Builder::add_to`); an app (a `src-tauri/Cargo.toml` depending on `paddock-shell`)
// must not call `paddock_window::tauri::start` in its own sources, and one of its Rust tests must run its composed start
// through `paddock_shell::testing::start` (the main window opens; `--hidden` with a tray opens none).
import { error } from '../finding.mjs';

const code = (text) => text.replace(/\/\/.*$/gm, '');

/** What is wrong with one app: `tests` and `sources` are the texts of its Rust tests and its other Rust sources. */
export function startProblems({ tests, sources }) {
  const out = [];
  const tested = tests.some((t) => {
    const c = code(t);
    return /paddock_shell::testing::start\b/.test(c) || (/paddock_shell::testing::\{[^}]*\bstart\b/.test(c) && /\bstart\s*\(/.test(c));
  });
  if (!tested) out.push('no start test: none of its Rust tests runs the composed start through paddock_shell::testing::start');
  if (sources.some((s) => /paddock_window::tauri::start\s*\(/.test(code(s)))) {
    out.push('opens its own main window (paddock_window::tauri::start): the shell opens it (Builder::launch and Builder::add_to)');
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
