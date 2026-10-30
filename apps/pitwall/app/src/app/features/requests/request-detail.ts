import { ChangeDetectionStrategy, Component, inject, input, output } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import type { RequestRecord } from '../../core/api.types';
import { date, duration, int } from '../../shared/format';
import { PRIORITY_LABEL } from '../../shared/labels';
import { DetailPanel } from '../../shared/ui/detail-panel';
import { Fact } from '../../shared/ui/fact';
import { API_LABEL, CACHE_LABEL, CHOSEN_LABEL, statusText } from './request-labels';
import { RequestsStore } from './requests.store';

/** The details of one request, in the panel beside the log. Prompt and response stay hidden until the user asks for them. */
@Component({
  selector: 'pw-request-detail',
  imports: [MatButton, MatIconModule, DetailPanel, Fact],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-detail-panel [heading]="r().endpoint" [sub]="appName() + ' · ' + when(r().at)" (closed)="closed.emit()">
      <div class="state">
        @if (r().status >= 400) {
          <span class="status" [attr.data-tone]="r().status >= 500 ? 'bad' : 'warn'">{{ statusLabel(r().status) }}</span>
        } @else {
          <span class="status" data-tone="ok">Succeeded · {{ r().status }}</span>
        }
        @if (r().stream) {
          <span class="faint">Streamed</span>
        }
      </div>
      @if (r().cache; as c) {
        <p class="cache">
          <span class="status" [attr.data-tone]="cacheLabel[c].tone">{{ cacheLabel[c].label }}</span>
          <span class="faint">{{ cacheLabel[c].text }} The <span class="mono">X-Pitwall-Cache</span> response header says <span class="mono">{{ c }}</span>.</span>
        </p>
      }
      @if (r().error) {
        <p class="err" role="note"><mat-icon svgIcon="pw:alert" aria-hidden="true" />{{ r().error }}</p>
      }

      <section aria-labelledby="pw-req-timing">
        <h3 id="pw-req-timing">Timing</h3>
        <div class="timing" role="img" [attr.aria-label]="'Queued ' + r().queuedMs + ' ms, first token ' + (r().ttftMs ?? 0) + ' ms, total ' + r().totalMs + ' ms'">
          <span class="q" [style.flex-grow]="r().queuedMs"></span>
          <span class="t" [style.flex-grow]="r().ttftMs ?? 0"></span>
          <span class="g" [style.flex-grow]="genMs()"></span>
        </div>
        <dl class="grid">
          <div><dt><i class="q"></i>Queued</dt><dd class="num">{{ dur(r().queuedMs) }}</dd></div>
          <div><dt><i class="t"></i>First token</dt><dd class="num">{{ dur(r().ttftMs) }}</dd></div>
          <div><dt><i class="g"></i>Total</dt><dd class="num">{{ dur(r().totalMs) }}</dd></div>
        </dl>
        @if (r().coldStartMs !== null) {
          <pw-fact label="Cold start" [value]="dur(r().coldStartMs)" [num]="true" />
        }
      </section>

      <section aria-labelledby="pw-req-route">
        <h3 id="pw-req-route">Routing</h3>
        <pw-fact label="API" [value]="api[r().api]" />
        @if (r().alias; as alias) {
          <pw-fact label="Preset asked for" [value]="alias" [mono]="true" />
          <pw-fact label="Resolved to" [value]="r().modelId ?? 'none'" [mono]="true" />
        } @else {
          <pw-fact label="Model" [value]="r().modelId ?? 'none'" [mono]="true" />
        }
        <pw-fact label="Chosen by" [value]="chosen[r().chosenBy]" />
        <pw-fact label="Priority" [value]="priority[r().priority]" />
        <pw-fact label="Prompt tokens" [value]="count(r().promptTokens)" [num]="true" />
        <pw-fact label="Completion tokens" [value]="count(r().completionTokens)" [num]="true" />
        @if (r().cachedTokens > 0) {
          <pw-fact label="Prompt tokens reused" [value]="count(r().cachedTokens)" [num]="true" />
        }
        <pw-fact label="Request id" [value]="r().id" [copy]="true" [mono]="true" />
      </section>

      <section aria-labelledby="pw-req-content">
        <h3 id="pw-req-content">Content</h3>
        @if (!r().hasContent) {
          <div class="hidden-box">
            <mat-icon svgIcon="pw:eye-off" aria-hidden="true" />
            <p>Pitwall didn't keep the prompt and response of this request. Keeping content is off by default (Settings ▸ Privacy).</p>
          </div>
        } @else if (store.content(); as c) {
          @if (c.loading) {
            <p class="faint">Loading…</p>
          } @else if (c.error) {
            <p class="err" role="alert"><mat-icon svgIcon="pw:alert" aria-hidden="true" />{{ c.error.message }}</p>
            <button type="button" matButton="outlined" (click)="store.showContent()">Retry</button>
          } @else if (c.data; as d) {
            <div class="content-head">
              <span class="faint">Request</span>
              <button type="button" matButton (click)="store.hideContent()"><mat-icon svgIcon="pw:eye-off" aria-hidden="true" />Hide content</button>
            </div>
            <pre class="mono" tabindex="0" aria-label="Request body">{{ d.request }}</pre>
            <span class="faint label2">Response</span>
            <pre class="mono" tabindex="0" aria-label="Response body">{{ d.response }}</pre>
          }
        } @else {
          <div class="hidden-box">
            <mat-icon svgIcon="pw:lock" aria-hidden="true" />
            <p>Prompts and responses are hidden. They are kept on this machine only.</p>
            <button type="button" matButton="outlined" (click)="store.showContent()"><mat-icon svgIcon="pw:eye" aria-hidden="true" />Show content</button>
          </div>
        }
      </section>
    </pw-detail-panel>
  `,
  styles: `
    :host {
      display: contents;
    }
    .state {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
      font-size: var(--pd-text-label);
    }
    .cache {
      display: flex;
      flex-direction: column;
      align-items: flex-start;
      gap: var(--pd-space-1);
      margin: var(--pd-space-3) 0 0;
      font-size: var(--pd-text-label);
    }
    .err {
      display: flex;
      gap: var(--pd-space-2);
      align-items: center;
      margin-top: var(--pd-space-3);
      padding: var(--pd-space-2) var(--pd-space-3);
      font-size: var(--pd-text-label);
      border-left: 3px solid var(--pd-danger);
      background: var(--pd-surface-2);
      border-radius: var(--pd-radius-tag);
      mat-icon {
        color: var(--pd-danger);
        flex: none;
      }
    }
    section {
      margin-top: var(--pd-space-4);
    }
    h3 {
      margin-bottom: var(--pd-space-2);
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-semibold);
      color: var(--pd-text-muted);
    }
    .timing {
      display: flex;
      gap: 1px;
      height: 8px;
      border-radius: var(--pd-radius-pill);
      overflow: hidden;
      background: var(--pd-surface-2);
      span {
        flex-basis: 0;
        min-width: 2px;
      }
    }
    .q {
      background: var(--pd-text-subtle);
    }
    .t {
      background: var(--pd-chart-2);
    }
    .g {
      background: var(--pd-chart-1);
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      margin: var(--pd-space-3) 0 var(--pd-space-2);
      dt {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: var(--pd-text-label);
        color: var(--pd-text-muted);
      }
      i {
        width: 8px;
        height: 8px;
        border-radius: 2px;
      }
      dd {
        margin: 2px 0 0;
        font-size: var(--pd-text-body-lg);
        font-weight: var(--pd-weight-semibold);
      }
    }
    .hidden-box {
      display: flex;
      flex-direction: column;
      align-items: flex-start;
      gap: var(--pd-space-2);
      padding: var(--pd-space-3);
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      background: var(--pd-surface-2);
      border-radius: var(--pd-radius-control);
      mat-icon {
        color: var(--pd-text-subtle);
      }
    }
    .content-head {
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-bottom: var(--pd-space-1);
      font-size: var(--pd-text-label);
    }
    .label2 {
      display: block;
      margin: var(--pd-space-3) 0 var(--pd-space-1);
      font-size: var(--pd-text-label);
    }
    pre {
      margin: 0;
      padding: var(--pd-space-3);
      max-height: 220px;
      overflow: auto;
      font-size: var(--pd-text-label);
      line-height: 1.5;
      white-space: pre-wrap;
      word-break: break-word;
      background: var(--pd-surface-2);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-control);
    }
  `,
})
export class RequestDetail {
  readonly r = input.required<RequestRecord>();
  readonly appName = input('');
  readonly closed = output<void>();
  protected readonly store = inject(RequestsStore);
  protected readonly dur = duration;
  protected readonly count = int;
  protected readonly statusLabel = statusText;
  protected readonly api = API_LABEL;
  protected readonly cacheLabel = CACHE_LABEL;
  protected readonly chosen = CHOSEN_LABEL;
  protected readonly priority = PRIORITY_LABEL;

  protected when(at: number): string {
    return date(at, true);
  }

  protected genMs(): number {
    const r = this.r();
    return Math.max(0, r.totalMs - r.queuedMs - (r.ttftMs ?? 0));
  }
}
