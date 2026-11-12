import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';

// The registration file of the Markers commands: stubs only; `markers-commands.ts` and the stores it reads load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:Command palette category|@@markers.cmd.category:Markers`;

export const MARKERS_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'markers.manage', title: $localize`:@@markers.cmd.manage.title:Manage markers`, category: CATEGORY, view: 'markers' },
  { id: 'markers.highlight', title: $localize`:@@markers.cmd.highlight.title:Highlight lines…`, category: CATEGORY },
  { id: 'markers.infer', title: $localize`:@@markers.cmd.infer.title:Infer pattern from selected lines…`, category: CATEGORY },
];

export const MARKERS_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./markers-commands').then((m) => m.MarkersCommands), MARKERS_COMMAND_STUBS);
