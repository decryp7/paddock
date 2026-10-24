import { provideShellSetup } from '@decryp7/paddock-ui/shell/setup';
import { demoSetupBackend } from './demo-setup';

/** What the Set-up window and the Settings > Set-up page boot with (loaded with them, never in the main window's first paint). */
export const SETUP_PROVIDERS = [provideShellSetup({ backend: demoSetupBackend })];
