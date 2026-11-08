import { PlatformService } from '../../core/platform';
import { osTerms } from '../../shared/os-terms';
import { ChangeDetectionStrategy, Component, OnDestroy, OnInit, computed, effect, inject, signal } from '@angular/core';
import { PdCell, PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { BenchEstimates, BenchKind, BenchKindResult, BenchMetric, BenchProgress, BenchRun, BenchStatus, MetricDelta, Volume } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatDayTime, formatMetric, formatPercent } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { BenchStore } from '../../state/bench.store';
import { DeviceArt } from '../inventory/device-art';
import { BenchArt } from './bench-art';
import { SustainedChart, SweepChart } from './bench-charts';
import {
  bestValues,
  clock,
  driveBlock,
  nextStep,
  noticeKind,
  rangeBar,
  runBlock,
  singleSample,
  systemVolume,
  writeConsent,
  type NoticeKind,
  devicesOf,
  cachedReadsIn,
  comparable,
  estimateLine,
  estimateRequest,
  unitWord,
  kindTitle,
  livenessNote,
  methodNote,
  methodsOf,
  phaseText,
  presetKinds,
  PRESETS,
  resultTitle,
  spreadText,
  statusNote,
  pastNote,
  featuredRun,
  verdictWord,
  type BenchPreset,
  type PickedPlan,
} from './bench-model';
import { MatButtonToggle, MatButtonToggleGroup } from '@angular/material/button-toggle';
import { MatButton } from '@angular/material/button';
import { MatFormField } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import { MatCheckbox } from '@angular/material/checkbox';
import { MatTooltip } from '@angular/material/tooltip';

const KINDS: { kind: BenchKind; text: string }[] = [
  { kind: 'cpu', text: 'Integer, sorting, float and vector work on one core and on all of them' },
  { kind: 'memory', text: 'Bandwidth (STREAM-style) and latency from the caches to main memory' },
  { kind: 'disk', text: 'Reads and writes on a scratch file on each chosen drive, removed afterwards' },
  { kind: 'gpu', text: 'Compute, memory copy, fill rate and geometry on the graphics adapter' },
];

