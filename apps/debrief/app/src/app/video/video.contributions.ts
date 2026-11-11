import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';
import { VIDEO_VIEW_ID } from './video.ids';

// The registration file of the Video commands: stubs only; `video-commands.ts` and the store it reads load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

export const VIDEO_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'video.open', title: $localize`:@@video.cmd.open:Open video…`, category: 'File' },
  { id: 'video.show', title: $localize`:@@video.cmd.show:Video: show`, category: $localize`:@@video.category:Video`, view: VIDEO_VIEW_ID },
];

export const VIDEO_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./video-commands').then((m) => m.VideoCommands), VIDEO_COMMAND_STUBS);
