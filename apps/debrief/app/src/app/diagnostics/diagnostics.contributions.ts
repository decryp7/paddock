import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';
import { CAPTURE_KEYBINDING } from './diagnostics.ids';
import { ACTIVITY_LOG_COMMAND, REPORT_PROBLEM_COMMAND } from './snapshot.types';

// The registration file of the Diagnostics commands: stubs only; `diagnostics-commands.ts` and the services it injects
// load when the window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const TOOLS = $localize`:@@diagnostics.commands.category.tools:Tools`;
const HELP = $localize`:@@diagnostics.commands.category.help:Help`;

export const DIAGNOSTICS_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'diagnostics.capture', title: $localize`:@@diagnostics.commands.capture:Capture diagnostic screenshot`, category: TOOLS, keybinding: CAPTURE_KEYBINDING },
  { id: 'diagnostics.snapshots.show', title: $localize`:@@diagnostics.commands.showSnapshots:Show error snapshots`, category: TOOLS, view: 'error-snapshots' },
  { id: REPORT_PROBLEM_COMMAND, title: $localize`:@@diagnostics.commands.report:Report a problem…`, category: HELP },
  { id: 'diagnostics.saveDiagnostics', title: $localize`:@@diagnostics.commands.saveDiagnostics:Save diagnostics…`, category: HELP },
  { id: ACTIVITY_LOG_COMMAND, title: $localize`:@@diagnostics.commands.activityLog:Show activity log`, category: HELP },
];

export const DIAGNOSTICS_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./diagnostics-commands').then((m) => m.DiagnosticsCommands), DIAGNOSTICS_COMMAND_STUBS);
