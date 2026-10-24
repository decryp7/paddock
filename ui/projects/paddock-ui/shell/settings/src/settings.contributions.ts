import { defineCommand, defineKeybinding, defineMenuItem, defineSettingsPage, msg, type ContributionBatch, type SettingsPageDef } from '@decryp7/paddock-ui/shell';

// The settings part's contributions: static data and loaders only (lazy by construction, plan §9). The Settings window
// is the app's dialog `pd.settings` (its contract names the settings commands); this opens it.

/** The id of the Settings window (the app declares it with `defineDialog`, see the entry point's docs). */
export const SETTINGS_DIALOG_ID = 'pd.settings';

/** The commands the Settings window calls: its dialog contract lists exactly these. */
export const SETTINGS_DIALOG_COMMANDS = ['settings_overview', 'settings_reset', 'settings_set'] as const;

/** The shell's own settings pages (each shows the Rust section of the same id in the Settings window's generic form). */
export const SHELL_SETTINGS_PAGES: readonly SettingsPageDef[] = [
  defineSettingsPage({
    id: 'pd.appearance',
    group: 'application',
    title: msg('pd.settings.appearance', 'Appearance'),
    order: 50,
  }),
];

export const SETTINGS_CONTRIBUTIONS: ContributionBatch = {
  commands: [
    defineCommand({
      id: 'pd.settings.open',
      title: msg('pd.settings.open', 'Settings…'),
      category: msg('pd.category.app', 'Application'),
      run: (ctx) => void ctx.openDialog(SETTINGS_DIALOG_ID),
    }),
  ],
  keybindings: [defineKeybinding({ command: 'pd.settings.open', key: 'Mod+,' })],
  menus: [defineMenuItem({ menu: 'app', command: 'pd.settings.open', group: 'settings', order: 100 })],
  settingsPages: SHELL_SETTINGS_PAGES,
};
