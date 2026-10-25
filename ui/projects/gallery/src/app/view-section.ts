import { ChangeDetectionStrategy, Component, inject, Injectable, signal, viewChild } from '@angular/core';
import { PdView, PdViewWrites, VIEW_HOST, VIEW_OPEN_AI_SETTINGS, sampleViewDoc, type GrantRef, type SecretRef, type ViewAction, type ViewConfirmRequest, type ViewHost, type ViewOpenRequest, type ViewPicked, type ViewPickRequest } from '@decryp7/paddock-ui/view';

interface Asking {
  readonly text: string;
  readonly yes: string;
  readonly writes: boolean;
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
  /** How many times the sample app's "Open AI settings" was chosen. */
  readonly aiSettings = signal(0);

  private ask(text: string, yes: string, writes = false): Promise<boolean> {
    return new Promise((resolve) => this.asking.set({ text, yes, writes, resolve: (ok) => (this.asking.set(null), resolve(ok)) }));
  }
  confirm(r: ViewConfirmRequest): Promise<boolean> {
    return this.ask(r.question, r.action.label, r.writes);
  }
  async openInBrowser(r: ViewOpenRequest): Promise<void> {
    if (await this.ask(`Open ${r.url} in your browser?`, 'Open')) this.opened.update((o) => [...o, r.url]);
  }
  async pickPath(r: ViewPickRequest): Promise<ViewPicked<GrantRef>> {
    return r.folder ? { value: 'grant-folder-1', name: '/Users/sample/Projects/paddock/target/release/bundle' } : { value: 'grant-file-1', name: '/Users/sample/notes.md' };
  }
  async pickSecret(): Promise<ViewPicked<SecretRef>> {
    return { value: 'secret-ref-1', name: 'release-publish-token' };
  }
}

/** `<pd-view>` (plan X51, ADR 0002) with a sample document of every node kind, an action log and the host's confirmation. */
@Component({
  selector: 'gal-view',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdView, PdViewWrites],
  providers: [
    GalViewHost,
    { provide: VIEW_HOST, useExisting: GalViewHost },
    { provide: VIEW_OPEN_AI_SETTINGS, useFactory: () => { const h = inject(GalViewHost); return () => h.aiSettings.update((n) => n + 1); } },
  ],
  template: `
    <section class="gal-section" data-testid="view" aria-labelledby="h-view">
      <h2 id="h-view">Declarative view</h2>
      <p>A plugin sends data; the app draws it. Every node kind below comes from one sample document; what you do leaves as an action, and the plugin answers with a patch.</p>
      <div class="gal-row" role="group" aria-label="Plugin answers">
        <button type="button" class="gal-btn" data-testid="view-patch" (click)="patchCaption()">Patch the caption</button>
        <button type="button" class="gal-btn" data-testid="view-stale" (click)="stale()">Send a stale patch</button>
        <button type="button" class="gal-btn" data-testid="view-bad" (click)="bad()">Send an invalid view</button>
        <button type="button" class="gal-btn" data-testid="view-reload" (click)="reload()">Reload</button>
        <button type="button" class="gal-btn" data-testid="view-load-demo" (click)="load()">Show loading</button>
        <button type="button" class="gal-btn" data-testid="view-busy-demo" [attr.aria-pressed]="working()" (click)="working.set(!working())">Show busy</button>
      </div>
      @if (host.asking(); as q) {
        <div class="gal-box gal-pad" role="alertdialog" aria-label="Confirm" data-testid="view-confirm">
          <p>{{ q.text }}</p>
          <div class="gal-row">
            <button type="button" class="gal-btn" data-testid="view-confirm-no" (click)="q.resolve(false)">Cancel</button>
            <button type="button" class="gal-btn gal-btn--primary" data-testid="view-confirm-yes" (click)="q.resolve(true)">{{ q.yes }}@if (q.writes) { <pd-view-writes /> }</button>
          </div>
        </div>
      }
      <p data-testid="view-log" aria-live="polite">Last action: {{ last() }}. Opened: {{ host.opened().length ? host.opened().join(', ') : 'nothing' }}. AI settings opened: {{ host.aiSettings() }}.</p>
      <div class="gal-box gal-pad" data-testid="view-box">
        @for (key of [mounted()]; track key) {
          <pd-view view="gallery.release" plugin="Release checks" logsLink [doc]="doc()" [loading]="loading()" [busy]="host.asking() !== null || working()" (action)="record($event)" (retry)="reload()" />
        }
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
  protected readonly loading = signal(false);
  /** Changing it draws the view again from nothing (the loading state is the first thing a view shows). */
  protected readonly mounted = signal(0);
  protected readonly working = signal(false);
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
    this.loading.set(false);
    this.doc.set(sampleViewDoc(1));
  }
  /** The host is fetching the view and has no document yet: the loading skeleton. */
  protected load(): void {
    this.doc.set(null);
    this.loading.set(true);
    this.mounted.update((n) => n + 1);
  }
}
