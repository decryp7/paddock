import { ChangeDetectionStrategy, Component } from '@angular/core';
import { StatusView } from './status/status-view';

/** The main window. */
@Component({
  selector: 'pc-root',
  imports: [StatusView],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<main><pc-status-view /></main>`,
})
export class App {}
