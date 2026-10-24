import { ChangeDetectionStrategy, Component, inject, Injectable, signal, viewChild } from '@angular/core';
import { PdView, VIEW_HOST, sampleViewDoc, type ViewAction, type ViewConfirmRequest, type ViewHost, type ViewOpenRequest, type ViewPickRequest } from '@decryp7/paddock-ui/view';

interface Asking {
  readonly text: string;
  readonly yes: string;
  resolve(ok: boolean): void;
}

/**
 * The gallery's stand-in for an app's host: in an app, confirmations are OS windows (rule 17) and the pickers are native dialogs; here
 * the question is a small panel in the page so the flow can be seen and tested without a window.
 */
@Injectable()
export class GalViewHost implements ViewHost {
  readonly asking = signal<Asking | null>(null);
  readonly opened = signal<string[]>([]);

  private ask(text: string, yes: string): Promise<boolean> {
    return new Promise((resolve) => this.asking.set({ text, yes, resolve: (ok) => (this.asking.set(null), resolve(ok)) }));
  }
  confirm(r: ViewConfirmRequest): Promise<boolean> {
    return this.ask(r.question, r.action.label);
  }
  async openInBrowser(r: ViewOpenRequest): Promise<void> {
    if (await this.ask(`Open ${r.url} in your browser?`, 'Open')) this.opened.update((o) => [...o, r.url]);
  }
  async pickPath(r: ViewPickRequest): Promise<string> {
    return r.folder ? 'grant-folder-1' : 'grant-file-1';
  }
  async pickSecret(): Promise<string> {
    return 'secret-ref-1';
  }
}

/** `<pd-view>` (plan X51, ADR 0002) with a sample document of every node kind, an action log and the host's confirmation. */
@Component({
  selector: 'gal-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdView],
  providers: [GalViewHost, { provide: VIEW_HOST, useExisting: GalViewHost }],
  template: `
    <section class="gal-section" data-testid="view" aria-labelledby="h-view">
      <h2 id="h-view">Declarative view</h2>
      <p>A plugin sends data; the app draws it. Every node kind below comes from one sample document; what you do leaves as an action, and the plugin answers with a patch.</p>
      <div class="gal-row" role="group" aria-label="Plugin answers">
        <button type="button" class="gal-btn" data-testid="view-patch" (click)="patchCaption()">Patch the caption</button>
        <button type="button" class="gal-btn" data-testid="view-stale" (click)="stale()">Send a stale patch</button>
        <button type="button" class="gal-btn" data-testid="view-bad" (click)="bad()">Send an invalid view</button>
        <button type="button" class="gal-btn" data-testid="view-reload" (click)="reload()">Reload</button>
      </div>
      @if (host.asking(); as q) {
        <div class="gal-box gal-pad" role="alertdialog" aria-label="Confirm" data-testid="view-confirm">
          <p>{{ q.text }}</p>
          <div class="gal-row">
            <button type="button" class="gal-btn gal-btn--primary" data-testid="view-confirm-yes" (click)="q.resolve(true)">{{ q.yes }}</button>
            <button type="button" class="gal-btn" data-testid="view-confirm-no" (click)="q.resolve(false)">Cancel</button>
          </div>
        </div>
      }
      <p data-testid="view-log" aria-live="polite">Last action: {{ last() }}. Opened: {{ host.opened().length ? host.opened().join(', ') : 'nothing' }}.</p>
      <div class="gal-box gal-pad" data-testid="view-box">
        <pd-view view="gallery.release" [doc]="doc()" [busy]="host.asking() !== null" (action)="record($event)" (retry)="reload()" />
      </div>
    </section>
  `,
  styles: `
    :host { display: block; }
    .gal-pad { padding: var(--pd-space-4); }
  `,
})
export class GalView {
  protected readonly host = inject(GalViewHost);
  private readonly view = viewChild.required(PdView);
  protected readonly doc = signal<unknown>(sampleViewDoc(1));
  protected readonly last = signal('none');
  private etag = 1;

  protected record(a: ViewAction): void {
    this.last.set(`${a.action} ${JSON.stringify(a.args)}`);
  }
  protected patchCaption(): void {
    const base = this.etag;
    this.etag += 1;
    this.view().applyPatch({ replace: { baseEtag: base, etag: this.etag, id: 'cap', node: { id: 'cap', kind: { text: { text: `Updated at revision ${this.etag}`, tone: 'good', style: 'caption' } } } } });
  }
  protected stale(): void {
    this.view().applyPatch({ replace: { baseEtag: 999, etag: 1000, id: 'cap', node: { id: 'cap', kind: { text: { text: 'never shown', tone: 'neutral', style: 'caption' } } } } });
  }
  protected bad(): void {
    this.doc.set({ ...sampleViewDoc(this.etag + 1), schema: 2 });
  }
  protected reload(): void {
    this.etag = 1;
    this.doc.set(sampleViewDoc(1));
  }
}
