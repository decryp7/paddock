import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { CacheSection } from './cache-section';
import { EngineSection } from './engine-section';
import { EngineStore } from './engine.store';
import { ENGINE_NAMES } from './storage-labels';

/** Storage: what the container engine and the Actions cache hold, and how to reclaim it. */
@Component({
  selector: 'pc-storage-page',
  imports: [EngineSection, CacheSection, MatIcon],
  providers: [EngineStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: `
    .page {
      display: grid;
      gap: var(--space-8);
    }
    .toolbar {
      display: flex;
      align-items: center;
      gap: var(--space-2);
    }
  `,
  template: `
    <div class="page">
      <header class="page-header">
        <div>
          <h1>Storage</h1>
          <p class="subtitle">What the container engine and the Actions cache hold, and how to reclaim it</p>
        </div>
        <div class="toolbar">
          @if (engineName(); as name) {
            <span class="chip" data-testid="engine-name">{{ name }}</span>
          }
          <button type="button" class="btn icon ghost" aria-label="Refresh" title="Refresh" data-testid="refresh" (click)="store.load()" [disabled]="store.report().loading">
            <mat-icon svgIcon="pc:refresh" aria-hidden="true" [class.spin]="store.report().loading" />
          </button>
          <button type="button" class="btn primary" data-testid="clean-up" (click)="cleanUp()" [disabled]="!store.usable()">Clean up...</button>
        </div>
      </header>
      <pc-engine-section />
      <pc-cache-section />
    </div>
  `,
})
export class StoragePage {
  protected readonly store = inject(EngineStore);
  protected readonly engineName = computed(() => {
    const e = this.store.report().value?.engine;
    return e ? ENGINE_NAMES[e] : '';
  });

  /** Scrolls to the prune section and puts the focus on its heading. */
  protected cleanUp(): void {
    const heading = document.getElementById('prune-title');
    heading?.scrollIntoView?.({ block: 'start', behavior: 'smooth' });
    heading?.focus();
  }
}
