import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { formatCount } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ANOMALY_ICON, ANOMALY_LABEL } from '../../shared/labels';
import { DiagnosisStore } from '../../state/diagnosis.store';

/**
 * Unusual patterns the engine found in the history (memory growth, periodic spikes, restart loops): a quiet card with a
 * warning bar and a one-line summary (about 48 px), expandable into the list. Nothing when there is none.
 */
@Component({
  selector: 'dy-anomalies-callout',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (store.anomalies().length; as n) {
      <section class="card" role="status" aria-label="Anomalies">
        <div class="bar"></div>
        <div class="head">
          <dy-icon name="trend" />
          <strong>{{ n === 1 ? 'Dyno noticed 1 unusual pattern' : 'Dyno noticed ' + count(n) + ' unusual patterns' }}</strong>
          <span class="kinds truncate">{{ kinds() }}</span>
          <span class="spacer"></span>
          <button type="button" class="btn sm ghost" [attr.aria-expanded]="open()" (click)="open.set(!open())">
            {{ open() ? 'Hide' : 'Show' }} <dy-icon [name]="open() ? 'chevron-down' : 'chevron-right'" />
          </button>
        </div>
        @if (open()) {
          <ul>
            @for (a of store.anomalies(); track a.id) {
              <li>
                <dy-icon [name]="icon(a.kind)" />
                <div><strong>{{ a.title }}</strong><span>{{ a.explanation }}</span></div>
              </li>
            }
          </ul>
        }
      </section>
    }
  `,
  styles: `
    :host { display: block; }
    .card { position: relative; overflow: hidden; padding: 0 var(--sp-4) 0 calc(var(--sp-4) + 3px); }
    .bar { position: absolute; left: 0; top: 0; bottom: 0; width: 3px; background: var(--warn); }
    .head { display: flex; align-items: center; gap: var(--sp-3); min-height: 48px; }
    .head > dy-icon { color: var(--warn); }
    .kinds { min-width: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .spacer { flex: 1; }
    ul { list-style: none; margin: 0; padding: 0 0 var(--sp-3); display: grid; gap: var(--sp-2); border-top: 1px solid var(--border-subtle); padding-top: var(--sp-3); }
    li { display: flex; gap: var(--sp-3); font-size: var(--fs-sm); }
    li > dy-icon { color: var(--text-3); margin-top: 2px; }
    li div { display: grid; }
    li span { color: var(--text-3); }
  `,
})
export class AnomaliesCallout implements OnInit {
  protected readonly store = inject(DiagnosisStore);
  protected readonly open = signal(false);
  protected readonly count = formatCount;
  protected readonly icon = (k: keyof typeof ANOMALY_ICON) => ANOMALY_ICON[k];
  protected readonly label = (k: keyof typeof ANOMALY_LABEL) => ANOMALY_LABEL[k];
  /** The kinds found, once each, as one quiet line ("Memory growth · Restart loop"). */
  protected readonly kinds = computed(() => [...new Set(this.store.anomalies().map((a) => this.label(a.kind)))].join(' · '));

  ngOnInit(): void {
    void this.store.loadAnomalies();
  }
}
