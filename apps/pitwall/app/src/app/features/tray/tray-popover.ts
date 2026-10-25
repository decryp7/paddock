import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { LiveStore } from '../../core/live.store';
import { WINDOW_CONTROL } from '../../core/window-control';
import { bytes, percent, rate } from '../../shared/format';
import { loadTone, pressureLook, servingLabel } from '../../shared/labels';
import { injectNow } from '../../shared/now';
import { HotModels } from '../dashboard/hot-models';
import { UpdatesStore } from '../updates/updates.store';

/** The tray / menu-bar popover (360×520, frameless): memory, hot models with countdowns, throughput and the key actions. */
@Component({
  selector: 'pw-tray-popover',
  imports: [MatIconModule, HotModels],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="tray">
      <header>
        <mat-icon class="logo" svgIcon="pw:logo" aria-hidden="true" />
        <h1>Pitwall</h1>
        <div class="title">
          @if (live.server(); as s) {
            <span class="state" [attr.data-state]="s.serving"><span class="dot" aria-hidden="true"></span>{{ servingLabel(s.serving) }} on :{{ s.port }}</span>
          } @else if (live.error()) {
            <span class="state" data-state="down"><span class="dot" aria-hidden="true"></span>Server unreachable</span>
          } @else {
            <span class="state"><span class="dot" aria-hidden="true"></span>Connecting…</span>
          }
        </div>
        <button type="button" class="btn ghost icon sm" aria-label="Open Pitwall" (click)="open('/')"><mat-icon svgIcon="pw:open" aria-hidden="true" /></button>
      </header>

      @if (live.pairings().length; as n) {
        <button type="button" class="pairing" (click)="open('/')">
          <mat-icon svgIcon="pw:apps" aria-hidden="true" />
          <span>{{ n === 1 ? 'An app wants to pair' : n + ' apps want to pair' }}. <b>Review</b></span>
        </button>
      }

      @if (updates.count(); as u) {
        <button type="button" class="pairing" (click)="open('/models')">
          <mat-icon svgIcon="pw:download" aria-hidden="true" />
          <span>{{ u === 1 ? 'A model update is available' : u + ' model updates are available' }}. <b>Review</b></span>
        </button>
      }

      @if (live.error() && !live.overview()) {
        <p class="down" role="alert">The Pitwall server is not responding. <button type="button" class="btn sm" (click)="live.load()">Retry</button></p>
      }

      @if (live.memory(); as m) {
        <section class="mem" aria-label="Memory">
          <div class="row">
            <span class="k">RAM</span>
            <span class="v num">{{ size(m.ramTotal - m.ramAvailable) }} <span class="faint">/ {{ size(m.ramTotal, 0) }}</span></span>
            @if (pressure(); as p) {
              @if (p.tone !== 'ok') {
                <span class="status" [attr.data-tone]="p.tone">{{ p.label }}</span>
              }
            }
          </div>
          <div class="meter" [attr.data-tone]="tone(ramPct())"><span [style.width.%]="ramPct()"></span></div>
          @if (vram(); as g) {
            <div class="row">
              <span class="k">VRAM</span>
              <span class="v num">{{ size(g.used) }} <span class="faint">/ {{ size(g.total, 0) }}</span></span>
              <span class="faint small">{{ g.util === null ? '' : g.util + '% busy' }}</span>
            </div>
            <div class="meter" [attr.data-tone]="tone(g.pct)"><span [style.width.%]="g.pct"></span></div>
          }
        </section>
      }

      <section class="stats" aria-label="Throughput">
        <div><b class="num">{{ tps() }}</b><span>tok/s</span></div>
        <div><b class="num">{{ live.queue()?.running?.length ?? 0 }}</b><span>running</span></div>
        <div><b class="num">{{ live.queue()?.waiting?.length ?? 0 }}</b><span>waiting</span></div>
      </section>

      <section class="models" aria-label="Loaded models">
        <h2>Loaded models</h2>
        <pw-hot-models [now]="now()" [compact]="true" [limit]="modelLimit()" />
      </section>

      <footer>
        @if (live.paused()) {
          <button type="button" class="btn" [disabled]="live.isBusy()('serving')" (click)="live.setServing(false)"><mat-icon svgIcon="pw:play" aria-hidden="true" />Resume</button>
        } @else {
          <button type="button" class="btn" [disabled]="live.isBusy()('serving') || !live.overview()" (click)="live.setServing(true)"><mat-icon svgIcon="pw:pause" aria-hidden="true" />Pause serving</button>
        }
        <button type="button" class="btn" [disabled]="live.isBusy()('release') || !live.overview()" (click)="live.releaseMemory()"><mat-icon svgIcon="pw:release" aria-hidden="true" />Release memory</button>
        <button type="button" class="btn primary wide" (click)="open('/')">Open Pitwall</button>
      </footer>
    </div>
  `,
  styles: `
    :host {
      display: block;
      height: 100vh;
    }
    .tray {
      display: flex;
      flex-direction: column;
      gap: var(--sp-2);
      height: 100%;
      padding: var(--sp-3) var(--sp-4) var(--sp-4);
      background: var(--bg-content);
      overflow: hidden;
    }
    .pairing {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      padding: var(--sp-2) var(--sp-3);
      font: inherit;
      font-size: var(--fs-sm);
      color: var(--text);
      text-align: left;
      background: var(--accent-soft);
      border: 1px solid color-mix(in srgb, var(--accent) 35%, transparent);
      border-radius: var(--r-md);
      mat-icon {
        color: var(--accent);
      }
    }
    .down {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--sp-2);
      padding: var(--sp-2) var(--sp-3);
      font-size: var(--fs-sm);
      background: var(--bad-soft);
      border-radius: var(--r-md);
    }
    header {
      display: flex;
      align-items: center;
      gap: 10px;
    }
    .logo {
      width: 24px;
      height: 24px;
      flex: none;
    }
    h1 {
      font-size: var(--fs-lg);
    }
    .title {
      flex: 1;
      display: flex;
      justify-content: flex-start;
      min-width: 0;
    }
    /* The state is one pill with an 8 px dot, like the window's sidebar. */
    .state {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      height: 22px;
      padding: 0 8px;
      font-size: var(--fs-xs);
      font-weight: var(--fw-medium);
      color: var(--text-2);
      background: var(--bg-active);
      border-radius: var(--r-full);
      white-space: nowrap;
      .dot {
        width: 8px;
        height: 8px;
        border-radius: 50%;
        background: var(--text-3);
      }
      &[data-state='serving'] .dot {
        background: var(--ok);
      }
      &[data-state='paused'] .dot,
      &[data-state='stopped'] .dot {
        background: var(--warn);
      }
      &[data-state='down'] .dot {
        background: var(--bad);
      }
    }
    .mem,
    .stats,
    .models {
      padding: var(--sp-3);
      background: var(--bg-card);
      border: 1px solid var(--border);
      border-radius: var(--r-lg);
    }
    .mem {
      display: grid;
      gap: 6px;
      .row {
        display: flex;
        align-items: center;
        gap: var(--sp-2);
      }
      .k {
        width: 40px;
        font-size: var(--fs-xs);
        color: var(--text-2);
      }
      .v {
        flex: 1;
        font-weight: var(--fw-medium);
      }
      .small {
        font-size: var(--fs-xs);
      }
      .meter {
        margin-bottom: 4px;
      }
    }
    .stats {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      padding: var(--sp-2) var(--sp-3);
      > div {
        display: flex;
        flex-direction: column;
        align-items: center;
      }
      b {
        font-size: var(--fs-xl);
        font-weight: var(--fw-semibold);
        font-variant-numeric: tabular-nums;
        line-height: 1.2;
      }
      span {
        font-size: var(--fs-xs);
        color: var(--text-2);
      }
    }
    .models {
      flex: 1;
      min-height: 0;
      overflow: auto;
      padding-top: var(--sp-2);
      h2 {
        margin-bottom: var(--sp-1);
        font-size: var(--fs-xs);
        font-weight: var(--fw-medium);
        color: var(--text-2);
      }
    }
    footer {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: var(--sp-2);
      .wide {
        grid-column: 1 / -1;
        height: 32px;
      }
    }
  `,
})
export class TrayPopover {
  protected readonly live = inject(LiveStore);
  protected readonly updates = inject(UpdatesStore);
  private readonly windows = inject(WINDOW_CONTROL);
  protected readonly servingLabel = servingLabel;
  protected readonly now = injectNow();
  protected readonly size = bytes;

  protected readonly pressure = computed(() => {
    const m = this.live.memory();
    return m ? pressureLook(m.pressure) : null;
  });
  protected readonly tone = loadTone;
  protected readonly ramPct = computed(() => {
    const m = this.live.memory();
    return m ? percent(m.ramTotal - m.ramAvailable, m.ramTotal) : 0;
  });
  protected readonly vram = computed(() => {
    const g = this.live.memory()?.gpus[0];
    if (!g || !g.vramTotal || g.vramUsed === null) return null;
    return { used: g.vramUsed, total: g.vramTotal, pct: percent(g.vramUsed, g.vramTotal), util: g.utilisation };
  });
  /** Two loaded models fit under a banner, three without one; the rest is summarised so no row is cut off. */
  protected readonly modelLimit = computed(() => (this.live.pairings().length > 0 || this.updates.count() > 0 ? 2 : 3));
  protected readonly tps = computed(() => (this.live.paused() ? '0.0' : rate(this.live.throughput()?.genTps ?? 0)));

  constructor() {
    void this.updates.ensureLoaded();
  }

  protected open(route: string): void {
    void this.windows.openMain(route);
  }
}
