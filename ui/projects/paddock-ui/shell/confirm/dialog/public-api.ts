/**
 * `@decryp7/paddock-ui/shell/confirm/dialog`: `pd.confirm`'s content (`PdConfirmDialog`), split from `shell/confirm` so only
 * its window loads it: `load: () => import('@decryp7/paddock-ui/shell/confirm/dialog').then((m) => m.PdConfirmDialog)`.
 */
export { PdConfirmDialog } from './src/confirm-dialog';
