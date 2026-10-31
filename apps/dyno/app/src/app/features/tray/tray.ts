import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, OnInit, afterNextRender, computed, inject, viewChild } from '@angular/core';
import { MatMenu, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { MatTooltip } from '@angular/material/tooltip';
import { SuTrayPopover, type SuTrayPlatform } from '@decryp7/paddock-ui';
import { PdWidgetArea } from '@decryp7/paddock-ui/widgets';
import { BACKEND } from '../../core/backend';
import type { WidgetActionId } from '../../core/generated/widgets';
import { Icon } from '../../shared/icon';
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
import { MatButton, MatIconButton } from '@angular/material/button';

/**
 * The tray dropdown (360 wide, frameless): the health pill and agent state, the person's widgets on `pd-widget-area`
 * (family ADR 0035: the layout and every value come from the engine), and the footer with Open Dyno and the ⋯ menu. The
 * tray / menu-bar icon itself is drawn by the shell.
 */
@Component({
  selector: 'dy-tray',
  imports: [MatButton, MatIconButton, MatMenu, MatMenuItem, MatMenuTrigger, Icon, SuTrayPopover, MatTooltip, PdWidgetArea, DynoPopoverWidgets],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pd-tray-popover label="Dyno" [platform]="platform" (dismissed)="close()">
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
      <div slot="footer" class="foot">
        <span class="spacer"></span>
        <button type="button" matButton="filled" (click)="openMain()"><dy-icon name="arrow-right" /> Open Dyno</button>
        <button type="button" matIconButton [matMenuTriggerFor]="more" aria-label="More actions" matTooltip="More actions" matTooltipPosition="left"><dy-icon name="more" /></button>
        <mat-menu #more="matMenu" yPosition="above" xPosition="before">
          <button mat-menu-item type="button" (click)="run('customizeWidgets')"><dy-icon name="sliders" />Customize widgets…</button>
          <button mat-menu-item type="button" (click)="run('openSettings')"><dy-icon name="settings" />Settings…@if (settingsKey; as k) {<kbd aria-hidden="true">{{ k }}</kbd>}</button>
          <hr class="sep" />
          <button mat-menu-item type="button" (click)="quit()"><dy-icon name="power" />Quit Dyno@if (quitKey; as k) {<kbd aria-hidden="true">{{ k }}</kbd>}</button>
        </mat-menu>
      </div>
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
    /* The shared menu icon size: 16 px. */
    .mat-mdc-menu-item dy-icon { flex: none; width: 16px; height: 16px; --icon-size: 16px; margin: 0 12px 0 0; vertical-align: middle; }
    .sep { border: 0; border-top: 1px solid var(--pd-hairline); margin: 4px 0; }
    kbd { margin-left: auto; padding: 0 4px; border: 1px solid var(--pd-border-strong); border-radius: 4px; font: inherit; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .foot { display: flex; align-items: center; gap: var(--sp-1); flex: 1; }
  `,
})
export class Tray implements OnInit {
  protected readonly live = inject(LiveStore);
  protected readonly health = inject(HealthStore);
  protected readonly layout = inject(WidgetLayouts);
  protected readonly values = inject(PopoverValues);
  private readonly backend = inject(BACKEND);
  private readonly host = inject(ElementRef<HTMLElement>);
  private readonly destroy = inject(DestroyRef);
  private readonly platformService = inject(PlatformService);
  protected readonly templates = viewChild.required(DynoPopoverWidgets);

  protected readonly platform: SuTrayPlatform = this.platformService.platform === 'browser' ? 'macos' : this.platformService.platform;
  protected readonly settingsKey = shortcutLabel(this.platform, 'settings');
  protected readonly quitKey = shortcutLabel(this.platform, 'quit');
  protected readonly meta = popoverMeta;
  /** The agent is away: the header says so, with no health score and no "Monitoring". */
  protected readonly stopped = computed(() => this.live.agent().state === 'failed' || this.values.values()?.notice?.action?.action === 'restartAgent');
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

  protected quit(): void {
    void this.backend.quitApp().catch(() => undefined);
  }
}
