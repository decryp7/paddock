import { settingsWindowProviders } from '@decryp7/paddock-ui/shell/settings/window';
import { demoSettingsBackend } from './demo-settings';
import { DEMO_SETTINGS_PAGES } from './demo.contributions';
import { LANGUAGE_PROVIDERS } from './language.providers';

/** The Settings window's providers (its dialog's `providers`): loaded with it, never in the main window's first paint. */
export const DEMO_SETTINGS_WINDOW = [...settingsWindowProviders({ backend: demoSettingsBackend, pages: DEMO_SETTINGS_PAGES }), ...LANGUAGE_PROVIDERS];
