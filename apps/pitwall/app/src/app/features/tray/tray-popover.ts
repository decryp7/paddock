import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { LiveStore } from '../../core/live.store';
import { WINDOW_CONTROL } from '../../core/window-control';
import { bytes, percent, rate } from '../../shared/format';
import { loadTone, pressureLook, servingLabel } from '../../shared/labels';
import { injectNow } from '../../shared/now';
import { HotModels } from '../dashboard/hot-models';
import { UpdatesStore } from '../updates/updates.store';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatTooltip } from '@angular/material/tooltip';

/** The tray / menu-bar popover (360×520, frameless): memory, hot models with countdowns, throughput and the key actions. */
@Component({
  selector: 'pw-tray-popover',
  imports: [MatTooltip, MatIconButton, MatButton, MatIconModule, HotModels],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="tray">
      <header>
        <img class="logo" src="brand/pitwall-macos.svg" alt="" width="24" height="24" />
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
        <button type="button" matIconButton aria-label="Open Pitwall" matTooltip="Open Pitwall" (click)="open('/')"><mat-icon svgIcon="pw:open" aria-hidden="true" /></button>
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
        <p class="down" role="alert">The Pitwall server is not responding. <button type="button" matButton="outlined" (click)="live.load()">Retry</button></p>
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
          <button type="button" matButton="outlined" [disabled]="live.isBusy()('serving')" (click)="live.setServing(false)"><mat-icon svgIcon="pw:play" aria-hidden="true" />Resume</button>
        } @else {
          <button type="button" matButton="outlined" [disabled]="live.isBusy()('serving') || !live.overview()" (click)="live.setServing(true)"><mat-icon svgIcon="pw:pause" aria-hidden="true" />Pause serving</button>
        }
        <button type="button" matButton="outlined" [disabled]="live.isBusy()('release') || !live.overview()" (click)="live.releaseMemory()"><mat-icon svgIcon="pw:release" aria-hidden="true" />Release memory</button>
        <button type="button" matButton="filled" class="wide" (click)="open('/')">Open Pitwall</button>
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
      gap: var(--pd-space-2);
      height: 100%;
      padding: var(--pd-space-3) var(--pd-space-4) var(--pd-space-4);
      background: var(--pd-bg);
      overflow: hidden;
    }
    .pairing {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      padding: var(--pd-space-2) var(--pd-space-3);
      font: inherit;
      font-size: var(--pd-text-label);
      color: var(--pd-text);
      text-align: left;
      background: var(--pd-accent-tint);
      border: 1px solid color-mix(in srgb, var(--pd-accent) 35%, transparent);
      border-radius: var(--pd-radius-control);
      mat-icon {
        color: var(--pd-accent);
      }
    }
    .down {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--pd-space-2);
      padding: var(--pd-space-2) var(--pd-space-3);
      font-size: var(--pd-text-label);
      background: var(--pd-danger-tint);
      border-radius: var(--pd-radius-control);
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
      font-size: var(--pd-text-subtitle);
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
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-medium);
      color: var(--pd-text-muted);
      background: var(--pd-pressed);
      border-radius: var(--pd-radius-pill);
      white-space: nowrap;
      .dot {
        width: 8px;
        height: 8px;
        border-radius: 50%;
        background: var(--pd-text-subtle);
      }
      &[data-state='serving'] .dot {
        background: var(--pd-success);
      }
      &[data-state='paused'] .dot,
      &[data-state='stopped'] .dot {
        background: var(--pd-warning);
      }
      &[data-state='down'] .dot {
        background: var(--pd-danger);
      }
    }
    .mem,
    .stats,
    .models {
      padding: var(--pd-space-3);
      background: var(--pd-surface-1);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
    }
    .mem {
      display: grid;
      gap: 6px;
      .row {
        display: flex;
        align-items: center;
        gap: var(--pd-space-2);
      }
      .k {
        width: 40px;
        font-size: var(--pd-text-label);
        color: var(--pd-text-muted);
      }
      .v {
        flex: 1;
        font-weight: var(--pd-weight-medium);
      }
      .small {
        font-size: var(--pd-text-label);
      }
      .meter {
        margin-bottom: 4px;
      }
    }
    .stats {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      padding: var(--pd-space-2) var(--pd-space-3);
      > div {
        display: flex;
        flex-direction: column;
        align-items: center;
      }
      b {
        font-size: var(--pd-text-title);
        font-weight: var(--pd-weight-semibold);
        font-variant-numeric: tabular-nums;
        line-height: 1.2;
      }
      span {
        font-size: var(--pd-text-label);
        color: var(--pd-text-muted);
      }
    }
    .models {
      flex: 1;
      min-height: 0;
      overflow: auto;
      padding-top: var(--pd-space-2);
      h2 {
        margin-bottom: var(--pd-space-1);
        font-size: var(--pd-text-label);
        font-weight: var(--pd-weight-medium);
        color: var(--pd-text-muted);
      }
    }
    footer {
      display: grid;
      grid-template-columns: 1fr 1fr;
      gap: var(--pd-space-2);
      /* Two actions side by side in a 360 px popover: each on one line, with less side padding than a page button. */
      button {
        min-width: 0;
        white-space: nowrap;
        --mat-button-outlined-horizontal-padding: var(--pd-space-2);
      }
      .wide {
        grid-column: 1 / -1;
        --mat-button-filled-container-height: 32px;
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
