import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';
import { ALERTS_VIEW, TAIL_STATUS_VIEW } from './live.ids';

// The registration file of the Live commands: stubs only; `live-commands.ts` and the live-tail services load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const FILE = $localize`:@@live.commands.categoryFile:File`;
const ANALYZE = $localize`:@@live.commands.categoryAnalyze:Analyze`;
const VIEW = $localize`:@@live.commands.categoryView:View`;

export const LIVE_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'live.toggle', title: $localize`:@@live.commands.toggle:Toggle live tail`, category: FILE },
  { id: 'live.open', title: $localize`:@@live.commands.open:Open for live tail…`, category: FILE },
  { id: 'live.resume', title: $localize`:@@live.commands.resume:Resume following`, category: ANALYZE },
  { id: 'live.alerts', title: $localize`:@@live.commands.showAlerts:Show live alerts`, category: VIEW, view: ALERTS_VIEW },
  { id: 'live.status', title: $localize`:@@live.commands.showStatus:Show tail status`, category: VIEW, view: TAIL_STATUS_VIEW },
  { id: 'live.rules', title: $localize`:@@live.commands.rules:Edit live alert rules…`, category: ANALYZE },
];

export const LIVE_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./live-commands').then((m) => m.LiveCommands), LIVE_COMMAND_STUBS);
