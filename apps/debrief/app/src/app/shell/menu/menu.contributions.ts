import type { Provider } from '@angular/core';
import { contributionProviders, type MenuItemDef } from '@decryp7/paddock-ui/shell/contributions';
import type { MenuState } from './menu-model';

/*
 * Where Debrief's commands sit in the menus, as the shell's `menus` contributions (workbench.md §4.3 M1): menu, group and
 * order per command. Debrief's own model (`menu-model.ts`) still lays the menus out until M2c moves them to the
 * shell's: its layout names the groups in each menu and where separators, submenus and the parts that are not commands
 * (views, recent files, windows) go. A placement in a group the layout does not name (a plugin's) is shown at the end of
 * its menu. Top-level menus are the shell's (`file`, `edit`, `view`, `window`, `help`, `app`) or Debrief's own
 * (`db.analyze`, `db.tools`); submenus are `db.<menu>.<name>`.
 */

/** A placement with what Debrief's menus add: the label in the menu (when it differs from the command's title), a check mark, and macOS app-menu rules. */
export interface DebriefMenuItemDef extends MenuItemDef {
  /** The menu's own label for the command, fixed or from the menus' state (Undo names what it undoes). */
  readonly label?: string | ((state: MenuState) => string);
  /** A check mark (menuitemcheckbox) from the menus' state. */
  readonly checked?: (state: MenuState) => boolean;
  /** `notNative`: left out of the macOS native menu bar, where the app menu has it (About, Settings, Quit). */
  readonly native?: 'notNative';
}

/** Declares a placement (frozen; the shell's defaults: group `main`). */
function item(command: string, menu: string, group: string, order: number, extra: Partial<Pick<DebriefMenuItemDef, 'label' | 'checked' | 'native'>> = {}): DebriefMenuItemDef {
  return Object.freeze({ menu, command, group, order, ...extra });
}

const ABOUT = $localize`:@@shell.menu.help.about:About Debrief`;
const CHECK_UPDATES = $localize`:@@shell.menu.help.checkForUpdates:Check for updates…`;
const SETTINGS = $localize`:@@shell.menu.settings:Settings…`;

