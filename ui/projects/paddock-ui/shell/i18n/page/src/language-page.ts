import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { LANGUAGE_CONFIG, languageOptions, LanguageStore, type LanguageChoice } from '@decryp7/paddock-ui/shell/i18n';

/**
 * Settings > Language: one radio per language (System default, English, each shipped translation by its own name, the
 * pseudo-locale where the app ships it). A choice is saved at once; translations load at start-up, so the page then says
 * the window has to restart and offers the restart (`LANGUAGE_CONFIG.reload`: the app relaunches keeping its session; by default this window reloads). `LANGUAGE_CONFIG` is a root token
 * with a default: the app provides it in the Settings window's own providers (the dialog's `providers`), not in the page's, because a
 * default in the root injector is found before anything a lazily loaded page adds.
 */
@Component({
  selector: 'pd-language-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section aria-labelledby="pd-language-title" data-testid="language-page">
      <h2 id="pd-language-title">{{ t.title }}</h2>
      <p class="pd-muted">{{ t.lede }}</p>
    </section>
    <div class="pd-options" role="radiogroup" aria-labelledby="pd-language-title">
      @for (o of options(); track o.value) {
        <label class="pd-option" [class.pd-on]="o.value === store.choice()">
          <input type="radio" name="pd-language" [value]="o.value" [checked]="o.value === store.choice()" [attr.data-testid]="'language-' + o.value" (change)="store.choose(o.value)" />
          <span>{{ o.label }}</span>
        </label>
      }
    </div>
    @if (store.reloadPending()) {
      <div class="pd-restart" role="status" data-testid="language-restart">
        <span>{{ t.restartNeeded }}</span>
        <button type="button" class="pd-btn" data-testid="language-restart-now" (click)="store.reload()">{{ t.restartNow }}</button>
      </div>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-4); color: var(--pd-text); line-height: 1.5; min-width: 0; container-type: inline-size; }
    h2 { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
    p { margin: 0; overflow-wrap: anywhere; }
    .pd-muted { color: var(--pd-text-muted); }
    .pd-options { display: flex; flex-direction: column; gap: var(--pd-space-1); }
    .pd-option { display: flex; align-items: center; gap: var(--pd-space-2); min-height: var(--pd-hit-min); padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-control); cursor: pointer; }
    .pd-option:hover { background: var(--pd-hover); }
    .pd-option.pd-on { background: var(--pd-accent-tint); font-weight: var(--pd-weight-semibold); }
    .pd-option input { accent-color: var(--pd-accent-fill); }
    .pd-option:focus-within { outline: 2px solid var(--pd-accent); outline-offset: 1px; }
    .pd-restart { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-surface-2); }
    .pd-restart span { flex: 1 1 14rem; min-width: 0; }
    .pd-btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-accent-fill); background: var(--pd-accent-fill); color: var(--pd-on-accent); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-btn:focus-visible { outline: 2px solid var(--pd-accent); outline-offset: 1px; }
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
