import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { Router, RouterLink } from '@angular/router';
import { ComingDialog } from '../../shell/coming-dialog';
import { Dialogs } from '../../shared/dialog';
import type { Suggestion } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import { formatCount, formatFraction, formatPercent, formatRate } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { CAUSE_ICON } from '../../shared/labels';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { LiveStore } from '../../state/live.store';

/** "Why is my PC slow?": one button, and the engine's latest answer (headline, top cause, evidence, what to do). */
@Component({
  selector: 'dy-diagnosis-card',
  imports: [Icon, RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="Why is my PC slow?">
      <div class="card-head">
        <h2><dy-icon name="why" /> Why is my PC slow?</h2>
        <span class="spacer"></span>
        @if (store.diagnosis()) {
          <a class="btn sm ghost" routerLink="/why-slow">See the evidence</a>
        }
        <button type="button" class="btn sm primary" [disabled]="store.running()" (click)="store.run()">
          <dy-icon name="why" /> {{ store.diagnosis() ? 'Check again' : 'Check now' }}
        </button>
      </div>
      <div class="body" [attr.aria-busy]="store.running()">
        @if (store.error(); as e) {
          <div class="err" role="alert">
            <dy-icon name="alert" />
            <div class="text"><strong>{{ e.message }}</strong>@if (hint(); as h) {<span>{{ h }}</span>}</div>
            <button type="button" class="btn sm" (click)="store.run()">Retry</button>
          </div>
        } @else if (store.running() && !store.diagnosis()) {
          <div class="skeleton" style="width: 70%"></div>
          <div class="skeleton"></div>
        } @else if (store.diagnosis(); as d) {
          <p class="headline">{{ d.headline }}</p>
          @if (cause(); as c) {
            <div class="cause">
              <span class="ic"><dy-icon [name]="icon(c.kind)" /></span>
              <div class="txt">
                <strong>{{ c.title }}</strong>
                <span>{{ c.explanation }}</span>
                <p class="evidence num">@for (e of c.evidence; track e.label) {<span>{{ e.label }} {{ e.value }}</span>}<span>Confidence {{ conf(c.confidence) }}</span></p>
              </div>
            </div>
            @if (c.suggestions.length) {
              <div class="actions">
                @for (s of c.suggestions; track s.label) {
                  <button type="button" class="btn sm" (click)="take(s)">{{ s.label }}</button>
                }
              </div>
            }
          }
          @if (others().length) {
            <button type="button" class="btn link more" [attr.aria-expanded]="showOthers()" (click)="showOthers.set(!showOthers())">
              {{ showOthers() ? 'Hide' : 'Show' }} {{ others().length === 1 ? '1 other possible cause' : count(others().length) + ' other possible causes' }}
            </button>
            @if (showOthers()) {
              <ul class="others">
                @for (c of others(); track c.title) {
                  <li><dy-icon [name]="icon(c.kind)" /><span class="t">{{ c.title }}</span><span class="num faint">{{ conf(c.confidence) }}</span></li>
                }
              </ul>
            }
          }
        } @else {
          <p class="idle">Dyno looks at CPU, memory, disk, heat and background work together and names the likeliest cause, with the evidence. It only suggests: nothing changes until you click.</p>
        }
      </div>
      @if (stats(); as st) {
        <dl class="stats num" aria-label="Right now">
          @for (x of st; track x.k) {
            <div><dt>{{ x.k }}</dt><dd [title]="x.v">{{ x.v }}</dd></div>
          }
        </dl>
      }
    </section>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .card { height: 100%; }
    .body { padding: var(--sp-4) var(--sp-4) var(--sp-5); display: grid; gap: var(--sp-4); align-content: start; }
    .idle { color: var(--text-2); max-width: 62ch; }
    .headline { font: var(--fw-semibold) var(--fs-lg) / 1.3 var(--font-display); }
    .cause { display: flex; gap: var(--sp-3); align-items: flex-start; }
    .ic { display: grid; place-items: center; width: 28px; height: 28px; color: var(--text-2); flex: none; }
    .txt { display: grid; gap: var(--sp-2); min-width: 0; }
    .txt > span { color: var(--text-2); font-size: var(--fs-sm); }
    .evidence { margin: 0; color: var(--text-3); font-size: var(--fs-sm); line-height: 1.5; }
    .evidence span + span::before { content: ' · '; }
    .actions { display: flex; flex-wrap: wrap; gap: var(--sp-2); padding-top: var(--sp-1); }
    .more { justify-self: start; }
    .others { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-1); font-size: var(--fs-sm); }
    .others li { display: flex; align-items: center; gap: var(--sp-2); }
    .others dy-icon { --icon-size: 14px; color: var(--text-3); }
    .others .t { flex: 1; }
    .faint { color: var(--text-3); }
    .card { display: flex; flex-direction: column; }
    .body { flex: 1; }
    .stats { margin: 0; padding: var(--sp-4); display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--sp-3); border-top: 1px solid var(--border-subtle); }
    .stats dt { color: var(--text-3); font-size: var(--fs-sm); }
    .stats dd { margin: 2px 0 0; font: var(--fw-semibold) var(--fs-md) / 1.3 var(--font-display); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .err { display: flex; gap: var(--sp-3); align-items: flex-start; }
    .err .text { flex: 1; display: grid; }
    .err .text span { color: var(--text-2); font-size: var(--fs-sm); }
  `,
})
export class DiagnosisCard {
  protected readonly store = inject(DiagnosisStore);
  protected readonly router = inject(Router);
  private readonly dialogs = inject(Dialogs);
  private readonly live = inject(LiveStore);
  /** The numbers the diagnosis weighs, as they are right now (formatting only; the engine does the reasoning). */
  protected readonly stats = computed(() => {
    const u = this.live.update();
    if (!u) return null;
    const sys = u.system;
    const memPct = sys.memory.total > 0 ? (sys.memory.used / sys.memory.total) * 100 : 0;
    const top = u.topCpu[0];
    return [
      { k: 'CPU', v: formatPercent(sys.cpu.total, 0) },
      { k: 'Memory', v: formatPercent(memPct, 0) },
      { k: 'Disk', v: formatRate(sys.diskIo.readBps + sys.diskIo.writeBps) },
      { k: 'Busiest', v: top ? top.name : '—' },
    ];
  });
  protected readonly cause = computed(() => this.store.diagnosis()?.causes[0] ?? null);
  protected readonly hint = computed(() => {
    const e = this.store.error();
    return e ? errorHint(e.code) : null;
  });
  protected readonly icon = CAUSE_ICON_FN;
  protected readonly conf = formatFraction;
  protected readonly count = formatCount;
  protected readonly showOthers = signal(false);
  protected readonly others = computed(() => this.store.diagnosis()?.causes.slice(1) ?? []);

  protected take(s: Suggestion): void {
    const a = s.action;
    if (typeof a === 'object') void this.store.act(a.process.target, a.process.action);
    else if (a === 'openCleanup') this.dialogs.open(ComingDialog);
    else if (a === 'openTaskManager') void this.router.navigateByUrl('/processes');
    else if (a === 'openStartup') void this.router.navigateByUrl('/health');
  }
}

function CAUSE_ICON_FN(kind: keyof typeof CAUSE_ICON) {
  return CAUSE_ICON[kind];
}
