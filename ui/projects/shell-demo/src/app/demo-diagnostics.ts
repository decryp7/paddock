import { MemoryDiagnosticsBackend, TauriDiagnosticsBackend, type DiagnosticsBackend, type FakeSave } from '@decryp7/paddock-ui/shell/diagnostics';
import { tauriIpc } from './demo-ipc';

const MODES: readonly FakeSave[] = ['save', 'dismiss', 'fail', 'hang'];

/** What the fake engine of the browser build does on Save (`?report=dismiss|fail|hang`); it saves by default. */
export function fakeSave(search: string = globalThis.location?.search ?? ''): FakeSave {
  const wanted = new URLSearchParams(search).get('report');
  return MODES.find((m) => m === wanted) ?? 'save';
}

let memory: DiagnosticsBackend | null = null;

/**
 * Report a problem over Tauri in the desktop build; in the browser, one in-memory engine per page (the main window and its
 * in-page dialogs share it, as windows share the real engine), with the files a real report holds.
 */
export function demoDiagnosticsBackend(): DiagnosticsBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriDiagnosticsBackend(ipc);
  memory ??= new MemoryDiagnosticsBackend({
    save: fakeSave(),
    files: [
      { name: 'system-info.json', kind: 'system-info', bytes: 412, summary: 'Paddock Shell Demo, your operating system and processor type, no hardware serial numbers' },
      { name: 'settings.json', kind: 'settings', bytes: 1_380, summary: 'Your settings; passwords, tokens and keys are replaced by ***' },
      { name: 'logs/shell-demo.2026-10-24.log', kind: 'log', bytes: 182_400, summary: '1,204 log lines, the recent part of shell-demo.2026-10-24.log; file contents are never logged' },
      { name: 'logs/shell-demo.2026-10-23.log', kind: 'log', bytes: 96_100, summary: '640 log lines, all of shell-demo.2026-10-23.log; file contents are never logged' },
    ],
  });
  return memory;
}
