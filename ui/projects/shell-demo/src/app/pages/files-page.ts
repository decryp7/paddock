import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FilesBackend, FilesStore, MemoryFilesBackend, PdRecentFiles } from '@decryp7/paddock-ui/shell/files';
import { DemoPage, SECTION_STYLES } from './demo-page';

/** The Files page: the shell's pickers, recent list, reveal and web addresses over the demo's in-memory engine (or paddock-files in the desktop build). */
@Component({
  selector: 'sd-files-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DemoPage, PdRecentFiles],
  template: `
    <sd-page heading="Files" subtitle="Only files you choose are opened. The engine checks every path again.">
      <section aria-labelledby="sd-files-open">
        <h2 id="sd-files-open">Open</h2>
        <div class="commands" role="group" aria-labelledby="sd-files-open">
          <button type="button" class="cmd primary" data-testid="files-pick" (click)="pick()">Open a file…</button>
          <button type="button" class="cmd" data-testid="files-folder" (click)="store.openAppFolder('logs')">Show the log folder</button>
          <button type="button" class="cmd" data-testid="files-url" (click)="store.openUrl('https://example.org/docs')">Open the documentation</button>
        </div>
        @if (memory) {
          <div class="commands demo" role="group" aria-label="Demo controls">
            <span class="context">Demo controls</span>
            <button type="button" class="cmd" data-testid="files-drop" (click)="memory.drop('/demo/dropped/trace.log')">Simulate a drop</button>
            <button type="button" class="cmd" data-testid="files-forge" (click)="forge()">Try an unchosen path</button>
          </div>
        }
        <p class="context" aria-live="polite" data-testid="files-last">{{ last() }}</p>
        @if (store.dropped().length > 0) {
          <p class="context" data-testid="files-dropped">
            Dropped on this window: {{ store.dropped().join(', ') }}
            <button type="button" class="cmd" data-testid="files-take" (click)="take()">Open them</button>
          </p>
        }
        @if (store.failure(); as failure) {
          <p class="context failure" role="alert" data-testid="files-failure">{{ failure.message }}</p>
        }
      </section>
      <section aria-labelledby="sd-files-recent">
        <h2 id="sd-files-recent">Recent files</h2>
        <pd-recent-files list="files" emptyText="No recent files yet. Open one above." (choose)="last.set('Opened ' + $event)" />
      </section>
    </sd-page>
  `,
  styles: SECTION_STYLES + `.failure { color: var(--pd-danger); } .primary { background: var(--pd-accent-fill); border-color: var(--pd-accent-fill); color: var(--pd-on-accent); } .primary:hover:not(:disabled) { background: var(--pd-accent-fill); filter: brightness(1.08); } .demo { align-items: center; }`,
})
export class FilesPage {
  protected readonly store = inject(FilesStore);
  private readonly backend = inject(FilesBackend);
  protected readonly memory = this.backend instanceof MemoryFilesBackend ? this.backend : null;
  protected readonly last = signal('Nothing opened yet.');

  protected async pick(): Promise<void> {
    const [path] = await this.store.pick({ kind: 'openFile', title: 'Open a log', filters: [{ name: 'Log files', extensions: ['log', 'txt'] }] });
    this.last.set(path ? `Chose ${path}` : 'The dialog was closed.');
  }

  /** The page opens what was dropped on the window like a chosen file. */
  protected take(): void {
    this.last.set(`Opened ${this.store.takeDropped().join(', ')}`);
  }

  /** What a compromised page could try: reveal a path nobody chose. The engine refuses. */
  protected async forge(): Promise<void> {
    await this.store.reveal('/etc/hosts');
  }
}
