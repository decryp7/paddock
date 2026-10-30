import { settingsWindowProviders } from '@decryp7/paddock-ui/shell/settings/window';
import { PITCREW_SETTINGS_PAGES } from './pitcrew.contributions';
import { settingsBackend } from './settings';

/** The Settings window's providers (its dialog's `providers`): the shell's window (Pitcrew's backend is in the window's base set). */
export const PITCREW_SETTINGS_WINDOW = settingsWindowProviders({ backend: settingsBackend, pages: PITCREW_SETTINGS_PAGES });
