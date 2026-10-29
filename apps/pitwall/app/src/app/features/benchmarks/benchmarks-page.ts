import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { ThemeService } from '../../core/theme';
import { benchmarkChart, latestPerRun } from '../../shared/charts/benchmark-chart';
import { EChart } from '../../shared/charts/echart';
import { date, int, rate } from '../../shared/format';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { BenchmarksStore } from './benchmarks.store';

/** How fast each model runs on this hardware: run per model, compare tokens per second and time to first token. */
@Component({
  selector: 'pw-benchmarks-page',
  imports: [MatIconModule, MatTooltipModule, Panel, EChart, EmptyState],
  providers: [BenchmarksStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Benchmarks</h1>
          <p class="sub">Measured on this machine with a 4k-token prompt. Each run loads the model if needed.</p>
        </div>
        <div class="actions">
          <button type="button" class="btn primary" [disabled]="store.runnable().length === 0 || store.anyRunning()" (click)="store.runAll()"><mat-icon svgIcon="pw:play" aria-hidden="true" />Run all</button>
        </div>
      </header>

      <div class="grid">
        <pw-panel class="models" heading="Models" icon="models" [loading]="store.loading()" [error]="store.error()" [ready]="store.models().length > 0 || (!store.loading() && !store.error())" (retry)="store.load()" [flush]="true">
          @if (store.runnable().length === 0) {
            <pw-empty-state icon="models" heading="No chat models" text="Download a model in Models to measure it here." />
          }
          <ul aria-label="Models to benchmark">
            @for (m of store.runnable(); track m.id) {
              @let last = store.latest().get(m.id);
              <li>
                <div class="main">
                  <span class="name">{{ m.name }}</span>
                  <span class="meta">
                    {{ m.quant ?? 'GGUF' }}{{ m.paramsB ? ' · ' + m.paramsB + 'B' : '' }}
                    @if (last) {
                      · last run {{ day(last.at) }}
                    } @else {
                      · never run
                    }
                  </span>
                </div>
                @if (last) {
                  <span class="score num" [matTooltip]="'Generation on ' + last.build"><b>{{ rateText(last.genTps) }}</b> tok/s</span>
                }
                <button type="button" class="btn ghost icon sm run" [class.on]="store.isRunning(m.id)" [attr.aria-label]="'Run benchmark for ' + m.name" [matTooltip]="store.isRunning(m.id) ? 'Running' : 'Run'" [disabled]="store.isRunning(m.id)" (click)="store.run(m.id)">
                  @if (store.isRunning(m.id)) {
                    <span class="spin" aria-hidden="true"></span>
                  } @else {
                    <mat-icon svgIcon="pw:play" aria-hidden="true" />
                  }
                </button>
              </li>
            }
          </ul>
        </pw-panel>

        <pw-panel class="chart" heading="Speed on this hardware" icon="benchmarks" sub="newest run per model and build" [loading]="store.loading()" [error]="store.error()" [ready]="store.results().length > 0 || (!store.loading() && !store.error())" (retry)="store.load()">
          @if (rows().length === 0) {
            <pw-empty-state icon="benchmarks" heading="No results yet" text="Run a benchmark to see tokens per second and time to first token." />
          } @else {
            <pw-echart [option]="option()" [themeKey]="theme.resolved()" [height]="chartHeight()" label="Generation tokens per second and time to first token per model and build" />
          }
        </pw-panel>
      </div>

      <pw-panel heading="Results" icon="requests" [sub]="store.results().length + ' runs'" [loading]="store.loading()" [error]="store.error()" [ready]="store.results().length > 0 || (!store.loading() && !store.error())" (retry)="store.load()" [flush]="true">
        @if (store.results().length === 0) {
          <pw-empty-state icon="benchmarks" heading="No runs yet" />
        } @else {
          <table class="table fit">
            <caption class="sr-only">Benchmark results, newest first</caption>
            <colgroup>
              <col /><col /><col class="c-gen" /><col class="c-prompt" /><col class="c-n" />
              @if (contextVaries()) {
                <col class="c-n" />
              }
              <col class="c-when" />
            </colgroup>
            <thead>
              <tr>
                <th scope="col">Model</th>
                <th scope="col">Build</th>
                <th scope="col" class="right">Generation tok/s</th>
                <th scope="col" class="right">Prompt tok/s</th>
                <th scope="col" class="right">TTFT ms</th>
                @if (contextVaries()) {
                  <th scope="col" class="right">Context</th>
                }
                <th scope="col" class="right">When</th>
              </tr>
            </thead>
            <tbody>
              @for (r of store.sorted(); track $index) {
                <tr>
                  <td class="strong" [matTooltip]="store.names().get(r.modelId) ?? r.modelId" matTooltipShowDelay="600">{{ store.names().get(r.modelId) ?? r.modelId }}</td>
                  <td class="build" [matTooltip]="r.build">{{ r.build }}</td>
                  <td class="right num"><b>{{ rateText(r.genTps) }}</b></td>
                  <td class="right num">{{ intText(r.promptTps) }}</td>
                  <td class="right num">{{ intText(r.ttftMs) }}</td>
                  @if (contextVaries()) {
                    <td class="right num">{{ intText(r.contextTokens) }}</td>
                  }
                  <td class="right when">{{ day(r.at, true) }}</td>
                </tr>
              }
            </tbody>
          </table>
        }
      </pw-panel>
    </div>
  `,
  styles: `
    .c-gen {
      width: 128px;
    }
    .c-prompt {
      width: 104px;
    }
    .c-n {
      width: 80px;
    }
    .c-when {
      width: 112px;
    }
    .page {
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-3);
    }
    .grid {
      display: grid;
      grid-template-columns: minmax(0, 5fr) minmax(0, 7fr);
      gap: var(--pd-space-3);
      align-items: start;
    }
    ul {
      list-style: none;
      margin: 0;
      padding: 0 var(--pd-space-4) var(--pd-space-2);
    }
    li {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
      padding: 10px 0;
      border-top: 1px solid var(--pd-hairline);
      &:first-child {
        border-top: none;
      }
    }
    .main {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-width: 0;
    }
    .name {
      font-weight: var(--pd-weight-medium);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .meta {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .score {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      white-space: nowrap;
      b {
        font-size: var(--pd-text-body-lg);
        font-weight: var(--pd-weight-semibold);
        color: var(--pd-text);
      }
    }
    /* Run is a quiet icon on hover or focus (while it runs it stays, with a spinner); "Run all" is the page's one primary. */
    .run {
      opacity: 0;
      &.on {
        opacity: 1;
      }
    }
    li:hover .run,
    li:focus-within .run,
    .run:focus-visible {
      opacity: 1;
    }
    @media (hover: none) {
      .run {
        opacity: 1;
      }
    }
    .spin {
      width: 11px;
      height: 11px;
      border: 1.5px solid currentColor;
      border-right-color: transparent;
      border-radius: 50%;
      animation: spin 0.8s linear infinite;
    }
    @keyframes spin {
      to {
        transform: rotate(360deg);
      }
    }
    .table {
      margin-top: var(--pd-space-3);
      th:first-child,
      td:first-child {
        padding-left: var(--pd-space-4);
      }
      th:last-child,
      td:last-child {
        padding-right: var(--pd-space-4);
      }
    }
    .strong {
      font-weight: var(--pd-weight-medium);
    }
    .table b {
      font-weight: var(--pd-weight-semibold);
    }
    .build,
    .when {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    @media (max-width: 1100px) {
      .grid {
        grid-template-columns: minmax(0, 1fr);
      }
    }
  `,
})
export class BenchmarksPage implements OnInit {
  protected readonly store = inject(BenchmarksStore);
  protected readonly theme = inject(ThemeService);
  protected readonly rateText = rate;
  protected readonly intText = int;
  protected readonly day = date;
  /** The context column only exists when runs differ in it: every run uses the same 4k prompt, so it is noise. */
  protected readonly contextVaries = computed(() => new Set(this.store.results().map((r) => r.contextTokens)).size > 1);
  protected readonly rows = computed(() => latestPerRun(this.store.results(), (id) => this.store.names().get(id) ?? id));
  protected readonly option = computed(() => benchmarkChart(this.rows(), this.theme.tokens()));
  protected readonly chartHeight = computed(() => `${Math.max(200, 82 + this.rows().length * 40)}px`);

  ngOnInit(): void {
    void this.store.load();
  }
}