/** Benchmarks: started only by a click, each in its own process with a timeout; the load at the time is stored with the result. */
@Component({
  selector: 'dy-bench-page',
  imports: [AsyncState, BenchArt, DeviceArt, Icon, PdTable, PdCell, MatTooltip, MatCheckbox, MatFormField, MatInput, MatButton, MatButtonToggle, MatButtonToggleGroup, SustainedChart, SweepChart],
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
                    <span class="st">{{ stepWord(r, k) }}</span>
                  </li>
                }
              </ol>
              @if (r.phase; as ph) {
                <p class="phase" aria-live="polite">
                  <span>{{ r.current ? title(r.current) + ' · ' : '' }}{{ ph.label }} · {{ phaseWhere(ph) }}</span>
                  <progress class="bar-live" [value]="ph.rep" [max]="ph.of" [attr.aria-label]="ph.label + ', ' + phaseWhere(ph)"></progress>
                </p>
              }
              @if (r.trace?.length) { <div class="live-chart"><dy-sustained-chart [trace]="r.trace!" /></div> }
              @if (liveNote(r.liveness); as ln) { <p class="note" [attr.data-tone]="r.liveness === 'notResponding' ? 'bad' : 'warn'" role="status"><dy-icon name="alert" /> <span>{{ ln }}</span></p> }
              @if (r.limited) { <p class="note" role="status"><dy-icon name="alert" /> <span>{{ r.limited }}</span></p> }
              <p class="note">Keep the computer otherwise idle while this runs; any other work lowers the numbers.</p>
            } @else {
              <mat-button-toggle-group class="presets" [value]="preset()" (change)="setPreset($event.value)" aria-label="What to run" hideSingleSelectionIndicator>
                @for (p of presets; track p.id) { <mat-button-toggle [value]="p.id">{{ p.title }}</mat-button-toggle> }
              </mat-button-toggle-group>
              <p class="note preset-text">{{ presetText() }}</p>
              @if (preset() === 'custom') {
                <div class="kinds" role="group" aria-label="What to measure">
                  @for (k of kinds; track k.kind) {
                    <mat-checkbox class="kind" [attr.data-on]="own().has(k.kind)" [checked]="own().has(k.kind)" [disabled]="k.kind === 'disk' && !o.diskTestEnabled" (change)="toggle(k.kind)">
                      <span class="kbody"><dy-icon [name]="icons[k.kind]" /><span class="kt"><strong>{{ title(k.kind) }}</strong><span class="sub">{{ k.kind === 'disk' && !o.diskTestEnabled ? 'Off for now' : k.text }}</span></span></span>
                    </mat-checkbox>
                  }
                </div>
              }
              @if (showDrives()) {
                <fieldset class="drives">
                  <legend>Drives to test</legend>
                  @if (store.volumes().length === 0) {
                    <p class="note">Dyno could not list the drives, so it tests the system drive.</p>
                  } @else {
                    @for (v of store.volumes(); track v.id) {
                      <mat-checkbox [checked]="drives().includes(v.id)" [disabled]="!!block(v)" (change)="toggleDrive(v.id)">
                        <span class="drive"><strong>{{ v.name }}</strong><span class="sub">{{ driveFacts(v) }}@if (block(v); as why) { · {{ why }} }</span></span>
                      </mat-checkbox>
                    }
                    <mat-checkbox [checked]="allowNetwork()" (change)="toggleNetwork()">Include network drives (the results are marked network)</mat-checkbox>
                  }
                  @if (preset() === 'custom') {
                    <div class="sizes" role="group" aria-label="Test file size">
                      <span class="sub">Test file</span>
                      <mat-button-toggle-group [value]="diskMib()" (change)="diskMib.set($event.value)" aria-label="Test file size" hideSingleSelectionIndicator>
                        @for (d of o.estimates?.disk ?? []; track d.mib) { <mat-button-toggle [value]="d.mib">{{ sizeWords(d.mib) }}</mat-button-toggle> }
                      </mat-button-toggle-group>
                    </div>
                  }
                  @if (o.estimates?.sustainedWriteMaxGib; as maxGib) {
                    <mat-checkbox [checked]="writeAsked()" (change)="writeAsked.set(!writeAsked()); writeConfirmed.set(false)">Also test sustained write (past the drive's fast cache)</mat-checkbox>
                    @if (writeAsked()) {
                      <div class="consent" role="group" aria-label="Sustained write">
                        <p><dy-icon name="alert" /> {{ consentText(o.estimates) }} At most {{ maxGib }} GB, and never more than a quarter of the free space.</p>
                        <mat-button-toggle-group [value]="writeGib()" (change)="writeGib.set($event.value); writeConfirmed.set(false)" aria-label="How much to write" hideSingleSelectionIndicator>
                          @for (g of writeChoices(maxGib); track g) { <mat-button-toggle [value]="g">{{ g }} GB</mat-button-toggle> }
                        </mat-button-toggle-group>
                        <mat-checkbox [checked]="writeConfirmed()" (change)="writeConfirmed.set(!writeConfirmed())">Yes, write {{ writeGib() }} GB to {{ drives().length > 1 ? 'each chosen drive' : 'the chosen drive' }}</mat-checkbox>
                      </div>
                    }
                  }
                </fieldset>
              }
              @if (chosen().has('gpu') && preset() !== 'quick' && store.adapters().length > 1) {
                <div class="sizes padded" role="group" aria-label="Graphics adapter">
                  <span class="sub">Graphics adapter</span>
                  <mat-button-toggle-group [value]="gpuAdapter()" (change)="gpuAdapter.set($event.value)" aria-label="Graphics adapter" hideSingleSelectionIndicator>
                    <mat-button-toggle [value]="null">Default</mat-button-toggle>
                    @for (a of store.adapters(); track a.index) { <mat-button-toggle [value]="a.index">{{ a.name }} ({{ a.kind === 'discrete' ? 'discrete' : a.kind === 'integrated' ? 'integrated' : a.kind === 'cpu' ? 'software' : a.kind }})</mat-button-toggle> }
                  </mat-button-toggle-group>
                </div>
              }
              @if (showSustained()) {
                <div class="sizes padded" role="group" aria-label="Length of the sustained run">
                  <span class="sub">Length</span>
                  <mat-button-toggle-group [value]="minutes()" (change)="minutes.set($event.value)" aria-label="Length of the sustained run" hideSingleSelectionIndicator>
                    @for (m of o.estimates?.sustainedMinutes ?? []; track m) { <mat-button-toggle [value]="m">{{ m }} min</mat-button-toggle> }
                  </mat-button-toggle-group>
                </div>
                <p class="note">Where Dyno cannot read this computer's temperature it limits the run to {{ o.estimates?.noSensorMaxMinutes ?? 5 }} minutes and says so; the hardware protects itself. The run stops by itself if the processor stays close to its critical temperature.</p>
              }
              @if (estimateText(); as e) {
                <p class="estimate" aria-live="polite">
                  <dy-icon name="info" /> {{ e }}
                </p>
              }
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
                <button type="button" [matButton]="store.consent() ? 'outlined' : 'filled'" [disabled]="store.busy() || !!blocked() || !!store.consent()" (click)="run()"><dy-icon name="play" /> Run benchmarks</button>
              </div>
              @if (blocked(); as why) { <p class="why-disabled" role="status"><dy-icon name="info" /> {{ why }}</p> }
              <div class="pair">
                <div class="field grow">
                  <label for="bench-change">Before and after a change</label>
                  <mat-form-field><input id="bench-change" matInput type="text" maxlength="80" [placeholder]="'What you are about to change, e.g. ' + terms.powerExample" [value]="change()" (input)="change.set($any($event.target).value)" /></mat-form-field>
                </div>
                <ol class="stepper" aria-label="Before and after steps">
                  <li><button type="button" [matButton]="!!change().trim() && !hasBefore() ? 'filled' : 'outlined'" [disabled]="store.busy() || !!blocked() || !!store.consent() || !change().trim()" (click)="run('before')">1 · Measure before</button></li>
                  <li class="mid">2 · Make the change</li>
                  <li><button type="button" [matButton]="hasBefore() ? 'filled' : 'outlined'" [disabled]="store.busy() || !!blocked() || !!store.consent() || !change().trim() || !hasBefore()" (click)="run('after')">3 · Measure after</button></li>
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
              @if (store.busy()) { <p class="load" role="status">This is your previous run; the new one is still measuring.</p> }
              @if (oldWay(run); as note) { <p class="load"><span class="badge">Old method</span> {{ note }}</p> }
              <p class="load" aria-label="Machine load when the run started">
                Machine at the start: <span class="num">{{ pct(run.load.cpuPercent) }}</span> CPU, <span class="num">{{ pct(run.load.memoryPercent) }}</span> memory{{ run.load.onBattery === true ? ', on battery' : run.load.onBattery === false ? ', on mains power' : '' }}
                @if (run.load.cpuPercent > 20) { <span class="badge" data-tone="warn">Busy machine: numbers may be low</span> }
              </p>
              @if (showCompared() && previous(); as prev) { <p class="load">Compared with your last run, <strong>{{ prev.label }}</strong> ({{ day(prev.startedAt) }}); a change counts only when it is larger than both runs' spreads.</p> }
              <div class="results">
                @for (r of run.results; track $index) {
                  <article class="result" [attr.data-state]="r.status.state">
                    <div class="art" aria-hidden="true">
                      @switch (r.kind) {
                        @case ('cpu') { <dy-device-art kind="cpu" /> }
                        @case ('disk') { <dy-device-art kind="nvme" [totalBytes]="0" /> }
                        @default { <dy-bench-art [kind]="r.kind === 'gpu' ? 'gpu' : 'memory'" /> }
                      }
                    </div>
                    <div class="body">
                      <h3>{{ resultName(r) }}</h3>
                      @if (metrics(r.status); as ms) {
                        <ul class="metrics">
                          @for (m of shown(ms); track m.id) {
                            <li>
                              <span class="label">{{ m.label }}@if (spread(m); as sp) { <small class="spread num">{{ sp }}</small> }@if (m.unsteady) { <span class="badge" data-tone="warn" matTooltip="The repetitions varied by more than 10 %">Unsteady</span> }@if (cachedIn(r, m)) { <span class="badge" data-tone="warn" matTooltip="These reads may have come from memory, not from the drive">Cached reads possible</span> }@if (single(m)) { <span class="badge outline" matTooltip="Measured once, so there is no spread to show">Single sample</span> }</span>
                              <span class="value num">{{ metric(m.value) }} <small>{{ unit(m) }}</small></span>
                              @if (range(m); as rb) {
                                <span class="bar" role="img" [attr.aria-label]="'Range ' + metric(m.min) + ' to ' + metric(m.max) + ' ' + m.unit + ' on a scale from 0 to ' + metric(rb.scale)"><i [style.left.%]="rb.lo * 100" [style.width.%]="(rb.hi - rb.lo) * 100"></i><b class="mid" [style.left.%]="rb.mid * 100"></b></span>
                              }
                              @if (vs(r.kind, m); as d) { <span class="vs"><span class="badge" [attr.data-tone]="tone(d)">{{ word(d) }}</span> {{ signed(d) }} compared with your last run</span> }
                            </li>
                          }
                        </ul>
                        @if (r.kind === 'memory') { <dy-sweep-chart [metrics]="ms" /> }
                        @if (r.trace?.length) { <dy-sustained-chart [trace]="r.trace!" /> }
                        <span class="took">Took {{ took(r) }}</span>
                      } @else {
                        <p class="why" role="status" [attr.data-kind]="kindOf(r.status)"><dy-icon [name]="icon(r.status)" /> <span><strong>{{ word2(r.status) }}</strong> {{ past(r.status, day(run.startedAt)) }} {{ next(r.status) }}</span></p>
                        @if (r.status.state === 'failed') { <button type="button" matButton="outlined" (click)="store.retryKind(r.kind)"><dy-icon name="refresh" /> Retry</button> }
                        @if (r.trace?.length) { <dy-sustained-chart [trace]="r.trace!" /> <p class="note">The points measured before it stopped are kept.</p> }
                        @if (r.needsFolder && r.device) {
                          <button type="button" matButton="outlined" (click)="store.runWithFolder(volumeOf(r.device))"><dy-icon name="folder" /> Choose a folder on {{ r.device }} and run again</button>
                        }
                      }
                      @if (r.details?.length) {
                        <details class="details"><summary>How it was measured</summary><ul>@for (d of r.details!; track d) { <li>{{ d }}</li> }</ul></details>
                      }
                    </div>
                  </article>
                }
              </div>
              <p class="legend">Each bar shows the range of that number's repetitions, from the lowest to the highest, on a scale from 0 to the larger of the highest repetition and your best earlier run; the dark mark is the median.</p>
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
                  <pd-table [toolbar]="false" label="Before and after" [rows]="c.deltas" [columns]="deltaColumns" [rowKey]="deltaKey" [filterable]="false" height="window">
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
              @if (picked().length !== 2) { <span class="sub">Select two runs to compare.</span> }
              <button type="button" matButton="outlined" [disabled]="o.runs.length === 0" (click)="store.exportRuns(exportIds(), 'csv')" matTooltip="Save the selected runs, or all of them, as a spreadsheet file"><dy-icon name="file" /> Export CSV</button>
              <button type="button" matButton="outlined" [disabled]="o.runs.length === 0" (click)="store.exportRuns(exportIds(), 'json')"><dy-icon name="file" /> Export JSON</button>
              <button type="button" matButton="outlined" [disabled]="picked().length !== 2" (click)="compare()"><dy-icon name="diff" /> Compare selected</button>
            </div>
            @if (store.exported(); as e) { <p class="note" role="status"><dy-icon name="check" /> Saved {{ e.bytes.toLocaleString('en-US') }} bytes to {{ e.path }}</p> }
            @if (o.runs.length === 0) {
              <p class="note">No runs yet. Choose what to measure above and click Run benchmarks.</p>
            } @else {
              <div class="tablewrap">
                <pd-table [toolbar]="false" label="Saved runs" [rows]="o.runs" [columns]="runColumns" [rowKey]="runKey" [filterable]="false" height="window">
                  <ng-template pdCell="pick" let-r><mat-checkbox [checked]="picked().includes(r.id)" [aria-label]="'Select ' + r.label" (change)="pick(r.id)" /></ng-template>
                  <ng-template pdCell="run" let-r><span class="runcell"><span class="slot">@if (r.phase) { <span class="badge outline">{{ r.phase === 'before' ? 'Before' : 'After' }}</span> }</span><span class="ell" [matTooltip]="r.label">{{ r.label }}</span></span></ng-template>
                  <ng-template pdCell="kinds" let-r><span class="wrap" [matTooltip]="kindsOf(r)">{{ kindsOf(r) }}</span></ng-template>
                  <ng-template pdCell="devices" let-r><span class="wrap" [matTooltip]="devices(r)">{{ devices(r) }}</span></ng-template>
                  <ng-template pdCell="method" let-r><span class="wrap" [matTooltip]="methods(r)">{{ methods(r) }}</span></ng-template>
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
    .metrics .spread { white-space: nowrap; margin-left: var(--sp-2); color: var(--text-3); font-size: var(--fs-xs); }
    .metrics .value { font: var(--fw-semibold) var(--fs-lg) / 1.2 var(--font-display); font-variant-numeric: tabular-nums; text-align: right; }
    .metrics .value small { font: var(--fw-regular) var(--fs-xs) / 1 var(--font-sans); color: var(--text-3); }
    .bar { position: relative; grid-column: 1 / -1; height: 7px; border-radius: 3px; background: var(--surface-active); }
    .bar i { position: absolute; top: 0; bottom: 0; background: var(--accent); border-radius: 3px; min-width: 3px; opacity: 0.55; }
    .bar b.mid { position: absolute; top: -2px; bottom: -2px; width: 3px; margin-left: -1px; background: var(--text-1); border-radius: 1px; }
    .vs { grid-column: 1 / -1; color: var(--text-2); font-size: var(--fs-xs); }
    .legend { margin: 0; padding: var(--sp-2) var(--sp-4) var(--sp-3); color: var(--text-3); font-size: var(--fs-xs); }
    .why-disabled { margin: 0; padding: var(--sp-2) var(--sp-4) 0; display: flex; gap: var(--sp-2); align-items: center; color: var(--text-2); }
    .why[data-kind='safety'], .why[data-kind='failed'] { color: var(--warn); }
    .why strong { color: var(--text-1); }
    .note, .why { display: flex; gap: var(--sp-2); align-items: flex-start; }
    .note dy-icon, .why dy-icon, .estimate dy-icon, .why-disabled dy-icon { flex: none; margin-top: 2px; }
    .metrics .label .badge { margin-left: var(--sp-2); }
    .wrap { white-space: normal; overflow-wrap: anywhere; }
    .phase { display: grid; gap: var(--sp-1); margin: var(--sp-3) 0 0; padding: 0 var(--sp-4); color: var(--text-2); font-size: var(--fs-sm); }
    .bar-live { width: 100%; height: 6px; accent-color: var(--accent); border: 0; border-radius: 3px; background: var(--surface-active); }
    .bar-live::-webkit-progress-bar { background: var(--surface-active); border-radius: 3px; }
    .bar-live::-webkit-progress-value { background: var(--accent); border-radius: 3px; }
    .bar-live::-moz-progress-bar { background: var(--accent); border-radius: 3px; }
    .consent { margin: var(--sp-3) 0; padding: var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); display: grid; gap: var(--sp-2); }
    .consent p { margin: 0; }
    .consent-actions { display: flex; gap: var(--sp-2); }
    .presets { margin: var(--sp-3) var(--sp-4) 0; flex-wrap: wrap; }
    .preset-text { padding-top: var(--sp-2); }
    .drives { margin: var(--sp-3) var(--sp-4) 0; padding: var(--sp-2) var(--sp-3); border: 1px solid var(--border-subtle); border-radius: var(--r-md); display: grid; gap: var(--sp-1); }
    .drives legend { padding: 0 var(--sp-1); color: var(--text-2); font-size: var(--fs-sm); }
    .drive { display: inline-grid; }
    .sizes { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; padding-top: var(--sp-2); }
    .sizes.padded { padding: var(--sp-3) var(--sp-4) 0; }
    .estimate { margin: 0; padding: var(--sp-2) var(--sp-4) 0; display: flex; gap: var(--sp-2); align-items: center; color: var(--text-1); }
    .live-chart { padding: var(--sp-2) var(--sp-4) 0; }
    .details { margin-top: var(--sp-2); color: var(--text-2); font-size: var(--fs-sm); }
    .details summary { cursor: pointer; }
    .details ul { margin: var(--sp-1) 0 0; padding-left: var(--sp-4); }
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
  protected readonly presets = PRESETS;
  protected readonly preset = signal<BenchPreset>('quick');
  /** The kinds ticked under Custom. */
  protected readonly own = signal<ReadonlySet<BenchKind>>(new Set<BenchKind>(['cpu', 'memory']));
  protected readonly drives = signal<string[]>([]);
  protected readonly gpuAdapter = signal<number | null>(null);
  protected readonly single = singleSample;
  protected readonly unit = unitWord;
  protected readonly devices = devicesOf;
  protected readonly methods = methodsOf;
  protected readonly allowNetwork = signal(false);
  protected readonly diskMib = signal(1024);
  protected readonly minutes = signal(5);
  protected readonly writeAsked = signal(false);
  protected readonly writeConfirmed = signal(false);
  protected readonly writeGib = signal(8);
  /** The kinds the chosen preset runs. */
  protected readonly chosen = computed<ReadonlySet<BenchKind>>(() => new Set(presetKinds(this.preset(), [...this.own()])));
  protected readonly presetText = computed(() => PRESETS.find((p) => p.id === this.preset())?.text ?? '');
  protected readonly showDrives = computed(() => this.chosen().has('disk') && this.preset() !== 'quick');
  protected readonly showSustained = computed(() => this.preset() === 'sustained');
  /** Why Run is disabled, in words ('' when it is not). */
  protected readonly blocked = computed(() =>
    runBlock({
      kinds: [...this.chosen()],
      drives: this.preset() === 'quick' ? ['system'] : this.drives(),
      needsDrive: this.store.volumes().length > 0,
      writeAsked: this.writeAsked() && this.showDrives(),
      writeConfirmed: this.writeConfirmed(),
      writeGib: this.writeGib(),
    }),
  );
  protected readonly cachedIn = (r: BenchKindResult, m: BenchMetric): boolean => cachedReadsIn(r.details, m);
  protected readonly range = (m: BenchMetric) => rangeBar(m, this.others().get(m.id));
  /** Whether "compared with your last run" is shown: the engine found something comparable and neither run is from 1.0. */
  protected readonly showCompared = computed(() => comparable(this.latest(), this.previous(), this.store.lastComparison()?.deltas.length ?? 0));
  /** The run before the latest one, for "compared with your last run". */
  protected readonly previous = computed(() => this.runsAfterLatest()[0] ?? null);
  /** The runs older than the featured one. */
  private readonly runsAfterLatest = computed(() => {
    const runs = this.store.overview()?.runs ?? [];
    const at = runs.findIndex((x) => x.id === this.latest()?.id);
    return at < 0 ? [] : runs.slice(at + 1);
  });
  protected readonly label = signal('');
  protected readonly change = signal('');
  protected readonly picked = signal<string[]>([]);
  protected readonly title = kindTitle;
  protected readonly metric = formatMetric;
  protected readonly pct = (v: number) => formatPercent(v, 0);
  protected readonly day = formatDayTime;
  protected readonly note = statusNote;
  protected readonly past = pastNote;
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
    { key: 'devices', header: 'Device', value: (r) => devicesOf(r), sortable: false, width: 'minmax(0, 1fr)', hideBelow: 820 },
    { key: 'method', header: 'Method', value: (r) => methodsOf(r), sortable: false, width: '110px', hideBelow: 900 },
    { key: 'actions', header: 'Actions', value: () => '', headerHidden: true, sortable: false, width: '44px' },
  ];
  protected readonly word = (d: MetricDelta) => verdictWord(d.verdict);
  protected readonly latest = computed(() => featuredRun(this.store.overview()?.runs ?? []));
  /** The best value of each metric over the runs before the latest one. */
  protected readonly others = computed(() => bestValues(this.runsAfterLatest()));
  protected readonly icons: Record<BenchKind, 'cpu' | 'memory' | 'disk' | 'gpu'> = { cpu: 'cpu', memory: 'memory', disk: 'disk', gpu: 'gpu' };
  protected readonly hasBefore = computed(() => {
    const g = this.change().trim();
    return !!g && (this.store.overview()?.runs ?? []).some((r) => r.group === g && r.phase === 'before');
  });

  ngOnInit(): void {
    void this.store.load();
    void this.store.loadAdapters();
    // The system drive is ticked from the start, so a disk run never has to guess which drive is meant.
    void this.store.loadVolumes().then(() => {
      const sys = systemVolume(this.store.volumes());
      if (sys && this.drives().length === 0) this.drives.set([sys.id]);
    });
  }

  ngOnDestroy(): void {
    this.store.stop();
  }

  protected toggle(k: BenchKind): void {
    const next = new Set(this.own());
    if (!next.delete(k)) next.add(k);
    this.own.set(next);
  }

  protected setPreset(p: BenchPreset): void {
    this.preset.set(p);
  }

  protected block(v: Volume): string {
    return driveBlock(v, this.allowNetwork());
  }

  protected toggleNetwork(): void {
    const on = !this.allowNetwork();
    this.allowNetwork.set(on);
    if (!on) this.drives.set(this.drives().filter((id) => !this.block(this.store.volumes().find((v) => v.id === id) ?? ({ readOnly: false, reachable: true, location: 'internal' } as Volume))));
  }

  protected consentText(est: BenchEstimates | undefined): string {
    return writeConsent(est, this.writeGib(), this.drives().length || 1);
  }

  protected stepWord(r: BenchProgress, k: BenchKind): string {
    if (r.done.includes(k)) return 'Done';
    if (r.current !== k) return 'Waiting';
    return r.liveness === 'notResponding' ? 'Not responding' : r.liveness === 'stillWorking' ? 'Still working' : 'Measuring…';
  }

  protected took(r: BenchKindResult): string {
    // One clock for a duration: the sustained run's length is the number of points it has.
    return r.trace?.length ? clock(r.trace.length) : clock(r.millis / 1000);
  }

  protected vs(kind: BenchKind, m: BenchMetric): MetricDelta | null {
    const c = this.showCompared() ? this.store.lastComparison() : null;
    return c?.deltas.find((d) => d.kind === kind && d.id === m.id) ?? null;
  }

  protected kindOf(s: BenchStatus): NoticeKind | null {
    return noticeKind(s);
  }

  protected icon(s: BenchStatus): 'alert' | 'close' | 'undo' | 'temperature' | 'info' {
    switch (noticeKind(s)) {
      case 'safety':
        return 'temperature';
      case 'failed':
        return 'close';
      case 'cancelled':
        return 'undo';
      default:
        return 'alert';
    }
  }

  protected word2(s: BenchStatus): string {
    switch (noticeKind(s)) {
      case 'safety':
        return 'Stopped for safety.';
      case 'failed':
        return 'Failed.';
      case 'cancelled':
        return 'Cancelled.';
      default:
        return 'Not measured.';
    }
  }

  protected next(s: BenchStatus): string {
    return nextStep(noticeKind(s));
  }

  protected toggleDrive(id: string): void {
    const now = this.drives();
    this.drives.set(now.includes(id) ? now.filter((x) => x !== id) : [...now, id]);
  }

  protected driveFacts(v: Volume): string {
    const free = v.free === null ? 'free space unknown' : `${Math.round(v.free / 1e9).toLocaleString('en-US')} GB free`;
    const where = v.location === 'internal' ? 'internal' : 'external' in v.location ? 'external' : 'network';
    return `${v.fs} · ${where} · ${free}${v.readOnly ? ' · read-only' : ''}`;
  }

  protected sizeWords(mib: number): string {
    return mib >= 1024 ? `${mib / 1024} GB` : `${mib} MB`;
  }

  protected writeChoices(max: number): number[] {
    return [8, 16, 32, 64].filter((g) => g <= max);
  }

  /** The plan as picked, for the engine's estimates. */
  private planOf(): PickedPlan {
    const p = this.preset();
    return {
      preset: p,
      kinds: [...this.chosen()],
      drives: p === 'quick' ? [] : this.drives(),
      diskMib: p === 'quick' ? null : p === 'full' ? 1024 : this.diskMib(),
      sustainedMinutes: p === 'sustained' ? this.minutes() : null,
      // Asked for is enough for the estimate; Run stays disabled until the amount is confirmed.
      sustainedWriteGib: this.writeAsked() && this.showDrives() ? this.writeGib() : null,
      allowNetwork: this.allowNetwork(),
    };
  }

  /** "Takes about 4 minutes. This writes up to 16 GB to Macintosh HD.", from the engine's estimate of what is ticked. */
  protected readonly estimateText = computed(() => {
    const plan = this.planOf();
    const first = this.store.volumes().find((v) => v.id === plan.drives[0]) ?? systemVolume(this.store.volumes());
    const drive = plan.drives.length > 1 ? `each of the ${plan.drives.length} drives` : (first?.name ?? 'the system drive');
    return estimateLine(this.store.estimate(), plan.kinds.includes('disk'), drive);
  });

  /** Asks the engine again whenever what is chosen changes. */
  private readonly asking = effect(() => {
    void this.store.estimateFor(estimateRequest(this.planOf()));
  });

  protected resultName(r: BenchKindResult): string {
    return resultTitle(r);
  }

  /** The metrics a card lists: the latency sweep's points are drawn as a chart, not listed one by one. */
  protected shown(ms: BenchMetric[]): BenchMetric[] {
    return ms.filter((m) => !/^mem-latency-\d+[km]$/.test(m.id));
  }

  protected volumeOf(device: string): string {
    return this.store.volumes().find((v) => v.name === device)?.id ?? device;
  }

  protected run(phase?: 'before' | 'after'): void {
    const kinds = KINDS.map((k) => k.kind).filter((k) => this.chosen().has(k));
    const group = this.change().trim();
    const label = phase ? `${group} (${phase})` : this.label().trim() || 'Benchmark';
    const p = this.planOf();
    void this.store.start({
      kinds,
      label,
      group: phase ? group : null,
      phase: phase ?? null,
      diskMib: p.diskMib,
      diskVolumes: p.drives,
      allowNetworkDrives: p.allowNetwork,
      sustainedMinutes: p.sustainedMinutes,
      sustainedWriteGib: p.sustainedWriteGib,
      gpuAdapter: this.chosen().has('gpu') ? this.gpuAdapter() : null,
    });
  }

  /** The runs to export: the two ticked ones, else every saved run. */
  protected exportIds(): string[] {
    const sel = this.picked();
    return sel.length ? sel : (this.store.overview()?.runs ?? []).map((r) => r.id);
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
