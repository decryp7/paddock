/**
 * `@decryp7/paddock-ui/shell/service/page`: the Settings > Background service page (`PdServicePage`), split from
 * `shell/service` so a main window's first paint carries none of it (plan S16), as `shell/settings/window` is. The app's
 * registry loads it: `load: () => import('@decryp7/paddock-ui/shell/service/page').then((m) => m.PdServicePage)`.
 */
export { PdServicePage } from './src/service-page';
