/**
 * `@decryp7/paddock-ui/shell/channels/page`: the Settings > Notification channels page (`PdChannelsPage`), split from
 * `shell/channels` so a main window's first paint carries none of it (plan S16). The app's registry loads it:
 * `load: () => import('@decryp7/paddock-ui/shell/channels/page').then((m) => m.PdChannelsPage)`.
 */
export { PdChannelsPage } from './src/channels-page';
