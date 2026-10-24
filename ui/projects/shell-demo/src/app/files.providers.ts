import type { EnvironmentProviders, Provider } from '@angular/core';
import { MemoryFilesBackend, provideShellFiles, TauriFilesBackend, type FilesBackend } from '@decryp7/paddock-ui/shell/files';
import type { PickRequest } from '@decryp7/paddock-ui/shell/files';
import { tauriIpc } from './demo-ipc';

/** The files a dialog "chooses" in the browser build, one after the other (fictional paths: nothing is read from any disk). */
export const DEMO_CHOICES: readonly string[] = [
  '/demo/logs/session-2026-10-24.log',
  '/demo/projects/a-very-long-project-folder-name/with/many/nested/folders/that/do/not/fit/in/a-narrow-pane/service-2026-10-24-11-30.log',
  '/demo/exports/report.zip',
];

/**
 * The memory engine as an app's open command would drive it: a chosen file is "opened" and so recorded in the recent list (the
 * Rust side records only after a granted open).
 */
export class DemoFilesBackend extends MemoryFilesBackend {
  private next = 0;

  override async pick(request: PickRequest): Promise<string[]> {
    // Each dialog answers with the next fictional file unless a test queued an answer.
    if (!this.hasChoice()) this.choose(DEMO_CHOICES[this.next++ % DEMO_CHOICES.length] ?? '');
    const chosen = await super.pick(request);
    for (const p of chosen) this.recordOpened('files', p);
    return chosen;
  }
}

/** The recent files the browser build starts with. */
export function demoLists() {
  return {
    lists: [
      { id: 'files', entries: [{ path: DEMO_CHOICES[0] ?? '', openedAt: 1_761_300_000_000 }, { path: DEMO_CHOICES[1] ?? '', openedAt: 1_761_200_000_000 }] },
      { id: 'projects', entries: [] },
    ],
  };
}

let memory: DemoFilesBackend | null = null;

/** User files over Tauri in the desktop build; in the browser, one in-memory engine per page. */
export function demoFilesBackend(): FilesBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriFilesBackend(ipc);
  memory ??= new DemoFilesBackend({ lists: demoLists() });
  return memory;
}

/** What the Files page boots with (loaded with the page, never in the main window's first paint). */
export const FILES_PROVIDERS: readonly (Provider | EnvironmentProviders)[] = [provideShellFiles({ backend: demoFilesBackend })];
