import { memoryKind, MemoryPacksBackend, TauriPacksBackend, type PacksBackend } from '@decryp7/paddock-ui/shell/packs';
import { tauriIpc } from './demo-ipc';

/** Where the fake packs start in the browser build (`?packs=problems|notes|clean`); clean by default. */
export type FakePacks = 'clean' | 'notes' | 'problems';

export function fakePacks(search: string = globalThis.location?.search ?? ''): FakePacks {
  const wanted = new URLSearchParams(search).get('packs');
  return wanted === 'problems' || wanted === 'notes' ? wanted : 'clean';
}

const PACKS = [
  { name: 'builtin.toml', origin: 'builtin' as const },
  { name: 'my-very-long-pack-file-name-for-the-cache-folders-of-the-office-computers.toml', origin: 'user' as const },
];

let memory: PacksBackend | null = null;

/**
 * The packs over Tauri in the desktop build; in the browser, one in-memory engine per page (the main window and its in-page
 * dialogs share it, as windows share the real engine). `?packs=problems` starts with a refused pack that Reload fixes.
 */
export function demoPacksBackend(): PacksBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriPacksBackend(ipc);
  const mode = fakePacks();
  memory ??= new MemoryPacksBackend({
    onReload: 'fix',
    kinds: [
      memoryKind(
        'notes',
        'Note packs',
        PACKS,
        mode === 'problems' ? [{ file: 'broken.toml', message: 'not valid TOML: expected `=` (line 2, column 3)' }] : [],
        mode === 'notes' ? [{ file: 'my-caches.toml', message: 'unknown key "extra" is ignored' }] : [],
      ),
    ],
  });
  return memory;
}
