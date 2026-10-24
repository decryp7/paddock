/**
 * `@decryp7/paddock-ui/shell/channels/dialog`: the channel dialogs' content, split from `shell/channels` so only their windows
 * load it: the secret dialog (`PdChannelSecretDialog`) and the remove confirmation (`PdChannelRemoveDialog`). The app's registry
 * loads them: `load: () => import('@decryp7/paddock-ui/shell/channels/dialog').then((m) => m.PdChannelSecretDialog)`.
 */
export { PdChannelSecretDialog } from './src/channel-secret-dialog';
export { PdChannelRemoveDialog } from './src/channel-remove-dialog';
