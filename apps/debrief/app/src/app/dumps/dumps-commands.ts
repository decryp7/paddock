import { inject, Injectable } from '@angular/core';
import { TOOL_IDS } from '../core/tools.types';
import { CommandContext, DebriefCommand } from '../extensions/tokens';
import { ToolCatalog } from '../tools/tool-catalog.store';
import { DUMP_VIEW_IDS } from './dumps.ids';
import { DumpFileKind, DumpsStore } from './dumps.store';
import { showOpened } from './provide-dumps';

const V = DUMP_VIEW_IDS;

/** File ▸ Open dump… / Open UMDH snapshots…, Tools ▸ Create dump of process…, and the views. */
@Injectable({ providedIn: 'root' })
export class DumpsCommands {
  private readonly store = inject(DumpsStore);
  private readonly catalog = inject(ToolCatalog);

  readonly commands: DebriefCommand[] = [
    { id: 'dumps.open', title: $localize`:@@dumps.cmd.open:Open dump…`, category: 'File', run: (ctx) => this.open(ctx, 'dump') },
    { id: 'dumps.openUmdh', title: $localize`:@@dumps.cmd.openUmdh:Open UMDH snapshots…`, category: 'File', run: (ctx) => this.open(ctx, 'umdh') },
    { id: 'dumps.capture', title: $localize`:@@dumps.cmd.capture:Create dump of process…`, category: $localize`:@@tools.category.tools:Tools`, when: () => this.catalog.listed(TOOL_IDS.dumpCapture), view: V.capture, run: (ctx) => ctx.showView(V.capture) },
    { id: 'dumps.show', title: $localize`:@@dumps.cmd.show:Dumps: show the dump`, category: $localize`:@@dumps.category:Dumps`, view: V.dump, run: (ctx) => ctx.showView(V.dump) },
    { id: 'dumps.showUmdh', title: $localize`:@@dumps.cmd.showUmdh:Dumps: UMDH heap growth`, category: $localize`:@@dumps.category:Dumps`, view: V.umdh, run: (ctx) => ctx.showView(V.umdh) },
  ];

  async open(ctx: CommandContext, kind: DumpFileKind): Promise<void> {
    const opened = await this.store.pickAndOpen(kind);
    showOpened(ctx, opened);
    if (!opened.length && this.store.error()) ctx.showView(kind === 'dump' ? V.dump : V.umdh);
  }
}
