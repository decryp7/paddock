import { provideShellPacks } from '@decryp7/paddock-ui/shell/packs';
import { demoPacksBackend } from './demo-packs';

/** What the Knowledge packs page boots with (loaded with the page, never in the main window's first paint). */
export const PACKS_PROVIDERS = [provideShellPacks({ backend: demoPacksBackend })];
