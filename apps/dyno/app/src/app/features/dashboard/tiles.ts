import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import type { Reading } from '../../core/api.types';
import { formatBytes, formatDuration, formatFraction, formatGhz, formatPercent, formatRate, formatCount } from '../../shared/format';
import { PRESSURE_LABEL, PRESSURE_TONE, readingReason, readingValue } from '../../shared/labels';
import { Sparkline } from '../../shared/sparkline';
import { LiveStore } from '../../state/live.store';
import { Tile } from './tile';

/** The tone of a used-fraction bar (visual only: the number is always printed). */
function usedTone(percent: number): 'ok' | 'warn' | 'bad' | null {
  return percent >= 95 ? 'bad' : percent >= 85 ? 'warn' : null;
}

const combine = <T>(reading: Reading<T> | undefined) => ({ value: reading ? readingValue(reading) : null, reason: reading ? readingReason(reading) : null });

/** CPU: the big number, a minute of history and one cell per core. */
@Component({
  selector: 'dy-cpu-tile',
  imports: [Tile, Sparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="CPU" icon="cpu" [ready]="live.connected()">
      @if (cpu(); as c) {
        <div class="headline">
          <span class="big num">{{ pct(c.total) }}</span>
          <span class="sub num">{{ ghz(c.frequencyMhz) }} · base {{ ghz(c.baseFrequencyMhz) }}</span>
        </div>
        <dy-sparkline [values]="live.series().cpu" [max]="100" label="CPU use over the last minute" style="--spark-h: 56px" />
        <div class="cores" role="img" [attr.aria-label]="coreLabel()">
          @for (v of c.perCore; track $index) {
            <span class="core" [style.background]="coreColor(v)" [title]="'Core ' + count($index) + ': ' + pct(v)"></span>
          }
        </div>
        <p class="legend">{{ coresText() }}</p>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .headline { display: flex; align-items: baseline; justify-content: space-between; gap: var(--sp-3); flex-wrap: wrap; }
    .big { font: var(--fw-semibold) var(--fs-display) / 1 var(--font-display); letter-spacing: -0.03em; }
    .sub, .legend { color: var(--text-3); font-size: var(--fs-sm); }
    .cores { display: grid; grid-auto-flow: column; grid-auto-columns: 1fr; gap: 3px; height: 22px; }
    .core { border-radius: 3px; background-color: var(--surface-active); box-shadow: inset 0 0 0 1px var(--border-subtle); }
  `,
})
export class CpuTile {
  protected readonly live = inject(LiveStore);
  protected readonly cpu = computed(() => this.live.system()?.cpu ?? null);
  protected readonly pct = formatPercent;
  protected readonly ghz = formatGhz;
  protected readonly count = (i: number) => formatCount(i + 1);
  protected readonly coreColor = (v: number) => `color-mix(in srgb, var(--accent) ${Math.round(Math.min(100, Math.max(0, v)) * 0.9 + 6)}%, var(--surface-active))`;
  protected readonly coresText = computed(() => {
    const c = this.cpu();
    if (!c) return '';
    const hottest = Math.max(0, ...c.perCore);
    return `${formatCount(c.perCore.length)} logical cores · busiest ${formatPercent(hottest)}`;
  });
  protected readonly coreLabel = computed(() => `Per-core use: ${this.cpu()?.perCore.map((v) => formatPercent(v)).join(', ') ?? ''}`);
}

/** Memory: used of total, the OS's pressure level, commit and swap. */
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
        <div class="headline">
          <span class="big num">{{ bytes(m.used) }}</span>
          <span class="sub num">of {{ bytes(m.total) }} · {{ pct(usedPercent()) }}</span>
        </div>
        <div class="bar-track" [attr.data-tone]="barTone()" role="img" [attr.aria-label]="'Memory used ' + pct(usedPercent())"><span [style.width.%]="usedPercent()"></span></div>
        <dy-sparkline [values]="live.series().memory" [max]="100" label="Memory use over the last minute" style="--spark-h: 36px" />
        <dl>
          <div><dt>Available</dt><dd class="num">{{ bytes(m.available) }}</dd></div>
          <div><dt>Swap</dt><dd class="num">{{ bytes(m.swapUsed) }} of {{ bytes(m.swapTotal) }}</dd></div>
          <div>
            <dt>Commit</dt>
            <dd class="num">{{ commitText() }}</dd>
          </div>
        </dl>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .headline { display: flex; align-items: baseline; justify-content: space-between; gap: var(--sp-3); flex-wrap: wrap; }
    .big { font: var(--fw-semibold) var(--fs-3xl) / 1 var(--font-display); letter-spacing: -0.02em; }
    .sub { color: var(--text-3); font-size: var(--fs-sm); }
    dl { margin: 0; display: grid; gap: var(--sp-1); font-size: var(--fs-sm); }
    dl > div { display: flex; justify-content: space-between; gap: var(--sp-3); }
    dt { color: var(--text-3); }
    dd { margin: 0; color: var(--text-1); text-align: right; }
  `,
})
export class MemoryTile {
  protected readonly live = inject(LiveStore);
  protected readonly mem = computed(() => this.live.system()?.memory ?? null);
  protected readonly bytes = formatBytes;
  protected readonly pct = formatPercent;
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

/** Disks: throughput, the queue and one free-space bar per drive. */
@Component({
  selector: 'dy-disks-tile',
  imports: [Tile, Sparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="Disk" icon="disk" [ready]="live.connected()">
      @if (sys(); as s) {
        <div class="rates">
          <div><span class="k"><i class="swatch" style="background: var(--chart-1)"></i>Read</span><span class="v num">{{ rate(s.diskIo.readBps) }}</span></div>
          <div><span class="k"><i class="swatch" style="background: var(--chart-2)"></i>Write</span><span class="v num">{{ rate(s.diskIo.writeBps) }}</span></div>
          <div><span class="k">Busy</span><span class="v num">{{ busy() }}</span></div>
          <div><span class="k">Queue</span><span class="v num">{{ queue() }}</span></div>
        </div>
        <dy-sparkline
          [values]="live.series().diskRead"
          [values2]="live.series().diskWrite"
          color="var(--chart-1)"
          color2="var(--chart-2)"
          label="Disk read and write over the last minute"
          style="--spark-h: 40px"
        />
        <ul class="drives">
          @for (d of s.disks; track d.id) {
            <li>
              <div class="line">
                <span class="name truncate">{{ d.name }}<small class="mono"> {{ d.mount }}</small></span>
                <span class="num free">{{ bytes(d.free) }} free</span>
              </div>
              <div class="bar-track" [attr.data-tone]="tone(d.total, d.free)" role="img" [attr.aria-label]="d.name + ' ' + pct(used(d.total, d.free)) + ' full'">
                <span [style.width.%]="used(d.total, d.free)"></span>
              </div>
              <small class="faint num">{{ pct(used(d.total, d.free)) }} of {{ bytes(d.total) }} used</small>
            </li>
          }
        </ul>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .rates { display: grid; grid-template-columns: 1fr 1fr; gap: var(--sp-1) var(--sp-4); font-size: var(--fs-sm); }
    .rates > div { display: flex; justify-content: space-between; gap: var(--sp-2); }
    .k { color: var(--text-3); display: inline-flex; align-items: center; gap: 6px; }
    .v { color: var(--text-1); font-weight: var(--fw-medium); }
    .swatch { width: 8px; height: 8px; border-radius: 2px; display: inline-block; }
    .drives { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-3); }
    .drives li { display: grid; gap: 5px; }
    .line { display: flex; justify-content: space-between; gap: var(--sp-2); font-size: var(--fs-sm); }
    .name small { color: var(--text-3); }
    .free { color: var(--text-1); font-weight: var(--fw-medium); }
    small.faint { font-size: var(--fs-xs); }
  `,
})
export class DisksTile {
  protected readonly live = inject(LiveStore);
  protected readonly sys = computed(() => this.live.system());
  protected readonly bytes = formatBytes;
  protected readonly pct = formatPercent;
  protected readonly rate = formatRate;
  protected readonly used = (total: number, free: number) => (total > 0 ? ((total - free) / total) * 100 : 0);
  protected readonly tone = (total: number, free: number) => usedTone(this.used(total, free));
  protected readonly busy = computed(() => {
    const c = combine(this.sys()?.diskIo.busy);
    return c.value !== null ? formatPercent(c.value) : 'n/a';
  });
  protected readonly queue = computed(() => {
    const c = combine(this.sys()?.diskIo.queue);
    return c.value !== null ? c.value.toFixed(1) : 'n/a';
  });
}

/** Network: receive and send rates with their history, and the busiest adapters. */
@Component({
  selector: 'dy-network-tile',
  imports: [Tile, Sparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="Network" icon="network" [ready]="live.connected()">
      @if (sys(); as s) {
        <div class="rates">
          <div><span class="k"><i class="swatch" style="background: var(--chart-1)"></i>Down</span><span class="v num">{{ rate(rx()) }}</span></div>
          <div><span class="k"><i class="swatch" style="background: var(--chart-2)"></i>Up</span><span class="v num">{{ rate(tx()) }}</span></div>
        </div>
        <dy-sparkline [values]="live.series().netRx" [values2]="live.series().netTx" color="var(--chart-1)" color2="var(--chart-2)" label="Network down and up over the last minute" style="--spark-h: 48px" />
        <ul>
          @for (n of s.network; track n.name) {
            <li><span class="truncate">{{ n.name }}</span><span class="num faint">↓{{ rate(n.rxBps) }} ↑{{ rate(n.txBps) }}</span></li>
          }
        </ul>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .rates { display: flex; justify-content: space-between; gap: var(--sp-3); }
    .rates > div { display: grid; gap: 2px; }
    .k { color: var(--text-3); font-size: var(--fs-sm); display: inline-flex; align-items: center; gap: 6px; }
    .v { font: var(--fw-semibold) var(--fs-lg) / 1.1 var(--font-display); letter-spacing: -0.01em; white-space: nowrap; }
    .swatch { width: 8px; height: 8px; border-radius: 2px; display: inline-block; }
    ul { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-1); font-size: var(--fs-xs); }
    li { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: var(--sp-2); }
    .faint { color: var(--text-3); white-space: nowrap; }
  `,
})
export class NetworkTile {
  protected readonly live = inject(LiveStore);
  protected readonly sys = computed(() => this.live.system());
  protected readonly rate = formatRate;
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
        <div class="headline">
          <span class="big num">{{ pct(g.utilization) }}</span>
          <span class="sub truncate">{{ g.name }}</span>
        </div>
        <dy-sparkline [values]="live.series().gpu" [max]="100" label="GPU use over the last minute" style="--spark-h: 36px" />
        <div class="vram">
          <span class="k">Video memory</span>
          <span class="num">{{ bytes(g.memoryUsed) }} of {{ bytes(g.memoryTotal) }}</span>
        </div>
        <div class="bar-track" role="img" [attr.aria-label]="'Video memory used ' + frac(g.memoryUsed / g.memoryTotal)"><span [style.width.%]="(g.memoryUsed / g.memoryTotal) * 100"></span></div>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .headline { display: flex; align-items: baseline; justify-content: space-between; gap: var(--sp-3); min-width: 0; }
    .big { font: var(--fw-semibold) var(--fs-3xl) / 1 var(--font-display); letter-spacing: -0.02em; }
    .sub { color: var(--text-3); font-size: var(--fs-xs); text-align: right; }
    .vram { display: flex; justify-content: space-between; font-size: var(--fs-sm); }
    .k { color: var(--text-3); }
  `,
})
export class GpuTile {
  protected readonly live = inject(LiveStore);
  protected readonly gpu = computed(() => combine(this.live.system()?.gpu));
  protected readonly pct = formatPercent;
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
        <div class="headline">
          <span class="big num">{{ pct(b.charge) }}</span>
          <span class="sub num">{{ remaining() }}</span>
        </div>
        <div class="bar-track" role="img" [attr.aria-label]="'Charge ' + pct(b.charge)"><span [style.width.%]="b.charge"></span></div>
        <dl>
          <div><dt>Battery health</dt><dd class="num">{{ pct(b.health) }} of design capacity</dd></div>
          <div><dt>Charge cycles</dt><dd class="num">{{ cycles() }}</dd></div>
        </dl>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    .headline { display: flex; align-items: baseline; justify-content: space-between; gap: var(--sp-3); }
    .big { font: var(--fw-semibold) var(--fs-3xl) / 1 var(--font-display); letter-spacing: -0.02em; }
    .sub { color: var(--text-3); font-size: var(--fs-sm); }
    dl { margin: 0; display: grid; gap: var(--sp-1); font-size: var(--fs-sm); }
    dl > div { display: flex; justify-content: space-between; gap: var(--sp-2); }
    dt { color: var(--text-3); }
    dd { margin: 0; }
  `,
})
export class BatteryTile {
  protected readonly live = inject(LiveStore);
  protected readonly battery = computed(() => combine(this.live.system()?.battery));
  protected readonly pct = formatPercent;
  protected readonly remaining = computed(() => {
    const b = this.battery().value;
    if (!b) return '';
    if (b.source === 'ac') return 'Charging or full';
    return b.secondsToEmpty === null ? 'Estimating time left…' : `${formatDuration(b.secondsToEmpty)} left`;
  });
  protected readonly cycles = computed(() => {
    const c = this.battery().value?.cycles;
    return c === null || c === undefined ? 'unknown' : formatCount(c);
  });
}

/** Temperatures and fans (each may be unavailable on its own). */
@Component({
  selector: 'dy-sensors-tile',
  imports: [Tile],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-tile title="Temperatures and fans" icon="temperature" [ready]="live.connected()" [reason]="bothMissing()">
      @if (temps().value; as list) {
        <ul>
          @for (t of list; track t.label) {
            <li>
              <span class="truncate">{{ t.label }}</span>
              <span class="num v">{{ celsius(t.celsius) }}</span>
              <div class="bar-track" [attr.data-tone]="t.critical !== null && t.celsius >= t.critical - 10 ? 'warn' : null" role="presentation">
                <span [style.width.%]="share(t.celsius, t.critical)"></span>
              </div>
            </li>
          }
        </ul>
      } @else {
        <p class="note"><strong>Temperatures:</strong> not available. {{ temps().reason }}</p>
      }
      @if (fans().value; as list) {
        <ul class="fans">
          @for (f of list; track f.label) {
            <li><span class="truncate">{{ f.label }}</span><span class="num v">{{ rpm(f.rpm) }}</span></li>
          }
        </ul>
      } @else {
        <p class="note"><strong>Fans:</strong> not available. {{ fans().reason }}</p>
      }
    </dy-tile>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    dy-tile { height: 100%; }
    ul { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-2); font-size: var(--fs-sm); }
    li { display: grid; grid-template-columns: 1fr auto; gap: 4px var(--sp-2); }
    li .bar-track { grid-column: 1 / -1; height: 4px; }
    .fans { grid-template-columns: 1fr; }
    .v { font-weight: var(--fw-medium); }
    .note { color: var(--text-2); font-size: var(--fs-xs); }
    .note strong { color: var(--text-1); font-weight: var(--fw-medium); }
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
}
