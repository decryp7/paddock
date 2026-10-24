import { ChangeDetectionStrategy, Component, computed, inject, resource, signal } from '@angular/core';
import { AboutBackend } from './backend';
import { ABOUT_STYLES } from './styles';

/**
 * About (a viewer: modeless, grows only; the dialog shell adds Close). The version is the engine's (rule 6), and each
 * legal text is fetched from the engine only when its button is pressed, so none of them is in the UI's JavaScript.
 */
@Component({
  selector: 'pd-about-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @switch (info.status()) {
      @case ('resolved') {
        @if (info.value(); as a) {
          <header>
            <h2>{{ a.appName }}</h2>
            <p class="muted" data-testid="about-version">Version {{ a.version }}</p>
          </header>
          <dl class="facts" aria-label="Build">
            @if (a.commit) {
              <dt>Commit</dt>
              <dd [title]="a.commit">{{ a.commit }}</dd>
            }
            @if (a.built) {
              <dt>Built</dt>
              <dd>{{ a.built }}</dd>
            }
            <dt>System</dt>
            <dd>{{ a.os }} {{ a.arch }}</dd>
            <dt>Updates</dt>
            <dd>{{ a.updatesAvailable ? 'Updates itself' : 'Updates are not available in this build' }}</dd>
            @if (a.releasesPage) {
              <dt>Releases</dt>
              <dd [title]="a.releasesPage">{{ a.releasesPage }}</dd>
            }
          </dl>
          @if (a.legal.length) {
            <section aria-labelledby="pd-about-legal">
              <h3 id="pd-about-legal">Licences and notices</h3>
              <div class="row">
                @for (doc of a.legal; track doc.id) {
                  <button type="button" class="btn" [attr.aria-pressed]="selected() === doc.id" (click)="show(doc.id)">{{ doc.title }}</button>
                }
              </div>
            </section>
            @if (selected()) {
              @switch (legal.status()) {
                @case ('resolved') {
                  <pre class="text" tabindex="0" [attr.aria-label]="selectedTitle()" data-testid="about-legal">{{ legal.value() }}</pre>
                }
                @case ('error') {
                  <p role="alert">The text could not be loaded. <button type="button" class="btn" (click)="legal.reload()">Retry</button></p>
                }
                @default {
                  <p class="muted" role="status">Loading…</p>
                }
              }
            }
          }
        }
      }
      @case ('error') {
        <p role="alert">The app did not say what it is. <button type="button" class="btn" (click)="info.reload()">Retry</button></p>
      }
      @default {
        <p class="muted" role="status">Loading…</p>
      }
    }
  `,
  styles: ABOUT_STYLES,
})
export class PdAboutDialog {
  private readonly backend = inject(AboutBackend);
  protected readonly info = resource({ loader: () => this.backend.info() });
  protected readonly selected = signal<string | null>(null);
  protected readonly selectedTitle = computed(() => this.info.value()?.legal.find((d) => d.id === this.selected())?.title ?? '');
  /** Fetched only once a document is chosen. */
  protected readonly legal = resource({
    params: () => this.selected() ?? undefined,
    loader: ({ params }) => this.backend.legal(params),
  });

  protected show(id: string): void {
    this.selected.set(this.selected() === id ? null : id);
  }
}
