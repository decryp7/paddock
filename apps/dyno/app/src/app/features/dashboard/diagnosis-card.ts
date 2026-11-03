import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { formatCount, formatFraction, formatPercent, formatRate } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { CAUSE_ICON } from '../../shared/labels';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { LiveStore } from '../../state/live.store';
import { hintPath } from '../why-slow/why-slow-model';
import { MatButton } from '@angular/material/button';
import { FullTextTip } from '../../shared/full-text-tip';

/** "Why is my computer slow?": one button, and the engine's latest answer (headline, top cause, evidence, what to do). */
@Component({
  selector: 'dy-diagnosis-card',
  imports: [FullTextTip, MatButton, Icon, RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="tools">
      <span class="spacer"></span>
      @if (store.diagnosis()) {
        <a matButton="text" routerLink="/why-slow">See the evidence</a>
      }
      <button type="button" matButton="filled" [disabled]="store.running()" (click)="store.run()">
        <dy-icon name="why" /> {{ store.diagnosis() ? 'Check again' : 'Check now' }}
      </button>
    </div>
    <div class="body" [attr.aria-busy]="store.running()">
      @if (store.diagnosis(); as d) {
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
          @if (c.hints.length) {
            <div class="actions">
              @for (h of c.hints; track h.text) {
                @if (hintPath(h); as path) {
                  <a matButton="outlined" [routerLink]="path">{{ h.text }}</a>
                } @else {
                  <span class="hint">{{ h.text }}</span>
                }
              }
            </div>
          }
        }
        @if (others().length) {
          <button type="button" matButton="text" class="more dy-link" [attr.aria-expanded]="showOthers()" (click)="showOthers.set(!showOthers())">
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
        <p class="idle">Dyno looks at CPU, memory, disk, heat and background work together and names the likeliest cause, with the evidence. It only looks: Dyno changes nothing on your computer.</p>
      }
    </div>
    @if (stats(); as st) {
      <dl class="stats num" aria-label="Right now">
        @for (x of st; track x.k) {
          <div><dt>{{ x.k }}</dt><dd [dyFullText]="x.v">{{ x.v }}</dd></div>
        }
      </dl>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--sp-3); min-width: 0; }
    .tools { display: flex; align-items: center; gap: var(--sp-2); flex-wrap: wrap; }
    .spacer { flex: 1; }
    .body { display: grid; gap: var(--sp-3); align-content: start; }
    .idle { color: var(--text-2); max-width: 62ch; }
    .headline { font: var(--fw-semibold) var(--fs-lg) / 1.3 var(--font-display); }
    .cause { display: flex; gap: var(--sp-3); align-items: flex-start; }
    .ic { display: grid; place-items: center; width: 28px; height: 28px; color: var(--text-2); flex: none; }
    .txt { display: grid; gap: var(--sp-2); min-width: 0; }
    .txt > span { color: var(--text-2); font-size: var(--fs-sm); }
    .evidence { margin: 0; color: var(--text-3); font-size: var(--fs-sm); line-height: 1.5; }
    .evidence span + span::before { content: ' · '; }
    .hint { color: var(--text-3); font-size: var(--fs-sm); }
    .actions { display: flex; flex-wrap: wrap; gap: var(--sp-2); padding-top: var(--sp-1); }
    .more { justify-self: start; }
    .others { list-style: none; margin: 0; padding: 0; display: grid; gap: var(--sp-1); font-size: var(--fs-sm); }
    .others li { display: flex; align-items: center; gap: var(--sp-2); }
    .others dy-icon { --icon-size: 14px; color: var(--text-3); }
    .others .t { flex: 1; }
    .faint { color: var(--text-3); }
    .stats { margin: 0; padding-top: var(--sp-3); display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--sp-3); border-top: 1px solid var(--border-subtle); }
    .stats dt { color: var(--text-3); font-size: var(--fs-sm); }
    .stats dd { margin: 2px 0 0; font: var(--fw-semibold) var(--fs-md) / 1.3 var(--font-display); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  `,
})
export class DiagnosisCard {
  protected readonly store = inject(DiagnosisStore);
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
  protected readonly icon = CAUSE_ICON_FN;
  protected readonly conf = formatFraction;
  protected readonly count = formatCount;
  protected readonly showOthers = signal(false);
  protected readonly others = computed(() => this.store.diagnosis()?.causes.slice(1) ?? []);

  protected readonly hintPath = hintPath;
}

function CAUSE_ICON_FN(kind: keyof typeof CAUSE_ICON) {
  return CAUSE_ICON[kind];
}
