import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatTooltip } from '@angular/material/tooltip';
import { LiveStore } from '../core/live.store';
import { bytes, percent } from '../shared/format';
import { loadTone, servingLabel } from '../shared/labels';

/**
 * The status bar's server state: a dot and a word ("Serving", "Paused", "Stopped", "Unreachable", "Connecting"), the port in
 * its tooltip. Status is never colour alone: the word always says it.
 */
@Component({
  selector: 'pw-serving-status',
  imports: [MatTooltip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<span class="state" role="status" data-testid="serving-status" [attr.data-state]="state()" [matTooltip]="tip()"><span class="dot" aria-hidden="true"></span>{{ text() }}</span>`,
  styles: `
    :host { display: inline-flex; min-width: 0; }
    .state { display: inline-flex; align-items: center; gap: var(--pd-space-2); white-space: nowrap; }
    .dot { width: 8px; height: 8px; border-radius: 50%; background: var(--pd-text-subtle); }
    .state[data-state='serving'] .dot { background: var(--pd-success); }
    .state[data-state='paused'] .dot, .state[data-state='stopped'] .dot { background: var(--pd-warning); }
    .state[data-state='down'] .dot { background: var(--pd-danger); }
  `,
})
export class ServingStatusItem {
  private readonly live = inject(LiveStore);
  protected readonly state = computed(() => this.live.server()?.serving ?? (this.live.error() ? 'down' : 'connecting'));
  protected readonly text = computed(() => {
    const s = this.live.server();
    if (s) return servingLabel(s.serving);
    return this.live.error() ? 'Unreachable' : 'Connecting…';
  });
  protected readonly tip = computed(() => {
    const s = this.live.server();
    if (s) return `${servingLabel(s.serving)} on 127.0.0.1:${s.port}`;
    return this.live.error()?.message ?? 'Asking the Pitwall server…';
  });
}

/** The status bar's memory: RAM in use of the total (and the first GPU's VRAM), with a warning word past 85 %. */
@Component({
  selector: 'pw-memory-status',
  imports: [MatTooltip],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (ram(); as r) {
      <span class="mem" role="img" data-testid="memory-status" [attr.data-tone]="r.tone" [attr.aria-label]="label()" [matTooltip]="tip()">
        <span class="lbl">RAM</span><span class="num">{{ r.used }} / {{ r.total }}</span>
        @if (vram(); as v) {
          <span class="sep" aria-hidden="true">·</span><span class="lbl">VRAM</span><span class="num">{{ v }}</span>
        }
        @if (r.tone !== 'neutral') {
          <span class="warn">{{ r.tone === 'bad' ? 'Nearly full' : 'High' }}</span>
        }
      </span>
    }
  `,
  styles: `
    :host { display: inline-flex; min-width: 0; }
    .mem { display: inline-flex; align-items: center; gap: var(--pd-space-1); white-space: nowrap; }
    .lbl { color: var(--pd-text-subtle); }
    .num { font-variant-numeric: tabular-nums; }
    .sep { color: var(--pd-text-subtle); padding: 0 var(--pd-space-1); }
    .warn { margin-inline-start: var(--pd-space-1); color: var(--pd-warning); }
    .mem[data-tone='bad'] .warn { color: var(--pd-danger); }
  `,
})
export class MemoryStatusItem {
  private readonly live = inject(LiveStore);
  protected readonly ram = computed(() => {
    const m = this.live.memory();
    if (!m || !m.ramTotal) return null;
    const used = m.ramTotal - m.ramAvailable;
    return { used: bytes(used), total: bytes(m.ramTotal, 0), pct: percent(used, m.ramTotal), tone: loadTone(percent(used, m.ramTotal)) };
  });
  protected readonly vram = computed(() => {
    const g = this.live.memory()?.gpus[0];
    return g && g.vramTotal && g.vramUsed !== null ? `${bytes(g.vramUsed)} / ${bytes(g.vramTotal, 0)}` : null;
  });
  /** The whole fact in words, for screen readers: "Memory: 17.4 GB of 32 GB RAM in use, 10.2 GB / 12 GB VRAM". */
  protected readonly label = computed(() => {
    const r = this.ram();
    if (!r) return '';
    const v = this.vram();
    const warn = r.tone === 'bad' ? ', nearly full' : r.tone === 'warn' ? ', high' : '';
    return `Memory: ${r.used} of ${r.total} RAM in use${warn}${v ? `, ${v} VRAM` : ''}`;
  });
  protected readonly tip = computed(() => {
    const r = this.ram();
    return r ? `Memory in use: ${r.pct} % of RAM. The dashboard shows what is loaded.` : '';
  });
}
