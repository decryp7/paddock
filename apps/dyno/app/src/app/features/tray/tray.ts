import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { formatBytes, formatPercent, formatRate } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { GRADE_LABEL, GRADE_TONE } from '../../shared/labels';
import { Notifier, Toasts } from '../../shared/notifier';
import { Sparkline } from '../../shared/sparkline';
import { ProfileSwitcher } from '../../shell/profile-switcher';
import { HealthStore } from '../../state/health.store';
import { LiveStore } from '../../state/live.store';
import { RulesStore } from '../../state/rules.store';
import { SettingsStore } from '../../state/settings.store';

/**
 * The tray dropdown (360 x 520, frameless): a compact dashboard with the health score when the engine has one, four
 * live sparklines, the top five apps, the profile and the three buttons. The tray / menu-bar icon itself is drawn by the shell.
 */
@Component({
  selector: 'dy-tray',
  imports: [Icon, Sparkline, ProfileSwitcher, Toasts, MatTooltip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <main class="tray">
      <header>
        <img src="dyno-logo.svg" alt="" width="22" height="22" />
        <strong>Dyno</strong>
        <span class="spacer"></span>
        @if (health.report(); as r) {
          <span class="grade-pill" [attr.data-tone]="tone()" [matTooltip]="'Health score ' + score(r.score) + ' of 100'"><strong class="num">{{ score(r.score) }}</strong> {{ grade() }}</span>
        }
        <span class="agent" [attr.data-state]="live.agent().state" role="status"><span class="pulse"></span>{{ agentText() }}</span>
      </header>

      <section class="spark" aria-label="Live">
        <div class="cell">
          <span class="k"><dy-icon name="cpu" /> CPU</span>
          <span class="v num">{{ pct(cpu()) }}</span>
          <dy-sparkline [values]="live.series().cpu" [max]="100" label="CPU" style="--spark-h: 28px" />
        </div>
        <div class="cell">
          <span class="k"><dy-icon name="memory" /> Memory</span>
          <span class="v num">{{ pct(mem()) }}</span>
          <dy-sparkline [values]="live.series().memory" [max]="100" color="var(--chart-2)" label="Memory" style="--spark-h: 28px" />
        </div>
        <div class="cell">
          <span class="k"><dy-icon name="disk" /> Disk</span>
          <span class="v num">{{ rate(disk()) }}</span>
          <dy-sparkline [values]="live.series().diskRead" [values2]="live.series().diskWrite" color="var(--chart-1)" color2="var(--chart-2)" label="Disk" style="--spark-h: 28px" />
        </div>
        <div class="cell">
          <span class="k"><dy-icon name="network" /> Network</span>
          <span class="v num">{{ rate(net()) }}</span>
          <dy-sparkline [values]="live.series().netRx" [values2]="live.series().netTx" color="var(--chart-1)" color2="var(--chart-2)" label="Network" style="--spark-h: 28px" />
        </div>
      </section>

      <section class="top" aria-label="Top apps">
        <span class="eyebrow">Top apps by CPU</span>
        <ol>
          @for (g of top(); track g.id) {
            <li><span class="truncate">{{ g.name }}</span><span class="num">{{ pct(g.cpu) }}</span><span class="num faint">{{ bytes(g.memory) }}</span></li>
          } @empty {
            <li class="faint">Waiting for the first reading…</li>
          }
        </ol>
      </section>

      <footer>
        <dy-profile-switcher [compact]="true" />
        <button type="button" class="btn sm icon" [disabled]="!rules.loaded() || rules.saving()" (click)="rules.pause(!rules.paused())" [attr.aria-label]="rules.paused() ? 'Resume rules' : 'Pause rules'" [matTooltip]="rules.paused() ? 'Resume rules' : 'Pause all rules'"><dy-icon [name]="rules.paused() ? 'play' : 'pause'" /></button>
        <span class="spacer"></span>
        <button type="button" class="btn sm primary" (click)="openMain()"><dy-icon name="arrow-right" /> Open Dyno</button>
        <button type="button" class="btn sm icon" (click)="quit()" aria-label="Quit Dyno" matTooltip="Quit the Dyno app (the background agent keeps measuring)"><dy-icon name="power" /></button>
      </footer>
    </main>
    <dy-toasts />
  `,
  styles: `
    :host { display: block; width: 100vw; height: 100vh; background: var(--overlay-bg); }
    .tray { height: 100%; display: flex; flex-direction: column; gap: var(--sp-3); padding: var(--sp-3) var(--sp-4) var(--sp-3); overflow: hidden; }
    header { display: flex; align-items: center; gap: var(--sp-2); }
    .spacer { flex: 1; }
    .agent { display: flex; align-items: center; gap: 6px; color: var(--text-3); font-size: var(--fs-xs); }
    .pulse { width: 7px; height: 7px; border-radius: 50%; background: var(--ok); }
    .agent[data-state='failed'] .pulse { background: var(--bad); }
    .agent[data-state='starting'] .pulse, .agent[data-state='restarting'] .pulse { background: var(--warn); }
    .grade-pill { display: inline-flex; align-items: center; gap: 6px; height: 22px; padding: 0 var(--sp-2); border-radius: var(--r-pill); font-size: var(--fs-xs); background: var(--ok-soft); color: var(--ok); }
    .grade-pill[data-tone='warn'] { background: var(--warn-soft); color: var(--warn); }
    .grade-pill[data-tone='bad'] { background: var(--bad-soft); color: var(--bad); }
    .score { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-2) var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-inset); }
    .score .n { font: var(--fw-semibold) var(--fs-lg) / 1 var(--font-display); }
    .score small { display: block; color: var(--text-3); }
    .grade { font: var(--fw-semibold) var(--fs-lg) / 1.2 var(--font-display); }
    .grade[data-tone='ok'] { color: var(--ok); } .grade[data-tone='warn'] { color: var(--warn); } .grade[data-tone='bad'] { color: var(--bad); }
    .spark { display: grid; grid-template-columns: 1fr 1fr; gap: var(--sp-2); }
    .cell { display: grid; grid-template-columns: 1fr auto; gap: 2px var(--sp-2); padding: var(--sp-2) var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); }
    .cell dy-sparkline { grid-column: 1 / -1; }
    .k { display: inline-flex; align-items: center; gap: 5px; color: var(--text-3); font-size: var(--fs-xs); }
    .k dy-icon { --icon-size: 13px; }
    .v { font-weight: var(--fw-semibold); font-size: var(--fs-md); white-space: nowrap; }
    .top { display: grid; gap: var(--sp-1); min-height: 0; }
    ol { list-style: none; margin: 0; padding: 0; display: grid; }
    li { display: grid; grid-template-columns: minmax(0, 1fr) 52px 70px; gap: var(--sp-2); padding: 5px 0; border-bottom: 1px solid var(--border-subtle); font-size: var(--fs-sm); }
    li .num:nth-of-type(1) { text-align: right; font-weight: var(--fw-medium); }
    li .faint { text-align: right; }
    .faint { color: var(--text-3); }
    footer { margin-top: auto; display: flex; align-items: center; gap: var(--sp-2); }
  `,
})
export class Tray implements OnInit {
  protected readonly live = inject(LiveStore);
  protected readonly rules = inject(RulesStore);
  protected readonly health = inject(HealthStore);
  private readonly settings = inject(SettingsStore);
  private readonly backend = inject(BACKEND);
  private readonly notifier = inject(Notifier);

  protected readonly pct = formatPercent;
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

  ngOnInit(): void {
    // Theme first, then the rest; nothing here blocks the first paint.
    void this.settings.load();
    void this.rules.ensure();
    // A score the engine does not provide yet is simply left out (the card only renders with a report).
    void this.health.loadReport();
  }

  protected async openMain(): Promise<void> {
    try {
      await this.backend.trayOpenMain();
    } catch (e) {
      this.notifier.error(toCommandError(e));
    }
  }

  protected async quit(): Promise<void> {
    try {
      await this.backend.trayQuit();
    } catch (e) {
      this.notifier.error(toCommandError(e));
    }
  }
}
