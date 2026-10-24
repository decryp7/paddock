/**
 * `@decryp7/paddock-ui/shell/storage/dialog`: the clear confirmation's content (`PdStorageConfirmDialog`), split from
 * `shell/storage` so only its window loads it. The app's registry loads it:
 * `load: () => import('@decryp7/paddock-ui/shell/storage/dialog').then((m) => m.PdStorageConfirmDialog)`.
 */
export { PdStorageConfirmDialog } from './src/storage-confirm-dialog';
