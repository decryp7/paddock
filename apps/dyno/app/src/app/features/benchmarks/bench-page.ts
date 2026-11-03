import { PlatformService } from '../../core/platform';
import { osTerms } from '../../shared/os-terms';
import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, inject, signal } from '@angular/core';
import { PdCell, PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { BenchKind, BenchMetric, BenchRun, BenchStatus, MetricDelta } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatDayTime, formatMetric, formatPercent } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { BenchStore } from '../../state/bench.store';
import { DeviceArt } from '../inventory/device-art';
import { BenchArt } from './bench-art';
import { barPair, bestValues, kindTitle, livenessNote, methodNote, phaseText, spreadText, statusNote, verdictWord } from './bench-model';
import { MatButton } from '@angular/material/button';
import { MatFormField } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import { MatCheckbox } from '@angular/material/checkbox';
import { MatTooltip } from '@angular/material/tooltip';

const KINDS: { kind: BenchKind; text: string }[] = [
  { kind: 'cpu', text: 'Integer work on one core and on all of them' },
  { kind: 'memory', text: 'Copy bandwidth and random-access latency' },
  { kind: 'disk', text: 'A capped scratch file, removed afterwards' },
  { kind: 'gpu', text: 'Optional; not included in this build' },
];

/** Benchmarks: started only by a click, each in its own process with a timeout; the load at the time is stored with the result. */
@Component({
  selector: 'dy-bench-page',
  imports: [AsyncState, BenchArt, DeviceArt, Icon, PdTable, PdCell, MatTooltip, MatCheckbox, MatFormField, MatInput, MatButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Benchmarks</h1>
          <p>Measure the CPU, memory and disk on request, and compare before and after a change. Nothing runs until you click, and it all stays on this computer.</p>
        </div>
      </header>

      <dy-async [loaded]="!!store.overview()" [error]="store.error()" what="benchmarks" [rows]="4" (retry)="store.load()">
        @if (store.overview(); as o) {
          <section class="card" aria-label="Run benchmarks">
            <div class="card-head">
              <h2><dy-icon name="gauge" /> Run</h2>
              <span class="spacer"></span>
              @if (o.running) {
                <button type="button" [matButton]="o.running.liveness === 'notResponding' ? 'filled' : 'outlined'" (click)="store.cancel()"><dy-icon name="stop" /> Cancel</button>
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
              @if (r.phase; as ph) {
                <p class="phase" aria-live="polite">
                  <span>{{ r.current ? title(r.current) + ' · ' : '' }}{{ ph.label }} · {{ phaseWhere(ph) }}</span>
                  <progress class="bar-live" [value]="ph.rep" [max]="ph.of" [attr.aria-label]="ph.label + ', ' + phaseWhere(ph)"></progress>
                </p>
              }
              @if (liveNote(r.liveness); as ln) { <p class="note" [attr.data-tone]="r.liveness === 'notResponding' ? 'bad' : 'warn'" role="status"><dy-icon name="info" /> {{ ln }}</p> }
              @if (r.limited) { <p class="note"><dy-icon name="info" /> {{ r.limited }}</p> }
              <p class="note">Keep the computer otherwise idle while this runs; any other work lowers the numbers.</p>
            } @else {
              <div class="kinds" role="group" aria-label="What to measure">
                @for (k of kinds; track k.kind) {
                  <mat-checkbox class="kind" [attr.data-on]="chosen().has(k.kind)" [checked]="chosen().has(k.kind)" [disabled]="k.kind === 'disk' && !o.diskTestEnabled" (change)="toggle(k.kind)">
                    <span class="kbody"><dy-icon [name]="icons[k.kind]" /><span class="kt"><strong>{{ title(k.kind) }}</strong><span class="sub">{{ k.kind === 'disk' && !o.diskTestEnabled ? 'Off for now: being rebuilt to measure the drive, not the memory cache' : k.text }}</span></span></span>
                  </mat-checkbox>
                }
              </div>
              @if (store.consent(); as c) {
                <div class="consent" role="alert">
                  <p><dy-icon name="info" /> {{ c.message }}</p>
                  <div class="consent-actions">
                    <button type="button" matButton="filled" (click)="store.allowBattery()">Run on battery</button>
                    <button type="button" matButton="outlined" (click)="store.declineBattery()">Don't run</button>
                  </div>
                </div>
              }
              <div class="runbar">
                <div class="field grow">
                  <label for="bench-label">Name this run</label>
                  <mat-form-field><input id="bench-label" matInput type="text" maxlength="80" placeholder="Baseline" [value]="label()" (input)="label.set($any($event.target).value)" /></mat-form-field>
                </div>
                <button type="button" matButton="filled" [disabled]="store.busy() || chosen().size === 0" (click)="run()"><dy-icon name="play" /> Run benchmarks</button>
              </div>
              <div class="pair">
                <div class="field grow">
                  <label for="bench-change">Before and after a change</label>
                  <mat-form-field><input id="bench-change" matInput type="text" maxlength="80" [placeholder]="'What you are about to change, e.g. ' + terms.powerExample" [value]="change()" (input)="change.set($any($event.target).value)" /></mat-form-field>
                </div>
                <ol class="stepper" aria-label="Before and after steps">
                  <li><button type="button" [matButton]="!!change().trim() && !hasBefore() ? 'filled' : 'outlined'" [disabled]="store.busy() || chosen().size === 0 || !change().trim()" (click)="run('before')">1 · Measure before</button></li>
                  <li class="mid">2 · Make the change</li>
                  <li><button type="button" [matButton]="hasBefore() ? 'filled' : 'outlined'" [disabled]="store.busy() || chosen().size === 0 || !change().trim() || !hasBefore()" (click)="run('after')">3 · Measure after</button></li>
                </ol>
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
              @if (oldWay(run); as note) { <p class="load"><span class="badge">Old method</span> {{ note }}</p> }
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
                              <span class="label">{{ m.label }}@if (spread(m); as sp) { <small class="spread num">{{ sp }}</small> }@if (m.unsteady) { <span class="badge" data-tone="warn">Unsteady</span> }</span>
                              <span class="value num">{{ metric(m.value) }} <small>{{ m.unit }}</small></span>
                              <span class="bar" role="presentation"><i [style.width.%]="pair(m).fill * 100"></i>@if (pair(m).marker !== null) { <b class="mk" [style.left.%]="pair(m).marker! * 100" matTooltip="Best earlier run"></b> }</span>
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
                <button type="button" matButton="text" (click)="store.clearComparison()">Close</button>
              </div>
              @for (w of c.warnings; track w) {
                <p class="warning"><dy-icon name="alert" /> {{ w }}</p>
              }
              @if (c.deltas.length === 0) {
                <p class="note">The two runs share no finished measurements.</p>
              } @else {
                <div class="tablewrap">
                  <pd-table [toolbar]="false" label="Before and after" [rows]="c.deltas" [columns]="deltaColumns" [rowKey]="deltaKey" [filterable]="false" [maxHeight]="420">
                    <ng-template pdCell="label" let-d>{{ d.label }}{{ ' ' }}<span class="faint">({{ title(d.kind) }})</span></ng-template>
                    <ng-template pdCell="before" let-d><span class="num">{{ metric(d.before) }} <small>{{ d.unit }}</small></span></ng-template>
                    <ng-template pdCell="after" let-d><span class="num">{{ metric(d.after) }} <small>{{ d.unit }}</small></span></ng-template>
                    <ng-template pdCell="verdict" let-d><span class="badge" [attr.data-tone]="tone(d)">{{ word(d) }}</span></ng-template>
                  </pd-table>
                </div>
              }
            </section>
          }

          <section class="card" aria-label="History">
            <div class="card-head">
              <h2><dy-icon name="calendar" /> History</h2>
              <span class="spacer"></span>
              <button type="button" matButton="outlined" [disabled]="picked().length !== 2" (click)="compare()"><dy-icon name="diff" /> Compare selected</button>
            </div>
            @if (o.runs.length === 0) {
              <p class="note">No runs yet. Choose what to measure above and click Run benchmarks.</p>
            } @else {
              <div class="tablewrap">
                <pd-table [toolbar]="false" label="Saved runs" [rows]="o.runs" [columns]="runColumns" [rowKey]="runKey" [filterable]="false" [maxHeight]="420">
                  <ng-template pdCell="pick" let-r><mat-checkbox [checked]="picked().includes(r.id)" [aria-label]="'Select ' + r.label" (change)="pick(r.id)" /></ng-template>
                  <ng-template pdCell="run" let-r><span class="runcell"><span class="slot">@if (r.phase) { <span class="badge outline">{{ r.phase === 'before' ? 'Before' : 'After' }}</span> }</span><span class="ell" [matTooltip]="r.label">{{ r.label }}</span></span></ng-template>
                  <ng-template pdCell="actions" let-r><button type="button" matButton="text" [attr.aria-label]="'Delete ' + r.label" (click)="store.remove(r.id)"><dy-icon name="trash" /></button></ng-template>
                </pd-table>
              </div>
            }
          </section>
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    dy-async { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-4); }
    .page { display: grid; grid-template-columns: minmax(0, 1fr); gap: var(--sp-4); align-content: start; }
    .kinds { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 230px), 1fr)); gap: var(--sp-2); padding: var(--sp-3) var(--sp-4) 0; }
    .kind { position: relative; display: flex; align-items: center; gap: var(--sp-3); padding: 0 12px; border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-inset); }
    .kind[data-on='true'] { border-color: var(--border-strong); background: var(--surface-2); }
    .kind dy-icon { --icon-size: 20px; color: var(--text-3); flex: none; }
    .kind[data-on='true'] dy-icon { color: var(--text-1); }
    .kbody { display: inline-flex; align-items: center; gap: var(--sp-3); }
    .kt { display: grid; min-width: 0; }
    .stepper { list-style: none; margin: 0; padding: 0; display: flex; gap: var(--sp-2); align-items: center; flex-wrap: wrap; }
    .stepper .mid { color: var(--text-3); font-size: var(--fs-sm); padding: 0 var(--sp-1); }
    .bar { position: relative; }
    .bar .mk { position: absolute; top: -2px; width: 2px; height: 9px; background: var(--text-3); border-radius: 1px; }
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
    .results { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 420px), 1fr)); gap: var(--sp-3); padding: var(--sp-2) var(--sp-4) 0; }
    .result { display: flex; gap: var(--sp-4); padding: var(--sp-3) var(--sp-4); border-radius: var(--r-lg); background: var(--surface-inset); min-width: 0; align-items: flex-start; }
    .result .art { flex: none; width: 96px; display: grid; place-items: center; padding-top: var(--sp-1); }
    .result .art dy-device-art { width: 72px; }
    .result .art { width: 120px; }
    .result .body { min-width: 0; flex: 1; }
    h3 { margin: 0 0 var(--sp-2); font: var(--fw-semibold) var(--fs-md) / 1.3 var(--font-sans); }
    .metrics { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-2); }
    .metrics li { display: grid; grid-template-columns: 1fr auto; gap: 2px var(--sp-3); }
    .metrics .label { color: var(--text-2); font-size: var(--fs-sm); }
    .metrics .spread { margin-left: var(--sp-2); color: var(--text-3); font-size: var(--fs-xs); }
    .metrics .value { font: var(--fw-semibold) var(--fs-lg) / 1.2 var(--font-display); font-variant-numeric: tabular-nums; text-align: right; }
    .metrics .value small { font: var(--fw-regular) var(--fs-xs) / 1 var(--font-sans); color: var(--text-3); }
    .bar { grid-column: 1 / -1; height: 5px; border-radius: 3px; background: var(--surface-active); }
    .bar i { display: block; height: 100%; background: var(--accent); border-radius: 3px; min-width: 2px; }
    .phase { display: grid; gap: var(--sp-1); margin: var(--sp-3) 0 0; color: var(--text-2); font-size: var(--fs-sm); }
    .bar-live { width: 100%; height: 6px; accent-color: var(--accent); }
    .consent { margin: var(--sp-3) 0; padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); display: grid; gap: var(--sp-2); }
    .consent p { margin: 0; }
    .consent-actions { display: flex; gap: var(--sp-2); }
    .took { display: block; margin-top: var(--sp-2); color: var(--text-3); font-size: var(--fs-xs); }
    .why { margin: 0; display: flex; gap: var(--sp-2); color: var(--text-2); font-size: var(--fs-sm); }
    .tablewrap { margin: 0 var(--sp-4) var(--sp-3); }
    .num { font-variant-numeric: tabular-nums; }
    .faint { color: var(--text-3); }
    small { color: var(--text-3); }
    /* The Before/After badge has a slot of its own before the label, so a long label ellipsises instead of pushing it out of the cell. */
    .runcell { display: flex; align-items: center; gap: var(--sp-2); min-width: 0; }
    .slot { flex: none; width: 58px; }
    .ell { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  `,
})
export class BenchPage implements OnInit, OnDestroy {
  protected readonly terms = osTerms(inject(PlatformService).platform);
  protected readonly store = inject(BenchStore);
  protected readonly kinds = KINDS;
  protected readonly chosen = signal<ReadonlySet<BenchKind>>(new Set<BenchKind>(['cpu', 'memory']));
  protected readonly label = signal('');
  protected readonly change = signal('');
  protected readonly picked = signal<string[]>([]);
  protected readonly title = kindTitle;
  protected readonly metric = formatMetric;
  protected readonly pct = (v: number) => formatPercent(v, 0);
  protected readonly day = formatDayTime;
  protected readonly note = statusNote;
  protected readonly spread = (m: BenchMetric): string => spreadText(m, formatMetric);
  protected readonly oldWay = methodNote;
  protected readonly phaseWhere = phaseText;
  protected readonly liveNote = livenessNote;
  protected readonly deltaKey = (d: MetricDelta): string => d.kind + d.id;
  protected readonly runKey = (r: BenchRun): string => r.id;
  protected readonly deltaColumns: PdColumn<MetricDelta>[] = [
    { key: 'label', header: 'Measurement', value: (d) => d.label, text: (d) => `${d.label} (${kindTitle(d.kind)})`, required: true, sortable: false, width: 'minmax(0, 2fr)' },
    { key: 'before', header: 'Before', value: (d) => d.before, align: 'end', sortable: false, width: '110px' },
    { key: 'after', header: 'After', value: (d) => d.after, align: 'end', sortable: false, width: '110px' },
    { key: 'change', header: 'Change', value: (d) => d.changePercent, text: (d) => this.signed(d), align: 'end', sortable: false, width: '100px' },
    { key: 'verdict', header: 'Verdict', value: (d) => verdictWord(d.verdict), sortable: false, width: '120px' },
  ];
  protected readonly runColumns: PdColumn<BenchRun>[] = [
    { key: 'pick', header: 'Select', value: () => '', headerHidden: true, sortable: false, width: '28px' },
    { key: 'run', header: 'Run', value: (r) => r.label, required: true, sortable: false, width: 'minmax(0, 2fr)' },
    { key: 'when', header: 'When', value: (r) => r.startedAt, text: (r) => formatDayTime(r.startedAt), sortable: false, width: '150px' },
    { key: 'load', header: 'CPU load', value: (r) => r.load.cpuPercent, text: (r) => formatPercent(r.load.cpuPercent, 0), align: 'end', sortable: false, width: '90px' },
    { key: 'kinds', header: 'Measured', value: (r) => this.kindsOf(r), sortable: false, width: 'minmax(0, 1fr)', hideBelow: 640 },
    { key: 'actions', header: 'Actions', value: () => '', headerHidden: true, sortable: false, width: '44px' },
  ];
  protected readonly word = (d: MetricDelta) => verdictWord(d.verdict);
  protected readonly latest = computed(() => this.store.overview()?.runs[0] ?? null);
  /** The best value of each metric over the runs before the latest one. */
  protected readonly others = computed(() => bestValues((this.store.overview()?.runs ?? []).slice(1)));
  protected readonly icons: Record<BenchKind, 'cpu' | 'memory' | 'disk' | 'gpu'> = { cpu: 'cpu', memory: 'memory', disk: 'disk', gpu: 'gpu' };
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

  protected pair(m: BenchMetric): { fill: number; marker: number | null } {
    return barPair(m, this.others().get(m.id));
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
