import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressSpinner } from '@angular/material/progress-spinner';
import { MatTableModule } from '@angular/material/table';
import { MatTooltipModule } from '@angular/material/tooltip';
import { ThemeService } from '../../core/theme';
import { benchmarkChart, latestPerRun } from '../../shared/charts/benchmark-chart';
import { EChart } from '../../shared/charts/echart';
import { date, int, rate } from '../../shared/format';
import { Panel } from '../../shared/ui/panel';
import { BenchmarksStore } from './benchmarks.store';
import { SuEmptyState } from '@decryp7/paddock-ui';

/** How fast each model runs on this hardware: run per model, compare tokens per second and time to first token. */
@Component({
  selector: 'pw-benchmarks-page',
  imports: [MatButton, MatIconButton, MatProgressSpinner, MatIconModule, MatTooltipModule, MatTableModule, Panel, EChart, SuEmptyState],
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
          <button type="button" matButton="filled" [disabled]="store.runnable().length === 0 || store.anyRunning()" (click)="store.runAll()"><mat-icon svgIcon="pw:play" aria-hidden="true" />Run all</button>
        </div>
      </header>

      <div class="grid">
        <pw-panel class="models" heading="Models" icon="models" [loading]="store.loading()" [error]="store.error()" [ready]="store.models().length > 0 || (!store.loading() && !store.error())" (retry)="store.load()" [flush]="true">
          @if (store.runnable().length === 0) {
            <pd-empty-state heading="No chat models" description="Download a model in Models to measure it here."><mat-icon slot="icon" svgIcon="pw:models" aria-hidden="true" /></pd-empty-state>
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
                <button type="button" matIconButton class="run" [class.on]="store.isRunning(m.id)" [attr.aria-label]="'Run benchmark for ' + m.name" [matTooltip]="store.isRunning(m.id) ? 'Running' : 'Run'" [disabled]="store.isRunning(m.id)" (click)="store.run(m.id)">
                  @if (store.isRunning(m.id)) {
                    <mat-progress-spinner class="spin" mode="indeterminate" diameter="14" strokeWidth="2" aria-hidden="true" />
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
            <pd-empty-state heading="No results yet" description="Run a benchmark to see tokens per second and time to first token."><mat-icon slot="icon" svgIcon="pw:benchmarks" aria-hidden="true" /></pd-empty-state>
          } @else {
            <pw-echart [option]="option()" [themeKey]="theme.resolved()" [height]="chartHeight()" label="Generation tokens per second and time to first token per model and build" />
          }
        </pw-panel>
      </div>

      <pw-panel heading="Results" icon="requests" [sub]="store.results().length + ' runs'" [loading]="store.loading()" [error]="store.error()" [ready]="store.results().length > 0 || (!store.loading() && !store.error())" (retry)="store.load()" [flush]="true">
        @if (store.results().length === 0) {
          <pd-empty-state heading="No runs yet"><mat-icon slot="icon" svgIcon="pw:benchmarks" aria-hidden="true" /></pd-empty-state>
        } @else {
          <table mat-table class="results" [dataSource]="store.sorted()" [fixedLayout]="true">
            <caption class="sr-only">Benchmark results, newest first</caption>
            <colgroup>
              <col /><col /><col class="c-gen" /><col class="c-prompt" /><col class="c-n" />
              @if (contextVaries()) {
                <col class="c-n" />
              }
              <col class="c-when" />
            </colgroup>
            <ng-container matColumnDef="model">
              <th mat-header-cell *matHeaderCellDef scope="col">Model</th>
              <td mat-cell *matCellDef="let r" class="strong" [matTooltip]="store.names().get(r.modelId) ?? r.modelId" matTooltipShowDelay="600">{{ store.names().get(r.modelId) ?? r.modelId }}</td>
            </ng-container>
            <ng-container matColumnDef="build">
              <th mat-header-cell *matHeaderCellDef scope="col">Build</th>
              <td mat-cell *matCellDef="let r" class="build" [matTooltip]="r.build">{{ r.build }}</td>
            </ng-container>
            <ng-container matColumnDef="gen">
              <th mat-header-cell *matHeaderCellDef scope="col" class="right">Generation tok/s</th>
              <td mat-cell *matCellDef="let r" class="right num"><b>{{ rateText(r.genTps) }}</b></td>
            </ng-container>
            <ng-container matColumnDef="prompt">
              <th mat-header-cell *matHeaderCellDef scope="col" class="right">Prompt tok/s</th>
              <td mat-cell *matCellDef="let r" class="right num">{{ intText(r.promptTps) }}</td>
            </ng-container>
            <ng-container matColumnDef="ttft">
              <th mat-header-cell *matHeaderCellDef scope="col" class="right">TTFT ms</th>
              <td mat-cell *matCellDef="let r" class="right num">{{ intText(r.ttftMs) }}</td>
            </ng-container>
            <ng-container matColumnDef="context">
              <th mat-header-cell *matHeaderCellDef scope="col" class="right">Context tokens</th>
              <td mat-cell *matCellDef="let r" class="right num">{{ intText(r.contextTokens) }}</td>
            </ng-container>
            <ng-container matColumnDef="when">
              <th mat-header-cell *matHeaderCellDef scope="col" class="right">When</th>
              <td mat-cell *matCellDef="let r" class="right when">{{ day(r.at, true) }}</td>
            </ng-container>
            <tr mat-header-row *matHeaderRowDef="columns(); sticky: true"></tr>
            <tr mat-row *matRowDef="let r; columns: columns()"></tr>
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
    .results {
      margin-top: var(--pd-space-3);
      .mat-mdc-header-cell:last-child,
      .mat-mdc-cell:last-child {
        padding-right: var(--pd-space-4);
      }
    }
    .strong {
      font-weight: var(--pd-weight-medium);
    }
    .results b {
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
  /** The results table's columns: Context only when the runs used different context sizes (rule 7, no constant column). */
  protected readonly columns = computed(() => ['model', 'build', 'gen', 'prompt', 'ttft', ...(this.contextVaries() ? ['context'] : []), 'when']);
  protected readonly rows = computed(() => latestPerRun(this.store.results(), (id) => this.store.names().get(id) ?? id));
  protected readonly option = computed(() => benchmarkChart(this.rows(), this.theme.tokens()));
  protected readonly chartHeight = computed(() => `${Math.max(200, 82 + this.rows().length * 40)}px`);

  ngOnInit(): void {
    void this.store.load();
  }
}
