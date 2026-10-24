/**
 * `@decryp7/paddock-ui/shell/channels/dialog`: the secret dialog's content (`PdChannelSecretDialog`), split from
 * `shell/channels` so only the dialog's window loads it. The app's registry loads it:
 * `load: () => import('@decryp7/paddock-ui/shell/channels/dialog').then((m) => m.PdChannelSecretDialog)`.
 */
export { PdChannelSecretDialog } from './src/channel-secret-dialog';
