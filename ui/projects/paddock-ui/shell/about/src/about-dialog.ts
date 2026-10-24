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
            <p class="muted" data-testid="about-version">{{ versionText(a.version) }}</p>
          </header>
          <dl class="facts" [attr.aria-label]="t.build">
            @if (a.commit) {
              <dt>{{ t.commit }}</dt>
              <dd [title]="a.commit">{{ a.commit }}</dd>
            }
            @if (a.built) {
              <dt>{{ t.built }}</dt>
              <dd>{{ a.built }}</dd>
            }
            <dt>{{ t.system }}</dt>
            <dd>{{ a.os }} {{ a.arch }}</dd>
            <dt>{{ t.updates }}</dt>
            <dd>{{ a.updatesAvailable ? t.updatesItself : t.updatesUnavailable }}</dd>
            @if (a.releasesPage) {
              <dt>{{ t.releases }}</dt>
              <dd [title]="a.releasesPage">{{ a.releasesPage }}</dd>
            }
          </dl>
          @if (a.legal.length) {
            <section aria-labelledby="pd-about-legal">
              <h3 id="pd-about-legal">{{ t.legal }}</h3>
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
                  <p role="alert">{{ t.legalFailed }} <button type="button" class="btn" (click)="legal.reload()">{{ t.retry }}</button></p>
                }
                @default {
                  <p class="muted" role="status">{{ t.loading }}</p>
                }
              }
            }
          }
        }
      }
      @case ('error') {
        <p role="alert">{{ t.infoFailed }} <button type="button" class="btn" (click)="info.reload()">{{ t.retry }}</button></p>
      }
      @default {
        <p class="muted" role="status">{{ t.loading }}</p>
      }
    }
  `,
  styles: ABOUT_STYLES,
})
export class PdAboutDialog {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    build: $localize`:@@pd.about.build:Build`,
    commit: $localize`:@@pd.about.commit:Commit`,
    built: $localize`:@@pd.about.built:Built`,
    system: $localize`:@@pd.about.system:System`,
    updates: $localize`:@@pd.about.updates:Updates`,
    updatesItself: $localize`:@@pd.about.updates.itself:Updates itself`,
    updatesUnavailable: $localize`:@@pd.about.updates.unavailable:Updates are not available in this build`,
    releases: $localize`:@@pd.about.releases:Releases`,
    legal: $localize`:@@pd.about.legal:Licences and notices`,
    legalFailed: $localize`:@@pd.about.legal.failed:The text could not be loaded.`,
    infoFailed: $localize`:@@pd.about.failed:The app did not say what it is.`,
    retry: $localize`:@@pd.about.retry:Retry`,
    loading: $localize`:@@pd.about.loading:Loading…`,
  };
  private readonly backend = inject(AboutBackend);
  protected readonly info = resource({ loader: () => this.backend.info() });
  protected readonly selected = signal<string | null>(null);
  protected readonly selectedTitle = computed(() => this.info.value()?.legal.find((d) => d.id === this.selected())?.title ?? '');
  /** Fetched only once a document is chosen. */
  protected readonly legal = resource({
    params: () => this.selected() ?? undefined,
    loader: ({ params }) => this.backend.legal(params),
  });

  protected versionText(version: string): string {
    return $localize`:@@pd.about.version:Version ${version}:version:`;
  }

  protected show(id: string): void {
    this.selected.set(this.selected() === id ? null : id);
  }
}
