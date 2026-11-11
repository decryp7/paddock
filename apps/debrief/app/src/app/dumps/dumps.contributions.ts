import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';
import { DUMP_VIEW_IDS } from './dumps.ids';

// The registration file of the Dumps commands: stubs only; `dumps-commands.ts` and the stores it reads load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const V = DUMP_VIEW_IDS;

export const DUMPS_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'dumps.open', title: $localize`:@@dumps.cmd.open:Open dump…`, category: 'File' },
  { id: 'dumps.openUmdh', title: $localize`:@@dumps.cmd.openUmdh:Open UMDH snapshots…`, category: 'File' },
  { id: 'dumps.capture', title: $localize`:@@dumps.cmd.capture:Create dump of process…`, category: $localize`:@@tools.category.tools:Tools`, view: V.capture },
  { id: 'dumps.show', title: $localize`:@@dumps.cmd.show:Dumps: show the dump`, category: $localize`:@@dumps.category:Dumps`, view: V.dump },
  { id: 'dumps.showUmdh', title: $localize`:@@dumps.cmd.showUmdh:Dumps: UMDH heap growth`, category: $localize`:@@dumps.category:Dumps`, view: V.umdh },
];

export const DUMPS_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./dumps-commands').then((m) => m.DumpsCommands), DUMPS_COMMAND_STUBS);
