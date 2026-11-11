import { inject, Injectable } from '@angular/core';
import { CommandContext, DebriefCommand } from '../extensions/tokens';
import { showOpened } from './provide-video';
import { VideoStore } from './video.store';
import { VIDEO_VIEW_ID } from './video.ids';

/** File ▸ Open video… and Video: show. */
@Injectable({ providedIn: 'root' })
export class VideoCommands {
  private readonly store = inject(VideoStore);

  readonly commands: DebriefCommand[] = [
    { id: 'video.open', title: $localize`:@@video.cmd.open:Open video…`, category: 'File', run: (ctx) => this.open(ctx) },
    { id: 'video.show', title: $localize`:@@video.cmd.show:Video: show`, category: $localize`:@@video.category:Video`, view: VIDEO_VIEW_ID, run: (ctx) => ctx.showView(VIDEO_VIEW_ID) },
  ];

  async open(ctx: CommandContext): Promise<void> {
    const opened = await this.store.pickAndOpen();
    showOpened(ctx, opened, !!this.store.errors().open);
  }
}
