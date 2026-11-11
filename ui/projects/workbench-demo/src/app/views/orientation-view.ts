import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { ShellContextHost } from '@decryp7/paddock-ui/shell/contributions';
import { formatKey, Keymap } from '@decryp7/paddock-ui/shell/menu';
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
      <button type="button" class="pd-btn pd-btn--primary" (click)="open('src/main.rs')">Open src/main.rs</button>
      <ul class="start" aria-label="More ways to start">
        <li><button type="button" class="pd-btn pd-btn--ghost" (click)="ports.openPalette('')">Go to a file</button><kbd class="pd-kbd">{{ keys.goTo }}</kbd></li>
        <li><button type="button" class="pd-btn pd-btn--ghost" (click)="ports.openPalette('>')">Show all commands</button><kbd class="pd-kbd">{{ keys.commands }}</kbd></li>
        <li><button type="button" class="pd-btn pd-btn--ghost" (click)="reindex()">Index the repository</button></li>
      </ul>
    </article>
  `,
  styles: `
    :host { display: block; height: 100%; overflow: auto; container: wd-orientation / inline-size; }
    article { max-width: var(--pd-content-max); padding: var(--pd-space-6); }
    h1 { margin: 0 0 var(--pd-space-3); font-size: var(--pd-text-title); font-weight: var(--pd-weight-semibold); }
    p { margin: 0 0 var(--pd-space-4); color: var(--pd-text-muted); }
    .start { list-style: none; margin: var(--pd-space-4) 0 0; padding: 0; display: flex; flex-direction: column; align-items: flex-start; gap: var(--pd-space-1); }
    .start li { display: flex; align-items: center; gap: var(--pd-space-2); }
  `,
})
export class OrientationView {
  protected readonly ports = inject(WORKBENCH_PORTS);
  private readonly keymap = inject(Keymap);
  /** The keys of the two palette commands, as this platform writes them. */
  protected readonly keys = {
    goTo: formatKey(this.keymap.keyFor('pd.wb.palette.quick-open') ?? 'Mod+P', this.keymap.platform),
    commands: formatKey(this.keymap.keyFor('pd.wb.palette.commands') ?? 'Mod+Shift+P', this.keymap.platform),
  };
  private readonly host = inject(ShellContextHost);

  protected async open(path: string): Promise<void> {
    const ctx = (await this.host.load()) as unknown as WorkbenchDemoContext;
    await ctx.openFile(path);
  }

  protected async reindex(): Promise<void> {
    ((await this.host.load()) as unknown as WorkbenchDemoContext).reindex();
  }
}
