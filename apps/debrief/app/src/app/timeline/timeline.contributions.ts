import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';

// The registration file of the "New timeline" command: stubs only. `timeline-views.ts` also holds the saved timelines
// (injected by the ui-state contribution), so the class is in the window anyway; the stub keeps the registration free
// of it. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

export const TIMELINES_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'timeline.new', title: $localize`:@@timeline.command.new:New timeline`, category: $localize`:@@timeline.command.category:View` },
];

export const TIMELINES_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./timeline-views').then((m) => m.TimelineViews), TIMELINES_COMMAND_STUBS);