/** Every placement of Debrief's commands, menu by menu. */
export const DEBRIEF_MENU_ITEMS: readonly DebriefMenuItemDef[] = Object.freeze([
  // Debrief ▸ (the macOS app menu)
  item('help.about', 'app', 'about', 100, { label: ABOUT }),
  item('help.checkForUpdates', 'app', 'about', 200, { label: CHECK_UPDATES }),
  item('settings.open', 'app', 'settings', 300, { label: SETTINGS }),
  item('app.quit', 'app', 'quit', 400, { label: $localize`:@@shell.menu.app.quit:Quit Debrief` }),

  // File
  item('project.new', 'file', 'new', 100),
  item('window.new', 'file', 'new', 200),
  item('file.openFiles', 'file', 'open', 300),
  item('file.openFolder', 'file', 'open', 400),
  item('live.open', 'file', 'open', 500),
  item('profiling.open', 'file', 'open', 600),
  item('dumps.open', 'file', 'open', 700),
  item('dumps.openUmdh', 'file', 'open', 800),
  item('video.open', 'file', 'open', 900),
  item('project.open', 'file', 'open', 1000),
  item('project.save', 'file', 'save', 1100),
  item('project.saveAs', 'file', 'save', 1200),
  item('share.redactedCopy', 'file', 'save', 1300),
  item('file.reimport', 'file', 'reimport', 1400),
  item('folder.setupAnalysis', 'file', 'reimport', 1500),
  item('file.export', 'file', 'export', 1600),
  item('report.export', 'file', 'export', 1700),
  item('file.close', 'file', 'close', 1800),
  item('window.close', 'file', 'close', 1900),
  // On macOS Quit is in the app menu.
  item('app.quit', 'file', 'quit', 2000, { native: 'notNative' }),

  // Edit
  item('edit.undo', 'edit', 'undo', 100, {
    label: (s) => (s.undoLabel ? $localize`:@@shell.menu.edit.undo:Undo ${s.undoLabel}:what:` : $localize`:@@shell.menu.edit.undoNothing:Undo (nothing to undo)`),
  }),
  item('edit.redo', 'edit', 'undo', 200, {
    label: (s) => (s.redoLabel ? $localize`:@@shell.menu.edit.redo:Redo ${s.redoLabel}:what:` : $localize`:@@shell.menu.edit.redoNothing:Redo (nothing to redo)`),
  }),
  item('edit.copy', 'edit', 'copy', 300),
  item('edit.copyJson', 'edit', 'copy', 400),
  item('edit.pasteLog', 'edit', 'paste', 500),
  item('edit.find', 'edit', 'find', 600),

  // View
  item('view.toggleSidebar', 'view', 'sidebar', 100, { label: $localize`:@@shell.menu.view.explorer:Explorer`, checked: (s) => s.sidebarOpen }),
  item('timeline.new', 'view', 'new', 200),
  item('live.toggle', 'db.view.live', 'live', 100),
  item('ai.setup', 'db.view.ai', 'setup', 100, { label: $localize`:@@shell.menu.view.ai.setup:Connect to Pitwall…` }),
  item('view.theme.system', 'db.view.theme', 'theme', 100, { label: $localize`:Theme name@@shell.menu.view.theme.system:Follow system`, checked: (s) => s.theme === null }),
  item('view.theme.calm', 'db.view.theme', 'theme', 200, { label: $localize`:Theme name@@shell.menu.view.theme.calm:Calm`, checked: (s) => s.theme === 'calm' }),
  item('view.theme.console', 'db.view.theme', 'theme', 300, { label: $localize`:Theme name@@shell.menu.view.theme.console:Console`, checked: (s) => s.theme === 'console' }),
  item('view.theme.precision', 'db.view.theme', 'theme', 400, { label: $localize`:Theme name@@shell.menu.view.theme.precision:Precision`, checked: (s) => s.theme === 'precision' }),
  item('view.theme.contrast', 'db.view.theme', 'theme', 500, { label: $localize`:Theme name@@shell.menu.view.theme.contrast:High contrast`, checked: (s) => s.theme === 'contrast' }),
  item('view.scheme.system', 'db.view.theme', 'scheme', 600, { label: $localize`:Colour scheme option@@shell.menu.view.scheme.system:System`, checked: (s) => s.scheme === 'system' }),
  item('view.scheme.dark', 'db.view.theme', 'scheme', 700, { label: $localize`:Colour scheme option@@shell.menu.view.scheme.dark:Dark`, checked: (s) => s.scheme === 'dark' }),
  item('view.scheme.light', 'db.view.theme', 'scheme', 800, { label: $localize`:Colour scheme option@@shell.menu.view.scheme.light:Light`, checked: (s) => s.scheme === 'light' }),
  item('view.sourceColors.source', 'db.view.source-colors', 'mode', 100, {
    label: $localize`:@@shell.menu.view.sourceColors.source:By file`,
    checked: (s) => (s.sourceColors ?? 'source') === 'source',
  }),
  item('view.sourceColors.instance', 'db.view.source-colors', 'mode', 200, {
    label: $localize`:@@shell.menu.view.sourceColors.instance:By instance`,
    checked: (s) => s.sourceColors === 'instance',
  }),
  item('view.sourceColors.off', 'db.view.source-colors', 'mode', 300, { label: $localize`:@@shell.menu.view.sourceColors.off:Off`, checked: (s) => s.sourceColors === 'off' }),

  // Analyze
  item('analyze.search', 'db.analyze', 'filter', 100),
  item('query.focus', 'db.analyze', 'filter', 200, { label: $localize`:@@shell.menu.analyze.focusQueryBar:Focus query bar` }),
  item('markers.highlight', 'db.analyze', 'filter', 300),
  item('analyze.markedOnly', 'db.analyze', 'filter', 400, { label: $localize`:@@shell.menu.analyze.markedOnly:Marked events only`, checked: (s) => s.markedOnly }),
  item('analyze.clearFilters', 'db.analyze', 'filter', 500),
  item('analyze.fit', 'db.analyze', 'range', 600),
  item('analyze.back', 'db.analyze', 'range', 700),
  item('analyze.forward', 'db.analyze', 'range', 800),
  item('nav.back', 'db.analyze', 'range', 900),
  item('nav.forward', 'db.analyze', 'range', 1000),
  item('analyze.utc', 'db.analyze.time-zone', 'zone', 100, { label: $localize`:@@shell.menu.analyze.utc:UTC`, checked: (s) => s.timezone === 'utc' }),
  item('analyze.local', 'db.analyze.time-zone', 'zone', 200, { label: $localize`:Time zone option@@shell.menu.analyze.local:Local`, checked: (s) => s.timezone === 'local' }),
  item('moment.analyse', 'db.analyze', 'moment', 1100),
  item('moment.selected', 'db.analyze', 'moment', 1200, { label: $localize`:@@shell.menu.analyze.analyseSelectedRange:Analyse the selected range` }),
  item('fieldstats.show', 'db.analyze', 'moment-after', 1300),
  item('compare.previous', 'db.analyze.compare', 'compare', 100, { label: $localize`:@@shell.menu.analyze.compare.previous:Current range vs previous` }),
  item('compare.beforeAfter', 'db.analyze.compare', 'compare', 200, { label: $localize`:@@shell.menu.analyze.compare.beforeAfter:Before / after selected time` }),
  item('ai.query', 'db.analyze.ai', 'ai', 100, { label: $localize`:@@shell.menu.analyze.ai.query:Ask AI to write a query…` }),
  item('ai.summarize', 'db.analyze.ai', 'ai', 200, { label: $localize`:@@shell.menu.analyze.ai.summarize:Summarize this range` }),
  item('ai.rootCauses', 'db.analyze.ai', 'ai', 300, { label: $localize`:@@shell.menu.analyze.ai.rootCauses:Find likely root causes` }),
  item('live.resume', 'db.analyze', 'live', 1400),
  item('live.rules', 'db.analyze', 'live', 1500),
  item('report.new', 'db.analyze', 'report', 1600),
  item('incident.build', 'db.analyze.report', 'summary', 100, { label: $localize`:@@shell.menu.report.incident:Incident summary…` }),
  item('report.addSelected', 'db.analyze.report', 'parts', 200, { label: $localize`:@@shell.menu.report.selectedEvent:Selected event` }),
  item('report.addNotes', 'db.analyze.report', 'parts', 300, { label: $localize`:@@shell.menu.report.notes:Notes` }),
  item('report.addTimeline', 'db.analyze.report', 'parts', 400, { label: $localize`:@@shell.menu.report.timeline:Timeline` }),
  item('report.addAnnotations', 'db.analyze.report', 'parts', 500, { label: $localize`:@@shell.menu.report.annotations:Annotations` }),
  item('report.addQueryTable', 'db.analyze.report', 'parts', 600, { label: $localize`:@@shell.menu.report.queryResult:Query result` }),
  item('report.addFindings', 'db.analyze.report', 'parts', 700, { label: $localize`:@@shell.menu.report.findings:Health-check findings` }),
  item('report.addAnomalies', 'db.analyze.report', 'parts', 800, { label: $localize`:@@shell.menu.report.anomalies:Anomalies as findings` }),

  // Tools
  item('tools.collect', 'db.tools', 'capture', 100),
  item('dumps.capture', 'db.tools', 'capture', 200),
  item('diagnostics.capture', 'db.tools', 'capture', 300),
  item('parsers.manage', 'db.tools', 'parsers', 400),
  item('app.restartElevated', 'db.tools', 'elevate', 500),
  item('settings.open', 'db.tools', 'settings', 600, { label: SETTINGS, native: 'notNative' }),

  // Window
  ...Array.from({ length: 9 }, (_, i) => item(`window.perspective.${i + 1}`, 'db.window.perspectives', 'switch', (i + 1) * 100, { checked: (s) => s.activePerspective === i + 1 })),
  item('window.layout.save', 'db.window.perspectives', 'save', 1000),
  item('window.layout.manage', 'db.window.perspectives', 'manage', 1100),
  item('window.layout.reset', 'db.window.perspectives', 'manage', 1200),
  item('window.autoHideAll', 'window', 'tool-windows', 100),
  item('window.pinAll', 'window', 'tool-windows', 200),
  item('window.closeAllDocuments', 'window', 'tool-windows', 300),
  item('window.float', 'window', 'panel', 400),
  item('window.popOut', 'window', 'panel', 500),
  item('window.fullScreen', 'window', 'panel', 600),

  // Help
  item('help.welcome', 'help', 'start', 100, { label: $localize`:@@shell.menu.help.welcome:Welcome` }),
  item('help.demo', 'help', 'start', 200),
  item('help.tour', 'help', 'start', 300),
  item('help.shortcuts', 'help', 'start', 400),
  item('help.whatsNew', 'help', 'docs', 600, { label: $localize`:@@shell.menu.help.whatsNew:What's new` }),
  item('help.reportProblem', 'help', 'trouble', 700),
  item('help.activityLog', 'help', 'trouble', 800),
  item('help.clearCache', 'help', 'trouble', 1000, { label: $localize`:@@shell.menu.help.clearCache:Clear cache…` }),
  item('app.restartSafeMode', 'help', 'trouble', 1100),
  item('app.restartNormally', 'help', 'trouble', 1200),
  item('help.checkForUpdates', 'help', 'about', 1300, { label: CHECK_UPDATES, native: 'notNative' }),
  item('help.notices', 'help', 'about', 1400, { label: $localize`:@@shell.menu.help.notices:Third-party notices` }),
  item('help.about', 'help', 'about', 1500, { label: ABOUT, native: 'notNative' }),
]);

/** Registers menu placements (a plugin's, or a test's) in the shell's `menus` kind. */
export function provideMenuItems(items: readonly DebriefMenuItemDef[]): Provider[] {
  return contributionProviders('menus', items);
}

/** One line in `app.config.ts`: where Debrief's commands sit in the menus. */
export function provideDebriefMenus(): Provider[] {
  return provideMenuItems(DEBRIEF_MENU_ITEMS);
}
