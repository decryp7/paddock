import { ChangeDetectionStrategy, Component } from '@angular/core';

/** Placeholder replaced by the storage feature. */
@Component({
  selector: 'pc-storage-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: '<div class="page"><header class="page-header"><h1>Storage</h1></header></div>',
})
export class StoragePage {}
