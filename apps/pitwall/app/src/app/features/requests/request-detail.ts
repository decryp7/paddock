import { A11yModule } from '@angular/cdk/a11y';
import { ChangeDetectionStrategy, Component, inject, input, output } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import type { RequestRecord } from '../../core/api.types';
import { date, duration, int } from '../../shared/format';
import { PRIORITY_LABEL, statusTone } from '../../shared/labels';
import { API_LABEL, CHOSEN_LABEL } from './request-labels';
import { RequestsStore } from './requests.store';

/** The detail drawer of one request. Prompt and response stay hidden until the user asks for them. */
@Component({
  selector: 'pw-request-detail',
  imports: [A11yModule, MatIconModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'dialog', 'aria-modal': 'false', 'aria-labelledby': 'pw-req-title', '(keydown.escape)': 'closed.emit()' },
  template: `
    <div class="inner" cdkTrapFocus [cdkTrapFocusAutoCapture]="true">
      <header>
        <div>
          <p class="eyebrow mono pitwall-selectable">{{ r().id }}</p>
          <h2 id="pw-req-title"><span class="mono">{{ r().endpoint }}</span></h2>
          <p class="faint">{{ appName() }} · {{ when(r().at) }}</p>
        </div>
        <button type="button" class="btn ghost icon" aria-label="Close details" (click)="closed.emit()" cdkFocusInitial><mat-icon svgIcon="pw:close" aria-hidden="true" /></button>
      </header>

      <div class="status-line">
        <span class="pill dot" [attr.data-tone]="tone(r().status)">{{ r().status }}</span>
        @if (r().stream) {
          <span class="tag">Streamed</span>
        }
        @if (r().coldStartMs !== null) {
          <span class="tag warn">Cold start {{ dur(r().coldStartMs) }}</span>
        }
      </div>
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
      </section>

      <section aria-labelledby="pw-req-route">
        <h3 id="pw-req-route">Routing</h3>
        <dl class="kv pitwall-selectable">
          <dt>API</dt><dd>{{ api[r().api] }}</dd>
          @if (r().alias; as alias) {
            <dt>Preset asked for</dt><dd class="mono">{{ alias }}</dd>
            <dt>Resolved to</dt><dd class="mono">{{ r().modelId ?? 'none' }}</dd>
          } @else {
            <dt>Model</dt><dd class="mono">{{ r().modelId ?? 'none' }}</dd>
          }
          <dt>Chosen by</dt><dd>{{ chosen[r().chosenBy] }}</dd>
          <dt>Priority</dt><dd>{{ priority[r().priority] }}</dd>
          <dt>Prompt tokens</dt><dd class="num">{{ count(r().promptTokens) }}</dd>
          <dt>Completion tokens</dt><dd class="num">{{ count(r().completionTokens) }}</dd>
        </dl>
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
            <button type="button" class="btn sm" (click)="store.showContent()">Retry</button>
          } @else if (c.data; as d) {
            <div class="content-head">
              <span class="faint">Request</span>
              <button type="button" class="btn ghost sm" (click)="store.hideContent()"><mat-icon svgIcon="pw:eye-off" aria-hidden="true" />Hide content</button>
            </div>
            <pre class="mono" tabindex="0" aria-label="Request body">{{ d.request }}</pre>
            <span class="faint label2">Response</span>
            <pre class="mono" tabindex="0" aria-label="Response body">{{ d.response }}</pre>
          }
        } @else {
          <div class="hidden-box">
            <mat-icon svgIcon="pw:lock" aria-hidden="true" />
            <p>Prompts and responses are hidden. They are kept on this machine only.</p>
            <button type="button" class="btn sm" (click)="store.showContent()"><mat-icon svgIcon="pw:eye" aria-hidden="true" />Show content</button>
          </div>
        }
      </section>
    </div>
  `,
  styles: `
    :host {
      position: fixed;
      top: calc(var(--titlebar-h) + var(--sp-3));
      right: var(--sp-3);
      bottom: var(--sp-3);
      z-index: 40;
      width: 440px;
      background: var(--bg-raised);
      border: 1px solid var(--border);
      border-radius: var(--r-xl);
      box-shadow: var(--shadow-pop);
      animation: slide var(--dur) var(--ease);
      overflow: hidden;
    }
    @keyframes slide {
      from {
        opacity: 0;
        transform: translateX(16px);
      }
    }
    .inner {
      height: 100%;
      overflow: auto;
      padding: var(--sp-5);
    }
    header {
      display: flex;
      justify-content: space-between;
      gap: var(--sp-3);
      h2 {
        margin: 2px 0;
        font-size: var(--fs-md);
        word-break: break-all;
      }
      p {
        font-size: var(--fs-sm);
      }
    }
    .eyebrow {
      font-size: var(--fs-xs);
      color: var(--text-3);
    }
    .status-line {
      display: flex;
      flex-wrap: wrap;
      gap: var(--sp-2);
      margin-top: var(--sp-4);
      .warn {
        color: var(--warn);
        border-color: color-mix(in srgb, var(--warn) 40%, transparent);
      }
    }
    .err {
      display: flex;
      gap: var(--sp-2);
      align-items: center;
      margin-top: var(--sp-3);
      padding: var(--sp-2) var(--sp-3);
      font-size: var(--fs-sm);
      background: var(--bad-soft);
      border-radius: var(--r-md);
      mat-icon {
        color: var(--bad);
        flex: none;
      }
    }
    section {
      margin-top: var(--sp-5);
    }
    h3 {
      margin-bottom: var(--sp-2);
      font-size: var(--fs-xs);
      font-weight: var(--fw-semibold);
      color: var(--text-3);
      text-transform: uppercase;
      letter-spacing: var(--tracking-caps);
    }
    .timing {
      display: flex;
      gap: 2px;
      height: 8px;
      border-radius: var(--r-full);
      overflow: hidden;
      background: var(--track);
      span {
        flex-basis: 0;
        min-width: 2px;
      }
    }
    .q {
      background: var(--text-3);
    }
    .t {
      background: var(--warn);
    }
    .g {
      background: var(--accent);
    }
    .grid {
      display: grid;
      grid-template-columns: repeat(3, 1fr);
      margin: var(--sp-3) 0 0;
      dt {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: var(--fs-xs);
        color: var(--text-3);
      }
      i {
        width: 8px;
        height: 8px;
        border-radius: 2px;
      }
      dd {
        margin: 2px 0 0;
        font-size: var(--fs-md);
        font-weight: var(--fw-semibold);
      }
    }
    .kv {
      display: grid;
      grid-template-columns: 140px 1fr;
      gap: 8px var(--sp-3);
      margin: 0;
      font-size: var(--fs-sm);
      dt {
        color: var(--text-3);
      }
      dd {
        margin: 0;
        word-break: break-all;
      }
    }
    .hidden-box {
      display: flex;
      flex-direction: column;
      align-items: flex-start;
      gap: var(--sp-2);
      padding: var(--sp-4);
      font-size: var(--fs-sm);
      color: var(--text-2);
      background: var(--bg-sunken);
      border: 1px dashed var(--border-strong);
      border-radius: var(--r-md);
      mat-icon {
        color: var(--text-3);
      }
    }
    .content-head {
      display: flex;
      justify-content: space-between;
      align-items: center;
      margin-bottom: var(--sp-1);
      font-size: var(--fs-xs);
    }
    .label2 {
      display: block;
      margin: var(--sp-3) 0 var(--sp-1);
      font-size: var(--fs-xs);
    }
    pre {
      margin: 0;
      padding: var(--sp-3);
      max-height: 260px;
      overflow: auto;
      font-size: var(--fs-xs);
      line-height: 1.5;
      white-space: pre-wrap;
      word-break: break-word;
      background: var(--bg-sunken);
      border: 1px solid var(--border-soft);
      border-radius: var(--r-md);
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
  protected readonly tone = statusTone;
  protected readonly api = API_LABEL;
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
