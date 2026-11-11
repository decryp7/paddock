import { inject, Injectable } from '@angular/core';
import { DebriefCommand } from '../extensions/tokens';
import { DialogService } from '../ui/dialog.service';
import { LiveTail } from '../workspace/live-tail';
import { LiveFollow } from './live-follow.store';
import { ALERTS_VIEW, TAIL_STATUS_VIEW } from './live.ids';

@Injectable({ providedIn: 'root' })
export class LiveCommands {
  private readonly live = inject(LiveTail);
  private readonly follow = inject(LiveFollow);
  private readonly dialogs = inject(DialogService);

  readonly commands: DebriefCommand[] = [
    { id: 'live.toggle', title: $localize`:@@live.commands.toggle:Toggle live tail`, category: $localize`:@@live.commands.categoryFile:File`, when: () => this.live.available(), run: () => this.live.toggle() },
    {
      id: 'live.open',
      title: $localize`:@@live.commands.open:Open for live tail…`,
      category: $localize`:@@live.commands.categoryFile:File`,
      run: async () => {
        const { OpenLiveDialog } = await import('./open-live-dialog');
        this.dialogs.open(OpenLiveDialog, { title: $localize`:@@live.commands.openTitle:Open for live tail`, width: 480 });
      },
    },
    { id: 'live.resume', title: $localize`:@@live.commands.resume:Resume following`, category: $localize`:@@live.commands.categoryAnalyze:Analyze`, when: () => this.live.enabled(), run: () => this.follow.resume() },
    { id: 'live.alerts', title: $localize`:@@live.commands.showAlerts:Show live alerts`, category: $localize`:@@live.commands.categoryView:View`, view: ALERTS_VIEW, run: (ctx) => ctx.showView(ALERTS_VIEW) },
    { id: 'live.status', title: $localize`:@@live.commands.showStatus:Show tail status`, category: $localize`:@@live.commands.categoryView:View`, view: TAIL_STATUS_VIEW, run: (ctx) => ctx.showView(TAIL_STATUS_VIEW) },
    {
      id: 'live.rules',
      title: $localize`:@@live.commands.rules:Edit live alert rules…`,
      category: $localize`:@@live.commands.categoryAnalyze:Analyze`,
      run: async () => {
        const { AlertRulesEditor } = await import('./alert-rules-editor');
        this.dialogs.open(AlertRulesEditor, { title: $localize`:@@live.alertsView.dialogTitle:Live alert rules`, width: 760 });
      },
    },
  ];
}
