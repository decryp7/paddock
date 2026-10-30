/**
 * `@decryp7/paddock-ui/shell/settings/window`: the Settings window (plan S10), split from `shell/settings` so a main
 * window's first paint carries none of it (plan S16): `PdSettingsWindow` (the app's `pd.settings` dialog), the generic
 * section form `PdSettingsSection` and its page `PdSettingsSectionPage`, the field control `PdSettingField` (Signal
 * Forms), the window's unsaved edits (`SettingsDrafts`) and `settingsWindowProviders` for the dialog's `providers`.
 */
export { SettingsDrafts, type SectionForm, type SectionValues } from './src/settings-drafts';
export { PdSettingField } from './src/setting-field';
export { PdSettingsSection, PdSettingsSectionPage, SETTINGS_PAGE } from './src/settings-section';
export { PdSettingsWindow, SETTINGS_WINDOW_PAGES, SETTINGS_RESTART } from './src/settings-window';
export { SettingsStatus, settingsPageStatus } from './src/settings-status';
export { settingsWindowProviders, type SettingsWindowOptions } from './src/provide-settings';
