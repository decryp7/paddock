/**
 * `@decryp7/paddock-ui/shell/packs/page`: the Settings > Knowledge packs page (`PdPacksPage`), split from `shell/packs` so a main
 * window's first paint carries none of it (plan S16). The app's registry loads it:
 * `load: () => import('@decryp7/paddock-ui/shell/packs/page').then((m) => m.PdPacksPage)`.
 */
export { PdPacksPage } from './src/packs-page';
