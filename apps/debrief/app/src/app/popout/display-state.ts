import { Injector } from '@angular/core';
import { DateFormat, TimeZoneMode } from '../core/format';
import { WorkspaceDataStore } from '../core/workspace/workspace-data.store';
import { isSchemeMode, isUiFont, UiPrefsStore } from '../core/workspace/ui-prefs.store';
import { isThemeId } from '../theme/themes';
import { GridDensity, SettingsStore, SOURCE_COLOR_MODES, SourceColorMode } from '../settings/settings.store';
import type { SharedStateDef } from './shared-state';
import { SyncedValue } from './ui-sync';

// The shared state groups every window may hold, dialog windows included: display settings and the data version. The
// workspace group (`shared-state.ts`) binds stores a dialog window must not import.

const ZONES: readonly TimeZoneMode[] = ['utc', 'local'];
const DATE_FORMATS: readonly DateFormat[] = ['readable', 'iso'];
const DENSITIES: readonly GridDensity[] = ['compact', 'comfortable'];

/** Theme and display settings: they need only the preference stores. */
function displayState(injector: Injector): SyncedValue[] {
  const prefs = injector.get(UiPrefsStore);
  const settings = injector.get(SettingsStore);
  return [
    { key: 'theme', read: () => prefs.theme(), apply: (v) => (v === null || isThemeId(v)) && prefs.setTheme(v) },
    { key: 'scheme', read: () => prefs.scheme(), apply: (v) => isSchemeMode(v) && prefs.setScheme(v) },
    { key: 'uiFont', read: () => prefs.uiFont(), apply: (v) => isUiFont(v) && prefs.setUiFont(v) },
    { key: 'timezone', read: () => prefs.timezone(), apply: (v) => ZONES.includes(v as TimeZoneMode) && prefs.setTimezone(v as TimeZoneMode) },
    { key: 'lifecycle', read: () => prefs.alwaysShowLifecycle(), apply: (v) => typeof v === 'boolean' && prefs.setAlwaysShowLifecycle(v) },
    { key: 'dateFormat', read: () => settings.dateFormat(), apply: (v) => DATE_FORMATS.includes(v as DateFormat) && settings.setDateFormat(v as DateFormat) },
    { key: 'density', read: () => settings.density(), apply: (v) => DENSITIES.includes(v as GridDensity) && settings.setDensity(v as GridDensity) },
    {
      key: 'sourceColors',
      read: () => settings.sourceColors(),
      apply: (v) => SOURCE_COLOR_MODES.includes(v as SourceColorMode) && settings.setSourceColors(v as SourceColorMode),
    },
  ];
}

/** The host's data versions: a guest reloads its summary when they move. */
function dataState(injector: Injector): SyncedValue[] {
  const data = injector.get(WorkspaceDataStore);
  return [
    {
      key: 'data',
      direction: 'fromHost',
      read: () => data.dataVersion() + data.profileVersion() * 1e6,
      apply: () => void data.refreshSummary().catch(() => undefined),
    },
  ];
}

/** Theme and display settings (every window). */
export const DISPLAY_SHARED_STATE: SharedStateDef = Object.freeze({ id: 'db.shared.display', scope: 'display', values: displayState });

/** The host's data version (windows that read the workspace summary, and pop-outs). */
export const DATA_SHARED_STATE: SharedStateDef = Object.freeze({ id: 'db.shared.data', scope: 'summary', values: dataState });
