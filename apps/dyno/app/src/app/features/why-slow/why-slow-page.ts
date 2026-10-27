import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import { ComingDialog } from '../../shell/coming-dialog';
import type { Cause, Diagnosis, Suggestion } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { Dialogs } from '../../shared/dialog';
import { formatDayTime, formatFraction } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { CAUSE_ICON } from '../../shared/labels';
import { Notifier } from '../../shared/notifier';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { causeLinks, diagnosisText, fixPreview, historyLine, windowText } from './why-slow-model';

/**
 * "Why is my PC slow?" in full: the engine's likeliest causes with what was measured, the apps behind each, pages to look
 * closer, and the fixes it proposes. Every fix asks first; nothing changes until you click it.
 */
@Component({
  selector: 'dy-why-slow-page',
  imports: [Icon, RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Why is my PC slow?</h1>
          <p>Dyno weighs CPU, memory, disk, heat and background work together. It only suggests; nothing changes until you click.</p>
        </div>
        <button type="button" class="btn" [disabled]="!shown()" (click)="copy()"><dy-icon name="copy" /> Copy as text</button>
        <button type="button" class="btn primary" [disabled]="store.running()" (click)="store.run()">
          <dy-icon name="refresh" /> {{ store.running() ? 'Checking…' : 'Run again' }}
        </button>
      </header>

      @if (store.error(); as e) {
        <div class="note" role="alert">
          <dy-icon name="alert" />
          <div class="msg"><strong>{{ e.message }}</strong>@if (hint(); as h) {<span>{{ h }}</span>}</div>
          <button type="button" class="btn sm" (click)="store.run()"><dy-icon name="refresh" /> Retry</button>
        </div>
      }

      @if (shown(); as d) {
        <section class="card summary" aria-label="Result">
          <h2 class="headline">{{ d.headline }}</h2>
          <p class="when num">
            {{ window(d) }}, checked {{ day(d.at) }}.
            @if (viewingOlder()) { This is an earlier result. <button type="button" class="btn link" (click)="selected.set(null)">Show the latest</button> }
          </p>
        </section>

        @for (c of d.causes; track c.kind + c.title; let i = $index) {
          <section class="card cause" [attr.aria-label]="c.title">
            <div class="top">
              <span class="ic"><dy-icon [name]="icon(c.kind)" /></span>
              <div class="txt">
                <h3>{{ c.title }}</h3>
                <p>{{ c.explanation }}</p>
              </div>
              <span class="conf num" [title]="'How sure Dyno is: ' + conf(c.confidence)">{{ conf(c.confidence) }} confident</span>
            </div>

            <div class="cols">
              @if (c.evidence.length) {
                <div class="col">
                  <h4>What was measured</h4>
                  <dl class="evidence num">
                    @for (e of c.evidence; track e.label) {
                      <div><dt>{{ e.label }}</dt><dd>{{ e.value }}</dd></div>
                    }
                  </dl>
                </div>
              }
              @if (c.involved.length) {
                <div class="col">
                  <h4>Apps involved</h4>
                  <ul class="apps">
                    @for (a of c.involved; track a.id) {
                      <li><span class="n truncate" [title]="a.name">{{ a.name }}</span><span class="num">{{ a.share }}</span></li>
                    }
                  </ul>
                </div>
              }
            </div>

            <div class="foot">
              <span class="look">Look closer:</span>
              @for (l of links(c); track l.path) {
                <a class="btn sm ghost" [routerLink]="l.path"><dy-icon [name]="l.icon" /> {{ l.label }}</a>
              }
              <span class="spacer"></span>
              @for (s of c.suggestions; track s.label) {
                @if (isWait(s)) {
                  <span class="wait">{{ s.label }}: waiting is enough</span>
                } @else {
                  <button type="button" class="btn sm" [class.primary]="i === 0 && isProcess(s)" (click)="take(s)">{{ s.label }}</button>
                }
              }
            </div>
          </section>
        }
      } @else if (store.running()) {
        <div class="skeleton" style="width: 60%"></div>
        <div class="skeleton"></div>
        <div class="skeleton" style="width: 80%"></div>
      } @else if (!store.error()) {
        <section class="card summary">
          <p class="idle">Run a check to see the likeliest cause, the numbers behind it and what you can do.</p>
        </section>
      }

      @if (store.history().length > 1) {
        <section class="card history" aria-label="Earlier results">
          <div class="card-head"><h2><dy-icon name="clock" /> Earlier in this session</h2></div>
          <ul>
            @for (h of store.history(); track h.at + '-' + $index; let i = $index) {
              <li>
                <button type="button" class="row" [attr.aria-pressed]="isShown(h)" (click)="select(h, i)">
                  <span class="t num">{{ day(h.at) }}</span>
                  <span class="h truncate">{{ line(h) }}</span>
                </button>
              </li>
            }
          </ul>
        </section>
      }
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .page-head { margin-bottom: 0; }
    .note { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-3) var(--sp-4); border-radius: var(--r-md); border: 1px solid var(--border); background: var(--surface-2); font-size: var(--fs-sm); }
    .note .msg { flex: 1; display: grid; min-width: 0; }
    .note .msg span { color: var(--text-2); }
    .summary { padding: var(--sp-4); display: grid; gap: var(--sp-2); }
    .headline { font: var(--fw-semibold) var(--fs-lg) / 1.3 var(--font-display); margin: 0; }
    .when, .idle { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .cause { padding: var(--sp-4); display: grid; gap: var(--sp-4); }
    .top { display: flex; gap: var(--sp-3); align-items: flex-start; }
    .ic { display: grid; place-items: center; width: 32px; height: 32px; border-radius: var(--r-md); background: var(--surface-inset); color: var(--text-2); flex: none; }
    .txt { flex: 1; min-width: 0; display: grid; gap: var(--sp-1); }
    .txt h3 { font-size: var(--fs-md); font-weight: var(--fw-semibold); margin: 0; }
    .txt p { margin: 0; color: var(--text-2); font-size: var(--fs-base); max-width: 72ch; }
    .conf { color: var(--text-3); font-size: var(--fs-sm); white-space: nowrap; }
    .cols { display: grid; grid-template-columns: repeat(auto-fit, minmax(260px, 1fr)); gap: var(--sp-5); }
    .col h4 { margin: 0 0 var(--sp-2); font-size: var(--fs-sm); font-weight: var(--fw-medium); color: var(--text-3); }
    .evidence { margin: 0; display: grid; }
    .evidence div, .apps li { display: flex; justify-content: space-between; gap: var(--sp-3); padding: var(--sp-1) 0; border-top: 1px solid var(--border-subtle); font-size: var(--fs-base); }
    .evidence div:first-child, .apps li:first-child { border-top: 0; }
    .evidence dt { color: var(--text-2); }
    .evidence dd { margin: 0; text-align: right; }
    .apps { list-style: none; margin: 0; padding: 0; display: grid; }
    .apps .n { min-width: 0; }
    .foot { display: flex; align-items: center; flex-wrap: wrap; gap: var(--sp-2); padding-top: var(--sp-3); border-top: 1px solid var(--border-subtle); }
    .foot .look { color: var(--text-3); font-size: var(--fs-sm); margin-right: var(--sp-1); }
    .foot .spacer { flex: 1; }
    .wait { color: var(--text-3); font-size: var(--fs-sm); }
    .history ul { list-style: none; margin: 0; padding: var(--sp-2) var(--sp-2) var(--sp-3); display: grid; }
    .history .row { display: flex; gap: var(--sp-3); width: 100%; padding: var(--sp-2) var(--sp-2); border: 0; border-radius: var(--r-sm); background: none; color: var(--text-1); font: inherit; text-align: left; }
    .history .row:hover { background: var(--surface-hover); }
    .history .row[aria-pressed='true'] { background: var(--surface-inset); }
    .history .t { color: var(--text-3); font-size: var(--fs-sm); min-width: 92px; }
    .history .h { flex: 1; min-width: 0; }
    .num { font-variant-numeric: tabular-nums; }
  `,
})
export class WhySlowPage implements OnInit {
  protected readonly store = inject(DiagnosisStore);
  private readonly router = inject(Router);
  private readonly dialogs = inject(Dialogs);
  private readonly notifier = inject(Notifier);

  /** An earlier result the person picked from the history (null = the latest). */
  protected readonly selected = signal<Diagnosis | null>(null);
  protected readonly shown = computed(() => this.selected() ?? this.store.diagnosis());
  protected readonly viewingOlder = computed(() => this.selected() !== null && this.selected() !== this.store.diagnosis());
  protected readonly hint = computed(() => {
    const e = this.store.error();
    return e ? errorHint(e.code) : null;
  });
  protected readonly icon = (k: Cause['kind']) => CAUSE_ICON[k];
  protected readonly conf = formatFraction;
  protected readonly day = formatDayTime;
  protected readonly window = windowText;
  protected readonly line = historyLine;
  protected readonly links = (c: Cause) => causeLinks(c.kind);

  ngOnInit(): void {
    void this.store.ensure();
  }

  protected isShown(d: Diagnosis): boolean {
    return d === this.shown();
  }

  protected select(d: Diagnosis, index: number): void {
    this.selected.set(index === 0 ? null : d);
  }

  protected isWait(s: Suggestion): boolean {
    return s.action === 'wait';
  }

  protected isProcess(s: Suggestion): boolean {
    return typeof s.action === 'object';
  }

  protected async copy(): Promise<void> {
    const d = this.shown();
    if (!d) return;
    try {
      await navigator.clipboard.writeText(diagnosisText(d));
      this.notifier.show('Copied the diagnosis');
    } catch {
      this.notifier.show('Dyno could not reach the clipboard. Select the text on the page instead.', 'warn');
    }
  }

  /** A fix that changes a process shows exactly what it will do and waits for a second click; the rest only navigate. */
  protected async take(s: Suggestion): Promise<void> {
    const a = s.action;
    if (typeof a === 'object') {
      const p = fixPreview(s);
      if (p && !(await this.dialogs.confirm({ title: p.title, message: p.message, details: p.details, danger: p.danger, confirmLabel: p.confirmLabel }))) return;
      await this.store.act(a.process.target, a.process.action);
    } else if (a === 'openCleanup') this.dialogs.open(ComingDialog);
    else if (a === 'openTaskManager') void this.router.navigateByUrl('/processes');
    else if (a === 'openStartup') void this.router.navigateByUrl('/health');
    else if (a === 'restart') this.notifier.show('Restart when it suits you. Dyno never restarts your PC for you.');
  }
}
