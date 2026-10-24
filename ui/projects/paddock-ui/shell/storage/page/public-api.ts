/**
 * `@decryp7/paddock-ui/shell/storage/page`: the Settings > Storage page (`PdStoragePage`), split from `shell/storage` so a
 * main window's first paint carries none of it (plan S16). The app's registry loads it:
 * `load: () => import('@decryp7/paddock-ui/shell/storage/page').then((m) => m.PdStoragePage)`.
 */
export { PdStoragePage } from './src/storage-page';
