import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { ShellContextHost } from '@decryp7/paddock-ui/shell';
import { WORKBENCH_PORTS } from '@decryp7/paddock-ui/workbench';
import type { WorkbenchDemoContext } from '../context';

/** The first screen (a document, priority 100): what the demo shows and where to start. */
@Component({
  selector: 'wd-orientation-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <article>
      <h1>Workbench demo</h1>
      <p>The shared workbench of the Paddock apps: the Explorer on the left, views you arrange in the dock, the command palette, jobs and the assistant.</p>
      <ul class="actions">
        <li><button type="button" (click)="open('src/main.rs')">Open src/main.rs</button></li>
        <li><button type="button" (click)="ports.openPalette('>')">Show all commands</button></li>
        <li><button type="button" (click)="ports.openPalette('')">Go to a file</button></li>
        <li><button type="button" (click)="reindex()">Index the repository</button></li>
      </ul>
    </article>
  `,
  styles: `
    :host { display: block; height: 100%; overflow: auto; container: wd-orientation / inline-size; }
    article { max-width: var(--pd-content-max); padding: var(--pd-space-6); }
    h1 { margin: 0 0 var(--pd-space-3); font-size: var(--pd-text-title); font-weight: var(--pd-weight-semibold); }
    p { margin: 0 0 var(--pd-space-4); color: var(--pd-text-muted); }
    .actions { list-style: none; margin: 0; padding: 0; display: flex; flex-wrap: wrap; gap: var(--pd-space-2); }
    button { height: var(--pd-button-height); padding: 0 var(--pd-space-3); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; cursor: pointer; }
    button:hover { background: var(--pd-hover); }
    button:focus-visible { outline: 2px solid var(--pd-focus-ring); }
  `,
})
export class OrientationView {
  protected readonly ports = inject(WORKBENCH_PORTS);
  private readonly host = inject(ShellContextHost);

  protected async open(path: string): Promise<void> {
    const ctx = (await this.host.load()) as unknown as WorkbenchDemoContext;
    await ctx.openFile(path);
  }

  protected async reindex(): Promise<void> {
    ((await this.host.load()) as unknown as WorkbenchDemoContext).reindex();
  }
}
