import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { RouterLink } from '@angular/router';
import type { Reading } from '../../core/api.types';
import { formatBytes, formatDuration, formatFraction, formatGhz, formatPercent, formatRate, formatCount, splitValue } from '../../shared/format';
import { PRESSURE_LABEL, PRESSURE_TONE, readingReason, readingValue } from '../../shared/labels';
import { Sparkline } from '../../shared/sparkline';
import { LiveStore } from '../../state/live.store';
import { Icon } from '../../shared/icon';
import { ViewPrefs } from '../../core/view-prefs';
import { coreGroups } from '../cpu/core-layout';
import { Tile } from './tile';

/** The tone of a used-fraction bar (visual only: the number is always printed). */
function usedTone(percent: number): 'ok' | 'warn' | 'bad' | null {
  return percent >= 95 ? 'bad' : percent >= 85 ? 'warn' : null;
}

const combine = <T>(reading: Reading<T> | undefined) => ({ value: reading ? readingValue(reading) : null, reason: reading ? readingReason(reading) : null });

/** The number and unit of a formatted value, for the hero (`<b>` number, `<small>` unit). */
const parts = splitValue;

/** CPU: hero, one sparkline, caption rows; the toggle switches to one live bar per core grouped by core kind. */
@Component({
  selector: 'dy-cpu-tile',
  imports: [Tile, Sparkline, RouterLink, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="CPU" icon="cpu" [ready]="live.connected()">
      @if (cpu()) {
        <span badge class="head">
          <span class="seg sm" role="group" aria-label="CPU view">
            <button type="button" [attr.aria-pressed]="prefs.cpuView() === 'overall'" (click)="prefs.setCpuView('overall')">Overall</button>
            <button type="button" [attr.aria-pressed]="prefs.cpuView() === 'cores'" (click)="prefs.setCpuView('cores')">Per core</button>
          </span>
          <a class="btn ghost icon sm" routerLink="/cpu" aria-label="CPU details" title="CPU details"><dy-icon name="arrow-right" /></a>
        </span>
      }
      @if (cpu(); as c) {
        <div class="hero"><b>{{ total(c.total)[0] }}</b><small>{{ total(c.total)[1] }}</small></div>
        @if (prefs.cpuView() === 'overall') {
          <dy-sparkline [values]="live.series().cpu" [max]="100" color="var(--accent)" label="CPU use over the last minute" style="--spark-h: 32px" />
          <div class="cores" role="img" [attr.aria-label]="coreLabel()">
            @for (v of c.perCore; track $index) {
              <span class="core" [style.background]="coreColor(v)" [title]="'Core ' + count($index) + ': ' + pct(v)"></span>
            }
          </div>
          <dl class="caps">
            <div><dt>Frequency</dt><dd>{{ ghz(c.frequencyMhz) }} · base {{ ghz(c.baseFrequencyMhz) }}</dd></div>
            <div><dt>Cores</dt><dd>{{ coresText() }}</dd></div>
          </dl>
        } @else {
          <div class="groups">
            @for (g of groups(); track g.kind) {
              <section [attr.aria-label]="g.title">
                <h3>{{ g.title }}</h3>
                <ul>
                  @for (core of g.cores; track core.index) {
                    <li>
                      <span class="lbl num">{{ core.label }}</span>
                      <span class="bar-track" role="img" [attr.aria-label]="core.label + ' ' + pct(c.perCore[core.index] ?? 0)"><span [style.width.%]="c.perCore[core.index] ?? 0"></span></span>
                      <span class="val num">{{ pct(c.perCore[core.index] ?? 0) }}</span>
                    </li>
                  }
                </ul>
              </section>
            }
          </div>
        }
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .head { display: inline-flex; align-items: center; gap: var(--sp-1); }
    .seg.sm button { height: 20px; padding: 0 var(--sp-2); font-size: var(--fs-xs); }
    .cores { display: grid; grid-auto-flow: column; grid-auto-columns: 1fr; gap: 2px; height: 12px; }
    .core { border-radius: 2px; background-color: var(--surface-active); }
    .groups { display: grid; gap: var(--sp-3); }
    h3 { font-size: var(--fs-xs); color: var(--text-3); font-weight: var(--fw-semibold); letter-spacing: 0.04em; text-transform: uppercase; margin-bottom: var(--sp-1); }
    ul { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 3px var(--sp-4); }
    li { display: grid; grid-template-columns: 22px minmax(0, 1fr) 38px; align-items: center; gap: var(--sp-2); font-size: var(--fs-xs); }
    .lbl { color: var(--text-3); }
    .val { text-align: right; }
    .bar-track { height: 4px; }
  `,
})
export class CpuTile {
  protected readonly live = inject(LiveStore);
  protected readonly prefs = inject(ViewPrefs);
  protected readonly cpu = computed(() => this.live.system()?.cpu ?? null);
  protected readonly groups = computed(() => {
    const c = this.cpu();
    return c ? coreGroups(c.coreKinds, c.perCore.length) : [];
  });
  protected readonly pct = formatPercent;
  protected readonly ghz = formatGhz;
  protected readonly total = (v: number) => parts(formatPercent(v));
  protected readonly count = (i: number) => formatCount(i + 1);
  /** A single-hue ramp from neutral to the accent (percent load; the number is in the tooltip). */
  protected readonly coreColor = (v: number) => `color-mix(in srgb, var(--accent) ${Math.round(Math.min(100, Math.max(0, v)) * 0.85)}%, var(--surface-active))`;
  protected readonly coresText = computed(() => {
    const c = this.cpu();
    if (!c) return '';
    return `${formatCount(c.perCore.length)} logical · busiest ${formatPercent(Math.max(0, ...c.perCore))}`;
  });
  protected readonly coreLabel = computed(() => `Per-core use: ${this.cpu()?.perCore.map((v) => formatPercent(v)).join(', ') ?? ''}`);
}

/** Memory: used (hero) of total, the OS's pressure level, commit and swap. */
@Component({
  selector: 'dy-memory-tile',
  imports: [Tile, Sparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="Memory" icon="memory" [ready]="live.connected()">
      @if (mem()) {
        <span badge class="badge" [attr.data-tone]="tone()">{{ pressureText() }}</span>
      }
      @if (mem(); as m) {
        <div class="hero"><b>{{ used(m.used)[0] }}</b><small>{{ used(m.used)[1] }} of {{ bytes(m.total) }}</small></div>
        <div class="bar-track" [attr.data-tone]="barTone()" role="img" [attr.aria-label]="'Memory used ' + pct(usedPercent())"><span [style.width.%]="usedPercent()"></span></div>
        <dy-sparkline [values]="live.series().memory" [max]="100" label="Memory use over the last minute" style="--spark-h: 32px" />
        <dl class="caps">
          <div><dt>In use</dt><dd>{{ pct(usedPercent()) }}</dd></div>
          <div><dt>Available</dt><dd>{{ bytes(m.available) }}</dd></div>
          <div><dt>Swap</dt><dd>{{ bytes(m.swapUsed) }} of {{ bytes(m.swapTotal) }}</dd></div>
          <div><dt>Commit</dt><dd>{{ commitText() }}</dd></div>
        </dl>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .bar-track { height: 4px; }
  `,
})
export class MemoryTile {
  protected readonly live = inject(LiveStore);
  protected readonly mem = computed(() => this.live.system()?.memory ?? null);
  protected readonly bytes = formatBytes;
  protected readonly pct = formatPercent;
  protected readonly used = (v: number) => parts(formatBytes(v));
  protected readonly usedPercent = computed(() => {
    const m = this.mem();
    return m && m.total > 0 ? (m.used / m.total) * 100 : 0;
  });
  protected readonly barTone = computed(() => usedTone(this.usedPercent()));
  protected readonly tone = computed(() => PRESSURE_TONE[this.live.update()?.pressure ?? 'normal']);
  protected readonly pressureText = computed(() => `Pressure: ${PRESSURE_LABEL[this.live.update()?.pressure ?? 'normal']}`);
  protected readonly commitText = computed(() => {
    const c = combine(this.mem()?.commit);
    return c.value ? `${formatBytes(c.value.used)} of ${formatBytes(c.value.limit)}` : (c.reason ?? '—');
  });
}

/** Disks: throughput (hero = read), the queue, and the fullest drive's free space. */
@Component({
  selector: 'dy-disks-tile',
  imports: [Tile, Sparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="Disk" icon="disk" [ready]="live.connected()">
      @if (sys(); as s) {
        @if (s.disks.length > 1) {
          <span badge class="badge outline">{{ n(s.disks.length) }} volumes</span>
        }
        <div class="hero"><b>{{ rd(s.diskIo.readBps)[0] }}</b><small>{{ rd(s.diskIo.readBps)[1] }} read</small></div>
        <dy-sparkline [values]="live.series().diskRead" [values2]="live.series().diskWrite" color="var(--chart-1)" color2="var(--chart-2)" label="Disk read and write over the last minute" style="--spark-h: 32px" />
        <dl class="caps">
          <div><dt><i class="swatch" style="background: var(--chart-2)"></i>Write</dt><dd>{{ rate(s.diskIo.writeBps) }}</dd></div>
          <div><dt>Busy · queue</dt><dd>{{ busy() }} · {{ queue() }}</dd></div>
        </dl>
        @if (fullest(); as d) {
          <div class="drive">
            <div class="line"><span class="truncate">{{ d.name }} <small class="mono">{{ d.mount }}</small></span><span class="num">{{ bytes(d.free) }} free</span></div>
            <div class="bar-track" [attr.data-tone]="tone(d.total, d.free)" role="img" [attr.aria-label]="d.name + ' ' + pct(used(d.total, d.free)) + ' full'"><span [style.width.%]="used(d.total, d.free)"></span></div>
          </div>
        }
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .swatch { width: 8px; height: 8px; border-radius: 2px; display: inline-block; margin-right: 6px; }
    .drive { display: grid; gap: 5px; font-size: var(--fs-sm); }
    .line { display: flex; justify-content: space-between; gap: var(--sp-2); }
    .line small { color: var(--text-3); }
    .bar-track { height: 4px; }
  `,
})
export class DisksTile {
  protected readonly live = inject(LiveStore);
  protected readonly sys = computed(() => this.live.system());
  protected readonly bytes = formatBytes;
  protected readonly pct = formatPercent;
  protected readonly rate = formatRate;
  protected readonly n = formatCount;
  protected readonly rd = (bps: number) => parts(formatRate(bps));
  protected readonly used = (total: number, free: number) => (total > 0 ? ((total - free) / total) * 100 : 0);
  protected readonly tone = (total: number, free: number) => usedTone(this.used(total, free));
  /** Only the fullest volume is drawn; the badge says how many there are. */
  protected readonly fullest = computed(() => [...(this.sys()?.disks ?? [])].sort((a, b) => this.used(b.total, b.free) - this.used(a.total, a.free))[0] ?? null);
  protected readonly busy = computed(() => {
    const c = combine(this.sys()?.diskIo.busy);
    return c.value !== null ? formatPercent(c.value) : 'n/a';
  });
  protected readonly queue = computed(() => {
    const c = combine(this.sys()?.diskIo.queue);
    return c.value !== null ? c.value.toFixed(1) : 'n/a';
  });
}

/** Network: hero = down, with up as a caption; both series on one sparkline. */
@Component({
  selector: 'dy-network-tile',
  imports: [Tile, Sparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="Network" icon="network" [ready]="live.connected()">
      @if (sys(); as s) {
        <div class="hero"><b>{{ down(rx())[0] }}</b><small>{{ down(rx())[1] }} down</small></div>
        <dy-sparkline [values]="live.series().netRx" [values2]="live.series().netTx" color="var(--chart-1)" color2="var(--chart-2)" label="Network down and up over the last minute" style="--spark-h: 32px" />
        <dl class="caps">
          <div><dt><i class="swatch" style="background: var(--chart-2)"></i>Up</dt><dd>{{ rate(tx()) }}</dd></div>
          @if (s.network[0]; as n) {
            <div><dt class="truncate">{{ n.name }}</dt><dd>↓ {{ rate(n.rxBps) }} · ↑ {{ rate(n.txBps) }}</dd></div>
          }
        </dl>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .swatch { width: 8px; height: 8px; border-radius: 2px; display: inline-block; margin-right: 6px; }
  `,
})
export class NetworkTile {
  protected readonly live = inject(LiveStore);
  protected readonly sys = computed(() => this.live.system());
  protected readonly rate = formatRate;
  protected readonly down = (bps: number) => parts(formatRate(bps));
  protected readonly rx = computed(() => this.sys()?.network.reduce((a, n) => a + n.rxBps, 0) ?? 0);
  protected readonly tx = computed(() => this.sys()?.network.reduce((a, n) => a + n.txBps, 0) ?? 0);
}

/** GPU: utilisation and video memory, or why the platform cannot tell. */
@Component({
  selector: 'dy-gpu-tile',
  imports: [Tile, Sparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="GPU" icon="gpu" [ready]="live.connected()" [reason]="gpu().reason">
      @if (gpu().value; as g) {
        <div class="hero"><b>{{ pct(g.utilization)[0] }}</b><small>{{ pct(g.utilization)[1] }}</small></div>
        <dy-sparkline [values]="live.series().gpu" [max]="100" label="GPU use over the last minute" style="--spark-h: 32px" />
        <dl class="caps">
          <div><dt>Video memory</dt><dd>{{ bytes(g.memoryUsed) }} of {{ bytes(g.memoryTotal) }}</dd></div>
          <div><dt class="truncate">{{ g.name }}</dt><dd>{{ frac(g.memoryUsed / g.memoryTotal) }} used</dd></div>
        </dl>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
  `,
})
export class GpuTile {
  protected readonly live = inject(LiveStore);
  protected readonly gpu = computed(() => combine(this.live.system()?.gpu));
  protected readonly pct = (v: number) => parts(formatPercent(v));
  protected readonly bytes = formatBytes;
  protected readonly frac = formatFraction;
}

/** Battery: charge, power source, wear and time left. */
@Component({
  selector: 'dy-battery-tile',
  imports: [Tile],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="Battery" icon="battery" [ready]="live.connected()" [reason]="battery().reason">
      @if (battery().value; as b) {
        <span badge class="badge" [attr.data-tone]="b.source === 'ac' ? 'ok' : 'info'">{{ b.source === 'ac' ? 'Plugged in' : 'On battery' }}</span>
      }
      @if (battery().value; as b) {
        <div class="hero"><b>{{ pct(b.charge)[0] }}</b><small>{{ pct(b.charge)[1] }} · {{ remaining() }}</small></div>
        <div class="bar-track" role="img" [attr.aria-label]="'Charge ' + pct(b.charge).join('')"><span [style.width.%]="b.charge"></span></div>
        <dl class="caps">
          <div><dt>Battery health</dt><dd>{{ health(b.health) }} of design capacity</dd></div>
          <div><dt>Charge cycles</dt><dd>{{ cycles() }}</dd></div>
        </dl>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .bar-track { height: 4px; }
  `,
})
export class BatteryTile {
  protected readonly live = inject(LiveStore);
  protected readonly battery = computed(() => combine(this.live.system()?.battery));
  protected readonly pct = (v: number) => parts(formatPercent(v));
  protected readonly health = formatPercent;
  protected readonly remaining = computed(() => {
    const b = this.battery().value;
    if (!b) return '';
    if (b.source === 'ac') return 'charging or full';
    return b.secondsToEmpty === null ? 'estimating' : `${formatDuration(b.secondsToEmpty)} left`;
  });
  protected readonly cycles = computed(() => {
    const c = this.battery().value?.cycles;
    return c === null || c === undefined ? 'unknown' : formatCount(c);
  });
}

/** Temperatures and fans (each may be unavailable on its own); hero = the first temperature. */
@Component({
  selector: 'dy-sensors-tile',
  imports: [Tile],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="Temperatures" icon="temperature" [ready]="live.connected()" [reason]="bothMissing()">
      @if (temps().value; as list) {
        @if (list[0]; as first) {
          <div class="hero"><b>{{ first.celsius.toFixed(0) }}</b><small>°C · {{ first.label }}</small></div>
          <div class="bar-track" [attr.data-tone]="near(first)" role="presentation"><span [style.width.%]="share(first.celsius, first.critical)"></span></div>
        }
        <dl class="caps">
          @for (t of list.slice(1); track t.label) {
            <div><dt>{{ t.label }}</dt><dd>{{ celsius(t.celsius) }}</dd></div>
          }
          @if (fans().value; as f) {
            @for (x of f; track x.label) {
              <div><dt>{{ x.label }}</dt><dd>{{ rpm(x.rpm) }}</dd></div>
            }
          } @else {
            <div><dt>Fans</dt><dd class="muted">not available</dd></div>
          }
        </dl>
        @if (fans().reason; as why) {
          <p class="note">{{ why }}</p>
        }
      } @else {
        <p class="note"><strong>Temperatures:</strong> not available. {{ temps().reason }}</p>
        @if (fans().value; as f) {
          <dl class="caps">
            @for (x of f; track x.label) {
              <div><dt>{{ x.label }}</dt><dd>{{ rpm(x.rpm) }}</dd></div>
            }
          </dl>
        }
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .bar-track { height: 4px; }
    .note { color: var(--text-3); font-size: var(--fs-xs); }
    .note strong { color: var(--text-2); font-weight: var(--fw-medium); }
  `,
})
export class SensorsTile {
  protected readonly live = inject(LiveStore);
  protected readonly temps = computed(() => combine(this.live.system()?.temperatures));
  protected readonly fans = computed(() => combine(this.live.system()?.fans));
  protected readonly bothMissing = computed(() => {
    const t = this.temps();
    const f = this.fans();
    return t.reason && f.reason ? `${t.reason} ${f.reason}` : null;
  });
  protected readonly celsius = (v: number) => `${v.toFixed(0)} °C`;
  protected readonly rpm = (v: number) => `${formatCount(v)} rpm`;
  protected readonly share = (v: number, critical: number | null) => Math.min(100, Math.max(0, (v / (critical ?? 100)) * 100));
  protected readonly near = (t: { celsius: number; critical: number | null }) => (t.critical !== null && t.celsius >= t.critical - 10 ? 'warn' : null);
}
