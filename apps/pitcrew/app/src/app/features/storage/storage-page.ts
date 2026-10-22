import { ChangeDetectionStrategy, Component } from '@angular/core';
import { CacheSection } from './cache-section';
import { EngineSection } from './engine-section';

/** Storage: what the container engine and the Actions cache hold, and how to reclaim it. */
@Component({
  selector: 'pc-storage-page',
  imports: [EngineSection, CacheSection],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: `
    .page {
      display: grid;
      gap: var(--space-8);
    }
  `,
  template: `
    <div class="page">
      <header class="page-header">
        <h1>Storage</h1>
      </header>
      <pc-engine-section />
      <pc-cache-section />
    </div>
  `,
})
export class StoragePage {}
