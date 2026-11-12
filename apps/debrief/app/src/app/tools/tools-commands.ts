import { inject, Injectable } from '@angular/core';
import { TOOL_IDS } from '../core/tools.types';
import { DebriefCommand } from '../extensions/tokens';
import { CollectorDialogService } from './collector/collector-dialog.service';
import { PluginsTabs } from '../plugins-panel/plugins-tabs';
import { ToolCatalog } from './tool-catalog.store';

/** Commands that open the tools. */
@Injectable({ providedIn: 'root' })
export class ToolsCommands {
  private readonly collector = inject(CollectorDialogService);
  private readonly catalog = inject(ToolCatalog);
  private readonly plugins = inject(PluginsTabs);

  readonly commands: DebriefCommand[] = [
    { id: 'tools.perfmon.show', title: $localize`:@@tools.cmd.perfmon:Open Performance Monitor`, category: $localize`:@@tools.category.tools:Tools`, when: () => this.catalog.listed(TOOL_IDS.perfmonTemplates), view: 'perfmon', run: (ctx) => ctx.showView('perfmon') },
    { id: 'tools.monitor.show', title: $localize`:@@tools.cmd.processMonitor:Open Process Monitor`, category: $localize`:@@tools.category.tools:Tools`, when: () => this.catalog.listed(TOOL_IDS.processMonitor), view: 'process-monitor', run: (ctx) => ctx.showView('process-monitor') },
    { id: 'tools.eventlog.show', title: $localize`:@@tools.cmd.eventLog:Open Event Log Follower`, category: $localize`:@@tools.category.tools:Tools`, when: () => this.catalog.listed(TOOL_IDS.eventLog), view: 'event-log-follower', run: (ctx) => ctx.showView('event-log-follower') },
    {
      id: 'tools.health.show',
      title: $localize`:@@tools.cmd.pluginHealth:Open plugin health`,
      category: $localize`:@@tools.category.tools:Tools`,
      run: (ctx) => {
        this.plugins.show('health');
        ctx.showView('plugins');
      },
    },
    { id: 'tools.collect', title: $localize`:@@tools.cmd.collect:Collect diagnostics bundle…`, category: $localize`:@@tools.category.tools:Tools`, when: () => this.catalog.listed(TOOL_IDS.diagnostics), run: () => this.collector.open() },
  ];
}
