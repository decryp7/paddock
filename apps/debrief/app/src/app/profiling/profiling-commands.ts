import { inject, Injectable } from '@angular/core';
import { CommandContext, DebriefCommand } from '../extensions/tokens';
import { CapturesStore } from './captures.store';
import { PROFILING_VIEW_IDS as V } from './profiling.ids';
import { showOpened } from './provide-profiling';

const CATEGORY = $localize`:@@profiling.category:Profiling`;

/** Palette commands (and the File menu's "Open performance capture…"). */
@Injectable({ providedIn: 'root' })
export class ProfilingCommands {
  private readonly captures = inject(CapturesStore);

  private show(id: string, title: string, view: string): DebriefCommand {
    return { id, title, category: CATEGORY, view, run: (ctx) => ctx.showView(view) };
  }

  readonly commands: DebriefCommand[] = [
    { id: 'profiling.open', title: $localize`:@@profiling.cmd.open:Open performance capture…`, category: 'File', run: (ctx) => this.open(ctx) },
    this.show('profiling.captures', $localize`:@@profiling.cmd.captures:Profiling: show captures`, V.captures),
    this.show('profiling.hotspots', $localize`:@@profiling.cmd.hotspots:Profiling: hotspots`, V.hotspots),
    this.show('profiling.tree', $localize`:@@profiling.cmd.tree:Profiling: call tree`, V.tree),
    this.show('profiling.inverted', $localize`:@@profiling.cmd.inverted:Profiling: inverted tree (bottom-up)`, V.inverted),
    this.show('profiling.butterfly', $localize`:@@profiling.cmd.butterfly:Profiling: callers and callees`, V.butterfly),
    this.show('profiling.flame', $localize`:@@profiling.cmd.flame:Profiling: flame graph`, V.flame),
    this.show('profiling.compare', $localize`:@@profiling.cmd.compare:Profiling: compare with a baseline (differential flame graph)`, V.compare),
    this.show('profiling.threads', $localize`:@@profiling.cmd.threads:Profiling: thread timeline`, V.threads),
    this.show('profiling.findings', $localize`:@@profiling.cmd.findings:Profiling: findings`, V.findings),
    this.show('profiling.heap', $localize`:@@profiling.cmd.heap:Profiling: memory (heap snapshots)`, V.heap),
  ];

  /** Opens captures and shows the most useful view for what was opened. */
  async open(ctx: CommandContext): Promise<void> {
    showOpened(ctx, await this.captures.pickAndOpen());
    if (this.captures.error()) ctx.showView(V.captures);
  }
}
