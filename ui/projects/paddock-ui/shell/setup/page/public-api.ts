/**
 * `@decryp7/paddock-ui/shell/setup/page`: the Set-up window (`PdSetupDialog`), the Settings > Set-up page (`PdSetupPage`) and
 * the checklist they share (`PdSetupChecklist`), split from `shell/setup` so a main window's first paint carries none of it
 * (plan S16). The app's registry loads them: `load: () => import('@decryp7/paddock-ui/shell/setup/page').then((m) => m.PdSetupDialog)`.
 */
export { PdSetupChecklist } from './src/setup-checklist';
export { PdSetupDialog } from './src/setup-dialog';
export { PdSetupPage } from './src/setup-page';
