import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import type { BenchKind, BenchMetric, BenchRun, BenchStatus, MetricDelta } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatDayTime, formatMetric, formatPercent } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { BenchStore } from '../../state/bench.store';
import { DeviceArt } from '../inventory/device-art';
import { BenchArt } from './bench-art';
import { barFraction, bestValues, kindTitle, statusNote, verdictWord } from './bench-model';

const KINDS: { kind: BenchKind; text: string }[] = [
  { kind: 'cpu', text: 'Integer work on one core and on all of them' },
  { kind: 'memory', text: 'Copy bandwidth and random-access latency' },
  { kind: 'disk', text: 'A capped scratch file, removed afterwards' },
  { kind: 'gpu', text: 'Optional; not included in this build' },
];

/** Benchmarks: started only by a click, each in its own process with a timeout; the load at the time is stored with the result. */
@Component({
  selector: 'dy-bench-page',
  imports: [AsyncState, BenchArt, DeviceArt, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Benchmarks</h1>
          <p>Measure the CPU, memory and disk on request, and compare before and after a change. Nothing runs until you click, and it all stays on this PC.</p>
        </div>
      </header>

      <dy-async [loaded]="!!store.overview()" [error]="store.error()" what="benchmarks" [rows]="4" (retry)="store.load()">
        @if (store.overview(); as o) {
          <section class="card" aria-label="Run benchmarks">
            <div class="card-head">
              <h2><dy-icon name="gauge" /> Run</h2>
              <span class="spacer"></span>
              @if (o.running) {
                <button type="button" class="btn sm" (click)="store.cancel()"><dy-icon name="stop" /> Cancel</button>
              }
            </div>
            @if (o.running; as r) {
              <ol class="steps" aria-live="polite" aria-label="Progress">
                @for (k of r.kinds; track k) {
                  <li [attr.data-state]="r.done.includes(k) ? 'done' : r.current === k ? 'now' : 'wait'">
                    <span class="dot" aria-hidden="true"></span>
                    <span class="name">{{ title(k) }}</span>
                    <span class="st">{{ r.done.includes(k) ? 'Done' : r.current === k ? 'Measuring…' : 'Waiting' }}</span>
                  </li>
                }
              </ol>
              <p class="note">Keep the PC otherwise idle while this runs; any other work lowers the numbers.</p>
            } @else {
              <div class="kinds" role="group" aria-label="What to measure">
                @for (k of kinds; track k.kind) {
                  <label class="kind" [attr.data-on]="chosen().has(k.kind)">
                    <input type="checkbox" [checked]="chosen().has(k.kind)" (change)="toggle(k.kind)" [attr.aria-label]="title(k.kind)" />
                    <span class="art" aria-hidden="true">
                      @switch (k.kind) {
                        @case ('cpu') { <dy-device-art kind="cpu" /> }
                        @case ('disk') { <dy-device-art kind="nvme" [totalBytes]="0" /> }
                        @default { <dy-bench-art [kind]="k.kind === 'gpu' ? 'gpu' : 'memory'" /> }
                      }
                    </span>
                    <strong>{{ title(k.kind) }}</strong>
                    <span class="sub">{{ k.text }}</span>
                  </label>
                }
              </div>
              <div class="runbar">
                <div class="field grow">
                  <label for="bench-label">Name this run</label>
                  <input id="bench-label" class="input" type="text" maxlength="80" placeholder="Baseline" [value]="label()" (input)="label.set($any($event.target).value)" />
                </div>
                <button type="button" class="btn primary" [disabled]="store.busy() || chosen().size === 0" (click)="run()"><dy-icon name="play" /> Run benchmarks</button>
              </div>
              <div class="pair">
                <div class="field grow">
                  <label for="bench-change">Before and after a change</label>
                  <input id="bench-change" class="input" type="text" maxlength="80" placeholder="What you are about to change, e.g. Power mode: High performance" [value]="change()" (input)="change.set($any($event.target).value)" />
                  <span class="hint">Measure before, make the change, then measure after. Dyno compares the two.</span>
                </div>
                <button type="button" class="btn" [disabled]="store.busy() || chosen().size === 0 || !change().trim()" (click)="run('before')">1 · Measure before</button>
                <button type="button" class="btn" [disabled]="store.busy() || chosen().size === 0 || !change().trim() || !hasBefore()" (click)="run('after')">2 · Measure after</button>
              </div>
            }
          </section>

          @if (latest(); as run) {
            <section class="card" aria-label="Latest result">
              <div class="card-head">
                <h2><dy-icon name="trend" /> {{ run.label }}</h2>
                <span class="spacer"></span>
                <span class="sub">{{ day(run.startedAt) }}</span>
              </div>
              <p class="load" aria-label="Machine load when the run started">
                Machine at the start: <span class="num">{{ pct(run.load.cpuPercent) }}</span> CPU, <span class="num">{{ pct(run.load.memoryPercent) }}</span> memory{{ run.load.onBattery === true ? ', on battery' : run.load.onBattery === false ? ', on mains power' : '' }}
                @if (run.load.cpuPercent > 20) { <span class="badge" data-tone="warn">Busy machine: numbers may be low</span> }
              </p>
              <div class="results">
                @for (r of run.results; track r.kind) {
                  <article class="result" [attr.data-state]="r.status.state">
                    <div class="art" aria-hidden="true">
                      @switch (r.kind) {
                        @case ('cpu') { <dy-device-art kind="cpu" /> }
                        @case ('disk') { <dy-device-art kind="nvme" [totalBytes]="0" /> }
                        @default { <dy-bench-art [kind]="r.kind === 'gpu' ? 'gpu' : 'memory'" /> }
                      }
                    </div>
                    <div class="body">
                      <h3>{{ title(r.kind) }}</h3>
                      @if (metrics(r.status); as ms) {
                        <ul class="metrics">
                          @for (m of ms; track m.id) {
                            <li>
                              <span class="label">{{ m.label }}</span>
                              <span class="value num">{{ metric(m.value) }} <small>{{ m.unit }}</small></span>
                              <span class="bar" role="presentation"><i [style.width.%]="bar(m)"></i></span>
                            </li>
                          }
                        </ul>
                        <span class="took">Took {{ seconds(r.millis) }}</span>
                      } @else {
                        <p class="why"><dy-icon name="info" /> {{ note(r.status) }}</p>
                      }
                    </div>
                  </article>
                }
              </div>
            </section>
          }

          @if (store.comparison(); as c) {
            <section class="card" aria-label="Comparison">
              <div class="card-head">
                <h2><dy-icon name="diff" /> Comparison</h2>
                <span class="spacer"></span>
                <button type="button" class="btn sm ghost" (click)="store.clearComparison()">Close</button>
              </div>
              @for (w of c.warnings; track w) {
                <p class="warning"><dy-icon name="alert" /> {{ w }}</p>
              }
              @if (c.deltas.length === 0) {
                <p class="note">The two runs share no finished measurements.</p>
              } @else {
                <table class="table">
                  <caption class="visually-hidden">Before and after</caption>
                  <thead><tr><th>Measurement</th><th class="r">Before</th><th class="r">After</th><th class="r">Change</th><th>Verdict</th></tr></thead>
                  <tbody>
                    @for (d of c.deltas; track d.kind + d.id) {
                      <tr>
                        <td>{{ d.label }} <span class="faint">({{ title(d.kind) }})</span></td>
                        <td class="r num">{{ metric(d.before) }} <small>{{ d.unit }}</small></td>
                        <td class="r num">{{ metric(d.after) }} <small>{{ d.unit }}</small></td>
                        <td class="r num">{{ signed(d) }}</td>
                        <td><span class="badge" [attr.data-tone]="tone(d)">{{ word(d) }}</span></td>
                      </tr>
                    }
                  </tbody>
                </table>
              }
            </section>
          }

          <section class="card" aria-label="History">
            <div class="card-head">
              <h2><dy-icon name="calendar" /> History</h2>
              <span class="spacer"></span>
              <button type="button" class="btn sm" [disabled]="picked().length !== 2" (click)="compare()"><dy-icon name="diff" /> Compare selected</button>
            </div>
            @if (o.runs.length === 0) {
              <p class="note">No runs yet. Choose what to measure above and click Run benchmarks.</p>
            } @else {
              <table class="table">
                <caption class="visually-hidden">Saved runs</caption>
                <thead><tr><th><span class="visually-hidden">Select</span></th><th>Run</th><th>When</th><th class="r">CPU load</th><th>Measured</th><th><span class="visually-hidden">Actions</span></th></tr></thead>
                <tbody>
                  @for (r of o.runs; track r.id) {
                    <tr>
                      <td><input type="checkbox" [checked]="picked().includes(r.id)" [attr.aria-label]="'Select ' + r.label" (change)="pick(r.id)" /></td>
                      <td>{{ r.label }}@if (r.phase) { <span class="badge outline">{{ r.phase === 'before' ? 'Before' : 'After' }}</span> }</td>
                      <td>{{ day(r.startedAt) }}</td>
                      <td class="r num">{{ pct(r.load.cpuPercent) }}</td>
                      <td>{{ kindsOf(r) }}</td>
                      <td class="r"><button type="button" class="btn sm ghost" [attr.aria-label]="'Delete ' + r.label" (click)="store.remove(r.id)"><dy-icon name="trash" /></button></td>
                    </tr>
                  }
                </tbody>
              </table>
            }
          </section>
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    dy-async { display: grid; gap: var(--sp-4); }
    .page { display: grid; gap: var(--sp-4); align-content: start; max-width: 980px; }
    .kinds { display: grid; grid-template-columns: repeat(auto-fill, minmax(200px, 1fr)); gap: var(--sp-3); padding: var(--sp-3) var(--sp-4) 0; }
    .kind { position: relative; display: grid; gap: var(--sp-1); padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-inset); cursor: pointer; }
    .kind[data-on='true'] { border-color: var(--accent); background: var(--accent-soft); }
    .kind input { position: absolute; top: var(--sp-3); right: var(--sp-3); }
    .kind .art { display: grid; place-items: center; height: 64px; margin-bottom: var(--sp-1); }
    .kind .art > * { width: 100%; max-height: 64px; }
    .kind .art dy-device-art { width: 64px; }
    .kind .art dy-device-art[kind='nvme'], .result .art dy-device-art[kind='nvme'] { width: 100%; }
    .sub { color: var(--text-2); font-size: var(--fs-sm); }
    .runbar, .pair { display: flex; gap: var(--sp-3); align-items: flex-end; padding: var(--sp-3) var(--sp-4) 0; flex-wrap: wrap; }
    .pair { padding-bottom: var(--sp-4); border-top: 1px solid var(--border-subtle); margin-top: var(--sp-4); }
    .grow { flex: 1 1 260px; }
    .steps { list-style: none; margin: 0; padding: var(--sp-3) var(--sp-4); display: grid; gap: var(--sp-2); }
    .steps li { display: grid; grid-template-columns: 14px 1fr auto; gap: var(--sp-3); align-items: center; }
    .steps .dot { width: 10px; height: 10px; border-radius: 50%; background: var(--border-strong); }
    .steps [data-state='now'] .dot { background: var(--accent); }
    .steps [data-state='done'] .dot { background: var(--ok); }
    .steps .st { color: var(--text-2); font-size: var(--fs-sm); }
    .note, .warning, .load { margin: 0; padding: var(--sp-2) var(--sp-4); color: var(--text-2); }
    .load { display: flex; gap: var(--sp-2); align-items: center; flex-wrap: wrap; }
    .warning { display: flex; gap: var(--sp-2); align-items: center; color: var(--warn); }
    .results { display: grid; grid-template-columns: repeat(auto-fill, minmax(420px, 1fr)); gap: var(--sp-3); padding: var(--sp-2) var(--sp-4) 0; }
    .result { display: flex; gap: var(--sp-4); padding: var(--sp-3) var(--sp-4); border-radius: var(--r-lg); background: var(--surface-inset); min-width: 0; align-items: flex-start; }
    .result .art { flex: none; width: 96px; display: grid; place-items: center; padding-top: var(--sp-1); }
    .result .art dy-device-art { width: 72px; }
    .result .art { width: 120px; }
    .result .body { min-width: 0; flex: 1; }
    h3 { margin: 0 0 var(--sp-2); font: var(--fw-semibold) var(--fs-md) / 1.3 var(--font-sans); }
    .metrics { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-2); }
    .metrics li { display: grid; grid-template-columns: 1fr auto; gap: 2px var(--sp-3); }
    .metrics .label { color: var(--text-2); font-size: var(--fs-sm); }
    .metrics .value { font: var(--fw-semibold) var(--fs-lg) / 1.2 var(--font-display); font-variant-numeric: tabular-nums; text-align: right; }
    .metrics .value small { font: var(--fw-regular) var(--fs-xs) / 1 var(--font-sans); color: var(--text-3); }
    .bar { grid-column: 1 / -1; height: 5px; border-radius: 3px; background: var(--surface-active); overflow: hidden; }
    .bar i { display: block; height: 100%; background: var(--accent); border-radius: 3px; min-width: 2px; }
    .took { display: block; margin-top: var(--sp-2); color: var(--text-3); font-size: var(--fs-xs); }
    .why { margin: 0; display: flex; gap: var(--sp-2); color: var(--text-2); font-size: var(--fs-sm); }
    .table { margin: 0 var(--sp-4) var(--sp-3); width: calc(100% - 2 * var(--sp-4)); }
    .faint { color: var(--text-3); }
    small { color: var(--text-3); }
    .badge.outline { margin-left: var(--sp-2); }
  `,
})
export class BenchPage implements OnInit, OnDestroy {
  protected readonly store = inject(BenchStore);
  protected readonly kinds = KINDS;
  protected readonly chosen = signal<ReadonlySet<BenchKind>>(new Set<BenchKind>(['cpu', 'memory', 'disk']));
  protected readonly label = signal('');
  protected readonly change = signal('');
  protected readonly picked = signal<string[]>([]);
  protected readonly title = kindTitle;
  protected readonly metric = formatMetric;
  protected readonly pct = (v: number) => formatPercent(v, 0);
  protected readonly day = formatDayTime;
  protected readonly note = statusNote;
  protected readonly word = (d: MetricDelta) => verdictWord(d.verdict);
  protected readonly latest = computed(() => this.store.overview()?.runs[0] ?? null);
  protected readonly best = computed(() => bestValues(this.store.overview()?.runs ?? []));
  protected readonly hasBefore = computed(() => {
    const g = this.change().trim();
    return !!g && (this.store.overview()?.runs ?? []).some((r) => r.group === g && r.phase === 'before');
  });

  ngOnInit(): void {
    void this.store.load();
  }

  ngOnDestroy(): void {
    this.store.stop();
  }

  protected toggle(k: BenchKind): void {
    const next = new Set(this.chosen());
    if (!next.delete(k)) next.add(k);
    this.chosen.set(next);
  }

  protected run(phase?: 'before' | 'after'): void {
    const kinds = KINDS.map((k) => k.kind).filter((k) => this.chosen().has(k));
    const group = this.change().trim();
    const label = phase ? `${group} (${phase})` : this.label().trim() || 'Benchmark';
    void this.store.start({ kinds, label, group: phase ? group : null, phase: phase ?? null });
  }

  protected pick(id: string): void {
    const now = this.picked();
    this.picked.set(now.includes(id) ? now.filter((x) => x !== id) : [...now, id].slice(-2));
  }

  protected compare(): void {
    const runs = this.store.overview()?.runs ?? [];
    const [a, b] = this.picked()
      .map((id) => runs.find((r) => r.id === id))
      .filter((r): r is BenchRun => !!r)
      .sort((x, y) => x.startedAt - y.startedAt);
    if (a && b) void this.store.compare(a.id, b.id);
  }

  protected metrics(s: BenchStatus): BenchMetric[] | null {
    return s.state === 'done' ? s.metrics : null;
  }

  protected bar(m: BenchMetric): number {
    return barFraction(m, this.best()) * 100;
  }

  protected seconds(ms: number): string {
    return `${(ms / 1000).toFixed(1)} s`;
  }

  protected kindsOf(r: BenchRun): string {
    return r.results.filter((x) => x.status.state === 'done').map((x) => kindTitle(x.kind)).join(', ') || 'Nothing finished';
  }

  protected tone(d: MetricDelta): string {
    return d.verdict === 'better' ? 'ok' : d.verdict === 'worse' ? 'bad' : 'info';
  }

  protected signed(d: MetricDelta): string {
    return `${d.changePercent > 0 ? '+' : ''}${d.changePercent.toFixed(1)} %`;
  }
}
