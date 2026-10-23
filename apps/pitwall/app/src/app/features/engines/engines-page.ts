import { ChangeDetectionStrategy, Component, OnInit, effect, inject, untracked } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { LiveStore } from '../../core/live.store';
import { injectNow } from '../../shared/now';
import { Panel } from '../../shared/ui/panel';
import { EngineCard } from './engine-card';
import { EngineSetupBanner } from './engine-setup-banner';
import { EnginesStore } from './engines.store';

/** The llama.cpp page (ADR 0020): builds, catalog, pins, self-test and processes. */
@Component({
  selector: 'pw-engines-page',
  imports: [MatIconModule, MatTooltipModule, Panel, EngineCard, EngineSetupBanner],
  providers: [EnginesStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div>
          <h1>llama.cpp</h1>
          <p class="sub">The engine behind every model: pinned releases, verified and self-tested here.</p>
        </div>
        <div class="actions">
          <button type="button" class="btn ghost icon" aria-label="Refresh" matTooltip="Refresh" (click)="store.load()" [disabled]="store.loading()"><mat-icon svgIcon="pw:refresh" aria-hidden="true" /></button>
        </div>
      </header>

      @if (!store.loaded()) {
        <pw-panel heading="llama.cpp" icon="engines" [loading]="store.loading()" [error]="store.error()" [ready]="false" [skeletonLines]="8" (retry)="store.load()" />
      } @else {
        @if (store.error(); as err) {
          <pw-panel heading="llama.cpp" icon="engines" [error]="err" [ready]="false" (retry)="store.load()" />
        }
        <pw-engine-setup />
        @if (store.llama(); as e) {
          <pw-engine-card [engine]="e" [now]="now()" />
        }
      }
    </div>
  `,
  styles: `
    :host {
      display: block;
    }
    .page {
      display: flex;
      flex-direction: column;
      gap: var(--sp-4);
    }
  `,
})
export class EnginesPage implements OnInit {
  protected readonly store = inject(EnginesStore);
  private readonly live = inject(LiveStore);
  protected readonly now = injectNow();

  constructor() {
    // A finished engine install shows up as an installed build: refresh once per download.
    const seen = new Set<string>();
    effect(() => {
      const done = this.live.downloads().filter((d) => d.kind === 'engine' && d.state === 'done' && !seen.has(d.id));
      if (!done.length) return;
      done.forEach((d) => seen.add(d.id));
      untracked(() => void this.store.load());
    });
  }

  ngOnInit(): void {
    void this.store.load();
  }
}
