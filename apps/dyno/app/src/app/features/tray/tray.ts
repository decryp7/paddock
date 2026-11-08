import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, OnInit, afterNextRender, computed, inject, viewChild } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';
import { SuTrayPopover, type SuTrayMenuEntry, type SuTrayPlatform } from '@decryp7/paddock-ui';
import { PdWidgetArea } from '@decryp7/paddock-ui/widgets';
import { BACKEND } from '../../core/backend';
import type { WidgetActionId } from '../../core/generated/widgets';
import { GRADE_LABEL, GRADE_TONE } from '../../shared/labels';
import { observeHeight } from '../../shared/observe-height';
import { PlatformService } from '../../core/platform';
import { PopoverValues } from '../../state/popover-values';
import { WidgetLayouts } from '../../state/widget-layouts';
import { DynoPopoverWidgets } from './widgets/popover-widgets';
import { popoverMeta } from './widgets/popover-meta';
import { shortcutLabel } from './popover-shortcuts';
import { HealthStore } from '../../state/health.store';
import { LiveStore } from '../../state/live.store';
import { QUIT_STOP, QuitStore } from '../../state/quit.store';

/**
 * The tray dropdown (360 wide, frameless): the health pill and agent state, the person's widgets on `pd-widget-area`
 * (family ADR 0035: the layout and every value come from the engine), and the shared footer of `pd-tray-popover` with Open Dyno and the ⋯ menu. The
 * tray / menu-bar icon itself is drawn by the shell.
 */
@Component({
  selector: 'dy-tray',
  imports: [SuTrayPopover, MatTooltip, PdWidgetArea, DynoPopoverWidgets],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pd-tray-popover label="Dyno" [platform]="platform" primaryLabel="Open Dyno" [menu]="menu()" (primary)="openMain()" (menuAction)="choose($event)" (dismissed)="close()">
      <div slot="header" class="head">
        <img src="brand/dyno-16.svg" alt="" width="16" height="16" />
        <strong>Dyno</strong>
        <span class="spacer"></span>
        @if (stopped()) {
          <span class="agent" data-state="failed" role="status"><span class="pulse"></span>Agent stopped</span>
        } @else {
        @if (health.report(); as r) {
          <span class="grade-pill" [attr.data-tone]="tone()" [matTooltip]="'Health score ' + score(r.score) + ' of 100'"><strong class="num">{{ score(r.score) }}</strong> {{ grade() }}</span>
        }
        <span class="agent" [attr.data-state]="live.agent().state" role="status"><span class="pulse"></span>{{ agentText() }}</span>
        }
      </div>
      <main class="tray" #body>
        @if (layout.popover(); as view) {
          <pd-widget-area [placements]="view.layout.placements" [catalogue]="view.catalogue" [values]="values.values()" [templates]="templates().templates()" [meta]="meta" (action)="run($event)" (customize)="run('customizeWidgets')" />
        }
        <dy-popover-widgets (action)="run($event)" />
      </main>
    </pd-tray-popover>
  `,
  styles: `
    :host { display: block; }
    .tray { display: grid; gap: var(--sp-2); }
    .head { display: flex; align-items: center; gap: var(--sp-2); flex: 1; }
    .head strong:first-of-type { font-weight: var(--fw-semibold); }
    .spacer { flex: 1; }
    .agent { display: flex; align-items: center; gap: 6px; color: var(--text-2); font-size: var(--fs-xs); }
    .pulse { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); }
    .agent[data-state='failed'] .pulse { background: var(--bad); }
    .agent[data-state='starting'] .pulse, .agent[data-state='restarting'] .pulse { background: var(--warn); }
    .grade-pill { display: inline-flex; align-items: center; gap: 6px; height: 22px; padding: 0 var(--sp-2); border-radius: var(--r-pill); font-size: var(--fs-xs); background: var(--ok-soft); color: var(--ok); }
    .grade-pill[data-tone='warn'] { background: var(--warn-soft); color: var(--warn); }
    .grade-pill[data-tone='bad'] { background: var(--bad-soft); color: var(--bad); }
  `,
})
export class Tray implements OnInit {
  protected readonly live = inject(LiveStore);
  protected readonly health = inject(HealthStore);
  protected readonly layout = inject(WidgetLayouts);
  protected readonly values = inject(PopoverValues);
  private readonly backend = inject(BACKEND);
  private readonly quitText = inject(QuitStore);
  private readonly host = inject(ElementRef<HTMLElement>);
  private readonly destroy = inject(DestroyRef);
  private readonly platformService = inject(PlatformService);
  protected readonly templates = viewChild.required(DynoPopoverWidgets);

  protected readonly platform: SuTrayPlatform = this.platformService.platform === 'browser' ? 'macos' : this.platformService.platform;
  private readonly settingsKey = shortcutLabel(this.platform, 'settings');
  private readonly quitKey = shortcutLabel(this.platform, 'quit');
  /** The footer's more-actions menu: Customize widgets, Settings, then the two ways to quit (their words come from the engine). */
  protected readonly menu = computed<readonly SuTrayMenuEntry[]>(() => [
    { kind: 'item', id: 'customizeWidgets', label: 'Customize widgets…', icon: 'dy:sliders' },
    { kind: 'item', id: 'openSettings', label: 'Settings…', icon: 'dy:settings', keyHint: this.settingsKey },
    { kind: 'separator' },
    { kind: 'item', id: 'quit', label: this.quitText.title(), icon: 'dy:power', keyHint: this.quitKey },
    { kind: 'item', id: 'quitAndStop', label: QUIT_STOP, icon: 'dy:power' },
  ]);
  protected readonly meta = popoverMeta;
  /** The agent is away: the header says so, with no health score and no "Monitoring". */
  protected readonly stopped = computed(() => this.live.agent().state === 'failed' || this.values.values()?.noticeKind === 'agentStopped');
  protected readonly score = (v: number) => Math.round(v).toString();
  protected readonly grade = computed(() => (this.health.report() ? GRADE_LABEL[this.health.report()!.grade] : ''));
  protected readonly tone = computed(() => (this.health.report() ? GRADE_TONE[this.health.report()!.grade] : 'accent'));
  protected readonly agentText = computed(() => (this.live.agent().state === 'running' ? 'Monitoring' : this.live.agent().state === 'failed' ? 'Agent stopped' : 'Connecting…'));

  constructor() {
    // The popover is as tall as its content: the shell fits the window to the height the page reports.
    afterNextRender(() => {
      const popup = (this.host.nativeElement as HTMLElement).querySelector('pd-tray-popover');
      if (popup) this.destroy.onDestroy(observeHeight(popup, (h) => void this.backend.fitTray(h).catch(() => undefined)));
    });
  }

  ngOnInit(): void {
    // A score the engine does not provide yet is simply left out (the pill only renders with a report).
    void this.health.loadReport();
    void this.quitText.refresh();
    void this.layout.use('popover');
    void this.values.start();
  }

  protected run(action: WidgetActionId): void {
    void this.values.run(action);
  }

  protected close(): void {
    void this.backend.hideTray().catch(() => undefined);
  }

  protected openMain(): void {
    void this.backend.showMainWindow().catch(() => undefined);
  }

  /** What a footer menu item does. */
  protected choose(id: string): void {
    if (id === 'quit') this.quit();
    else if (id === 'quitAndStop') this.quitAndStop();
    else if (id === 'customizeWidgets' || id === 'openSettings') this.run(id);
  }

  private quit(): void {
    void this.backend.quitApp().catch(() => undefined);
  }

  private quitAndStop(): void {
    void this.backend.quitAndStopAgent().catch(() => undefined);
  }
}
