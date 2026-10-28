import { showMeasured } from '../../core/measured';
import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, OnInit, afterNextRender, computed, inject } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';
import { SuTrayPopover, type SuTrayPlatform } from '@decryp7/paddock-ui';
import { BACKEND } from '../../core/backend';
import { formatBytes, formatPercent, formatRate } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { GRADE_LABEL, GRADE_TONE } from '../../shared/labels';
import { observeHeight } from '../../shared/observe-height';
import { PlatformService } from '../../core/platform';
import { Sparkline } from '../../shared/sparkline';
import { HealthStore } from '../../state/health.store';
import { LiveStore } from '../../state/live.store';
import { MatButton, MatIconButton } from '@angular/material/button';

/**
 * The tray dropdown (360 x 520, frameless): a compact dashboard with the health score when the engine has one, four
 * live sparklines, the top five apps, and the buttons. The tray / menu-bar icon itself is drawn by the shell.
 */
@Component({
  selector: 'dy-tray',
  imports: [MatButton, MatIconButton, Icon, Sparkline, SuTrayPopover, MatTooltip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pd-tray-popover label="Dyno" [platform]="platform" (dismissed)="close()">
      <div slot="header" class="head">
        <img src="brand/dyno-16.svg" alt="" width="16" height="16" />
        <strong>Dyno</strong>
        <span class="spacer"></span>
        @if (health.report(); as r) {
          <span class="grade-pill" [attr.data-tone]="tone()" [matTooltip]="'Health score ' + score(r.score) + ' of 100'"><strong class="num">{{ score(r.score) }}</strong> {{ grade() }}</span>
        }
        <span class="agent" [attr.data-state]="live.agent().state" role="status"><span class="pulse"></span>{{ agentText() }}</span>
      </div>
      <main class="tray" #body>
        <section class="metrics" aria-label="Live">
          <div class="cell">
            <span class="k"><dy-icon name="cpu" /> CPU</span>
            <span class="v num">{{ pct(cpu()) }}</span>
            <dy-sparkline [values]="live.series().cpu" [max]="100" color="var(--accent)" label="CPU" style="--spark-h: 28px" />
          </div>
          <div class="cell">
            <span class="k"><dy-icon name="memory" /> Memory</span>
            <span class="v num">{{ pct(mem()) }}</span>
            <dy-sparkline [values]="live.series().memory" [max]="100" [zoom]="true" color="var(--chart-1)" label="Memory" style="--spark-h: 28px" />
          </div>
          <div class="cell">
            <span class="k"><dy-icon name="disk" /> Disk</span>
            <span class="v num">{{ rate(disk()) }}</span>
            <dy-sparkline [values]="live.series().diskRead" [values2]="live.series().diskWrite" color2="var(--text-3)" label="Disk" style="--spark-h: 28px" />
          </div>
          <div class="cell">
            <span class="k"><dy-icon name="network" /> Network</span>
            <span class="v num">{{ rate(net()) }}</span>
            <dy-sparkline [values]="live.series().netRx" [values2]="live.series().netTx" color2="var(--text-3)" label="Network" style="--spark-h: 28px" />
          </div>
        </section>

        <section class="top" aria-label="Top apps">
          <span class="eyebrow">Top apps by CPU</span>
          <ol>
            @for (g of top(); track g.id) {
              <li><span class="truncate">{{ g.name }}</span><span class="num">{{ show(g.cpu, pct) }}</span><span class="num faint">{{ show(g.memory, bytes) }}</span></li>
            } @empty {
              <li class="faint">Waiting for the first reading…</li>
            }
          </ol>
        </section>
      </main>
      <div slot="footer" class="foot">
        <span class="spacer"></span>
        <button type="button" matButton="filled" (click)="openMain()"><dy-icon name="arrow-right" /> Open Dyno</button>
        <button type="button" matIconButton (click)="quit()" aria-label="Quit Dyno" matTooltip="Quit the Dyno app (the background agent keeps measuring)"><dy-icon name="power" /></button>
      </div>
    </pd-tray-popover>
  `,
  styles: `
    :host { display: block; }
    .tray { display: grid; gap: var(--sp-3); }
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
    .metrics { display: grid; grid-template-columns: 1fr 1fr; grid-auto-rows: 1fr; gap: var(--sp-2); }
    .cell { display: grid; grid-template-columns: max-content minmax(24px, 1fr); grid-template-rows: auto 28px; align-items: center; gap: 4px var(--sp-2); padding: var(--sp-2) var(--sp-3); border: 1px solid var(--pd-hairline); border-radius: var(--r-lg); background: color-mix(in srgb, var(--surface-2) 55%, transparent); min-width: 0; }
    .cell .k { grid-column: 1 / -1; }
    .k { display: inline-flex; align-items: center; gap: 5px; min-width: 0; overflow: hidden; white-space: nowrap; color: var(--text-2); font-size: var(--fs-xs); }
    .k dy-icon { flex: none; }
    .k dy-icon { --icon-size: 13px; }
    .v { font: var(--fw-semibold) 18px / 1.1 var(--font-display); font-variant-numeric: tabular-nums; text-align: left; white-space: nowrap; }
    .top { display: grid; gap: 2px; }
    .top .eyebrow { padding: 0 var(--sp-1); margin-bottom: 2px; }
    ol { list-style: none; margin: 0; padding: 0; display: grid; }
    li { display: grid; grid-template-columns: minmax(0, 1fr) 52px 70px; gap: var(--sp-2); padding: 5px var(--sp-2); border-radius: var(--r-md); font-size: var(--fs-sm); }
    li:hover { background: var(--surface-hover); }
    li .num { text-align: right; }
    li .num:nth-of-type(1) { font-weight: var(--fw-medium); }
    .faint { color: var(--text-3); }
    .foot { display: flex; align-items: center; gap: var(--sp-1); flex: 1; }
  `,
})
export class Tray implements OnInit {
  protected readonly live = inject(LiveStore);
  protected readonly health = inject(HealthStore);
  private readonly backend = inject(BACKEND);
  private readonly host = inject(ElementRef<HTMLElement>);
  private readonly destroy = inject(DestroyRef);
  private readonly platformService = inject(PlatformService);

  protected readonly platform: SuTrayPlatform = this.platformService.platform === 'browser' ? 'macos' : this.platformService.platform;
  protected readonly pct = formatPercent;
  protected readonly show = showMeasured;
  protected readonly rate = formatRate;
  protected readonly bytes = formatBytes;
  protected readonly score = (v: number) => Math.round(v).toString();
  protected readonly cpu = computed(() => this.live.system()?.cpu.total ?? 0);
  protected readonly mem = computed(() => {
    const m = this.live.system()?.memory;
    return m && m.total > 0 ? (m.used / m.total) * 100 : 0;
  });
  protected readonly disk = computed(() => (this.live.system()?.diskIo.readBps ?? 0) + (this.live.system()?.diskIo.writeBps ?? 0));
  protected readonly net = computed(() => this.live.system()?.network.reduce((a, n) => a + n.rxBps + n.txBps, 0) ?? 0);
  protected readonly top = computed(() => (this.live.update()?.topCpu ?? []).slice(0, 5));
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
    // A score the engine does not provide yet is simply left out (the card only renders with a report).
    void this.health.loadReport();
    // A popover opens in a fresh webview: its graphs start from the engine's recent history, not empty, then follow live.
    void this.backend.history('hour', null).then((h) => this.live.seed(h.points)).catch(() => undefined);
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
