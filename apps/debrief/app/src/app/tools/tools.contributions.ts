import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';

// The registration file of the Tools, App lifecycle, Project and Saved-query commands: stubs only; the command classes
// and the services they inject load when the window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps
// the stubs equal to the code.

const TOOLS = $localize`:@@tools.category.tools:Tools`;

export const TOOLS_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'tools.perfmon.show', title: $localize`:@@tools.cmd.perfmon:Open Performance Monitor`, category: TOOLS, view: 'perfmon' },
  { id: 'tools.monitor.show', title: $localize`:@@tools.cmd.processMonitor:Open Process Monitor`, category: TOOLS, view: 'process-monitor' },
  { id: 'tools.eventlog.show', title: $localize`:@@tools.cmd.eventLog:Open Event Log Follower`, category: TOOLS, view: 'event-log-follower' },
  { id: 'tools.health.show', title: $localize`:@@tools.cmd.pluginHealth:Open plugin health`, category: TOOLS },
  { id: 'tools.collect', title: $localize`:@@tools.cmd.collect:Collect diagnostics bundle…`, category: TOOLS },
];

const APP = $localize`:Command category@@lifecycle.commands.category:App`;

export const LIFECYCLE_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'app.restartElevated', title: $localize`:@@lifecycle.commands.restartElevated:Restart as administrator`, category: APP },
  { id: 'app.restartKeepingSession', title: $localize`:@@lifecycle.commands.restartKeepingSession:Restart and reopen this session`, category: APP },
  { id: 'app.restartSafeMode', title: $localize`:@@lifecycle.commands.restartSafeMode:Restart in safe mode`, category: APP },
  { id: 'app.restartNormally', title: $localize`:@@lifecycle.commands.restartNormally:Restart normally`, category: APP },
];

const FILE = $localize`:Command palette category|@@projects.cmd.fileCategory:File`;

export const PROJECT_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'project.new', title: $localize`:@@projects.cmd.new:New project`, category: FILE },
  { id: 'project.open', title: $localize`:@@projects.cmd.open:Open project…`, category: FILE },
  { id: 'project.recent', title: $localize`:@@projects.cmd.recent:Open recent project…`, category: FILE },
  { id: 'project.openLast', title: $localize`:@@projects.cmd.openLast:Open last project`, category: FILE },
  { id: 'project.save', title: $localize`:@@projects.cmd.save:Save project`, category: FILE, keybinding: 'Mod+S' },
  { id: 'project.saveAs', title: $localize`:@@projects.cmd.saveAs:Save project as…`, category: FILE, keybinding: 'Mod+Shift+S' },
];

export const SAVED_QUERY_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'savedQueries.show', title: $localize`:@@projects.cmd.savedQueries.show:Show saved queries`, category: $localize`:Command palette category|@@projects.cmd.analyzeCategory:Analyze` },
];

export const TOOLS_COMMANDS_CONTRIBUTIONS = [
  ...lazyCommands(() => import('./tools-commands').then((m) => m.ToolsCommands), TOOLS_COMMAND_STUBS),
  ...lazyCommands(() => import('../lifecycle/lifecycle-commands').then((m) => m.LifecycleCommands), LIFECYCLE_COMMAND_STUBS),
  ...lazyCommands(() => import('../projects/project-commands').then((m) => m.ProjectCommands), PROJECT_COMMAND_STUBS),
  ...lazyCommands(() => import('../projects/saved-query-commands').then((m) => m.SavedQueryCommands), SAVED_QUERY_COMMAND_STUBS),
];
