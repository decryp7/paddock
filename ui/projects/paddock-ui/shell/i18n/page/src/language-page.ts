import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatRadioButton, MatRadioGroup } from '@angular/material/radio';
import { LANGUAGE_CONFIG, languageOptions, LanguageStore, type LanguageChoice } from '@decryp7/paddock-ui/shell/i18n';

/**
 * Settings > Language: one radio per language (System default, English, each shipped translation by its own name, the
 * pseudo-locale where the app ships it). A choice is saved at once; translations load at start-up, so the page then says
 * the window has to restart and offers the restart (`LANGUAGE_CONFIG.reload`: the app relaunches keeping its session; by default this window reloads). `LANGUAGE_CONFIG` is a root token
 * with a default: the app provides it in the Settings window's own providers (the dialog's `providers`), not in the page's, because a
 * default in the root injector is found before anything a lazily loaded page adds; the page builds its own `LanguageStore`.
 */
@Component({
  selector: 'pd-language-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  // Its own store: built from the `LANGUAGE_CONFIG` this page sees (the Settings window's providers), not the root's one.
  imports: [MatButton, MatRadioButton, MatRadioGroup],
  providers: [LanguageStore],
  template: `
    <section aria-labelledby="pd-language-title" data-testid="language-page">
      <h2 id="pd-language-title">{{ t.title }}</h2>
      <p class="pd-muted">{{ t.lede }}</p>
    </section>
    <mat-radio-group class="pd-options" aria-labelledby="pd-language-title" name="pd-language" [value]="store.choice()" (change)="store.choose($event.value)">
      @for (o of options(); track o.value) {
        <mat-radio-button class="pd-option" [class.pd-on]="o.value === store.choice()" [value]="o.value" [attr.data-testid]="'language-' + o.value">{{ o.label }}</mat-radio-button>
      }
    </mat-radio-group>
    @if (store.reloadPending()) {
      <div class="pd-restart" role="status" data-testid="language-restart">
        <span>{{ t.restartNeeded }}</span>
        <button matButton="filled" type="button" data-testid="language-restart-now" (click)="store.reload()">{{ t.restartNow }}</button>
      </div>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
    h2 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
    p { margin: 0; overflow-wrap: anywhere; }
    .pd-muted { color: var(--pd-text-muted); }
    .pd-options { display: flex; flex-direction: column; gap: var(--pd-space-1); }
    .pd-option { min-height: var(--pd-hit-min); padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); }
    .pd-option:hover { background: var(--pd-hover); }
    .pd-option.pd-on { background: var(--pd-accent-tint); font-weight: var(--pd-weight-semibold); }
    .pd-restart { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); }
    .pd-restart span { flex: 1 1 14rem; min-width: 0; }
  `,
})
export class PdLanguagePage {
  protected readonly store = inject(LanguageStore);
  private readonly config = inject(LANGUAGE_CONFIG);

  protected readonly t = {
    title: $localize`:@@pd.language.title:Language`,
    lede: $localize`:@@pd.language.lede:The language of the app’s menus, dialogs and messages. Names of files, logs and other data are never translated.`,
    restartNeeded: $localize`:@@pd.language.restart-needed:The new language applies after the window restarts.`,
    restartNow: $localize`:@@pd.language.restart-now:Restart now`,
    system: $localize`:@@pd.language.system:System default`,
    english: $localize`:@@pd.language.english:English`,
    pseudo: $localize`:@@pd.language.pseudo:Pseudo-locale (for testing)`,
  };

  protected readonly options = computed<{ value: LanguageChoice; label: string }[]>(() =>
    languageOptions(this.config.shipped, { system: this.t.system, english: this.t.english, pseudo: this.t.pseudo }).map((o) => ({ value: o.value, label: o.label })),
  );
}
