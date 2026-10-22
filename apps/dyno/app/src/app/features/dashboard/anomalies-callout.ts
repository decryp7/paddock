import { ChangeDetectionStrategy, Component, OnInit, inject } from '@angular/core';
import { formatCount } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ANOMALY_ICON, ANOMALY_LABEL } from '../../shared/labels';
import { DiagnosisStore } from '../../state/diagnosis.store';

/** A banner when the engine found something odd in the history (memory growth, periodic spikes, restart loops). Nothing when there is none. */
@Component({
  selector: 'dy-anomalies-callout',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (store.anomalies().length; as n) {
      <section class="callout" role="status" aria-label="Anomalies">
        <span class="ic"><dy-icon name="trend" /></span>
        <div class="txt">
          <strong>{{ n === 1 ? 'Dyno noticed something unusual' : 'Dyno noticed ' + count(n) + ' unusual patterns' }}</strong>
          <ul>
            @for (a of store.anomalies().slice(0, 3); track a.id) {
              <li [title]="a.explanation">
                <dy-icon [name]="icon(a.kind)" /><span><b>{{ label(a.kind) }}:</b> {{ a.title }}</span>
              </li>
            }
          </ul>
        </div>
      </section>
    }
  `,
  styles: `
    :host { display: block; }
    .callout { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-3) var(--sp-4); border: 1px solid color-mix(in srgb, var(--warn) 40%, transparent); border-radius: var(--r-lg); background: var(--warn-soft); }
    .ic { display: grid; place-items: center; width: 32px; height: 32px; border-radius: var(--r-md); background: var(--surface-2); color: var(--warn); flex: none; }
    .txt { flex: 1; min-width: 0; display: grid; gap: 2px; }
    ul { list-style: none; margin: 0; padding: 0; display: grid; gap: 1px; font-size: var(--fs-sm); color: var(--text-2); }
    li dy-icon { --icon-size: 13px; vertical-align: -2px; }
    li { display: grid; grid-template-columns: auto minmax(0, 1fr); align-items: baseline; column-gap: 6px; }
    .why { color: var(--text-3); }
    b { font-weight: var(--fw-medium); color: var(--text-1); }
  `,
})
export class AnomaliesCallout implements OnInit {
  protected readonly store = inject(DiagnosisStore);
  protected readonly icon = (k: keyof typeof ANOMALY_ICON) => ANOMALY_ICON[k];
  protected readonly count = formatCount;
  protected readonly label = (k: keyof typeof ANOMALY_LABEL) => ANOMALY_LABEL[k];

  ngOnInit(): void {
    void this.store.loadAnomalies();
  }
}
