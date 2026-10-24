import { ChangeDetectionStrategy, Component, computed, inject, InjectionToken, input } from '@angular/core';
import { FormField } from '@angular/forms/signals';
import type { SettingsPageDef } from '@decryp7/paddock-ui/shell';
import { type SettingField, SettingsStore } from '@decryp7/paddock-ui/shell/settings';
import { PdSettingField } from './setting-field';
import { SettingsDrafts } from './settings-drafts';

/** The settings page a page component is shown for (its element injector has it). */
export const SETTINGS_PAGE = new InjectionToken<SettingsPageDef>('PADDOCK_SETTINGS_PAGE');

/**
 * The generic form of one engine section (`paddock-settings`): its title, description and fields rendered from the
 * schema, Reset section, and the advanced fields behind "Show advanced". A page shows it with
 * `<pd-settings-section section="pw.models" />`, alone or with its own content around it.
 */
@Component({
  selector: 'pd-settings-section',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdSettingField, FormField],
  host: { '[attr.data-section]': 'section()' },
  template: `
    @if (view(); as s) {
      @if (heading()) {
        <div class="pd-head">
          <h2 class="pd-title" [id]="headingId()">{{ s.title }}</h2>
          <span class="pd-spacer"></span>
          @if (s.fields.length && !keys()) {
            <button type="button" class="pd-btn" (click)="resetSection()">{{ t.resetSection }}</button>
          }
        </div>
        @if (s.description) {
          <p class="pd-desc">{{ s.description }}</p>
        }
      }
      @for (e of store.sectionRefusals(s.id); track $index) {
        <p class="pd-error" role="alert">{{ e }}</p>
      }
      @if (form(); as f) {
        <div class="pd-fields" role="group" [attr.aria-labelledby]="heading() ? headingId() : null" [attr.aria-label]="heading() ? null : s.title">
          @for (field of fields(); track field.key) {
            <pd-setting-field [formField]="$any(f.tree)[field.key]" [setting]="field" (commit)="commit()" (resetToDefault)="resetField(field.key)" />
          }
        </div>
      }
      @if (hiddenAdvanced(); as n) {
        <p class="pd-hidden" data-testid="settings-hidden-advanced">
          {{ hiddenText(n) }}
          <button type="button" class="pd-link" (click)="store.setShowAdvanced(true)">{{ t.showThem }}</button>
        </p>
      }
      @if (!s.fields.length) {
        <p class="pd-muted">{{ t.empty }}</p>
      }
    } @else if (store.isLoaded()) {
      <p class="pd-muted" role="status">{{ t.unavailable }}</p>
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; min-width: 0; }
    .pd-head { display: flex; align-items: center; gap: var(--pd-space-2); flex-wrap: wrap; }
    .pd-title { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
    .pd-spacer { flex: 1; }
    .pd-desc { margin: var(--pd-space-1) 0 var(--pd-space-2); color: var(--pd-text-muted); line-height: 1.45; }
    .pd-fields { display: flex; flex-direction: column; min-width: 0; }
    .pd-btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); font: inherit; cursor: pointer; }
    .pd-btn:focus-visible, .pd-link:focus-visible { outline: 2px solid var(--pd-accent); outline-offset: 1px; }
    .pd-muted { color: var(--pd-text-muted); }
    .pd-error { margin: var(--pd-space-1) 0; color: var(--pd-danger-text, var(--pd-text)); }
    /* The line may wrap and is never clipped: it sits below the fields at its full height. */
    .pd-hidden { flex: none; margin: var(--pd-space-3) 0 0; padding-block: var(--pd-space-1); color: var(--pd-text-muted); line-height: var(--pd-lh-body); overflow-wrap: anywhere; }
    .pd-link { border: 0; padding: 0; background: none; color: var(--pd-text); font: inherit; text-decoration: underline; cursor: pointer; }
  `,
})
export class PdSettingsSection {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    resetSection: $localize`:@@pd.settings.section.reset:Reset section`,
    showThem: $localize`:@@pd.settings.section.show-them:Show them`,
    empty: $localize`:@@pd.settings.section.empty:This section has no settings.`,
    unavailable: $localize`:@@pd.settings.section.unavailable:These settings are not available in this app.`,
  };
  protected hiddenText(n: number): string {
    return n === 1 ? $localize`:@@pd.settings.section.hidden.one:1 advanced setting is hidden.` : $localize`:@@pd.settings.section.hidden.other:${n}:count: advanced settings are hidden.`;
  }
  protected readonly store = inject(SettingsStore);
  private readonly drafts = inject(SettingsDrafts);
  /** The engine section's id. */
  readonly section = input.required<string>();
  /** Only these fields (search results); null: every field, advanced ones behind "Show advanced". */
  readonly keys = input<readonly string[] | null>(null);
  /** Shows the title, description and Reset section. */
  readonly heading = input(true);

  protected readonly view = computed(() => this.store.section(this.section()) ?? null);
  protected readonly form = computed(() => this.drafts.formFor(this.section()));
  protected readonly headingId = computed(() => `pd-settings-section-${this.section().replace(/[^A-Za-z0-9_-]/g, '-')}`);

  /** Advanced fields show when asked, when searched for, or while they hold an edit or an error. */
  protected readonly fields = computed<readonly SettingField[]>(() => {
    const s = this.view();
    if (!s) return [];
    const keys = this.keys();
    if (keys) return s.fields.filter((f) => keys.includes(f.key));
    if (this.store.showAdvanced()) return s.fields;
    const dirty = this.form()?.dirtyKeys() ?? [];
    return s.fields.filter((f) => !f.advanced || dirty.includes(f.key) || this.store.refusalOf(s.id, f.key, this.form()?.model()[f.key]) !== null);
  });

  protected readonly hiddenAdvanced = computed(() => (this.keys() ? 0 : (this.view()?.fields.length ?? 0) - this.fields().length));

  /**
   * Applies the section's edits that have no error: one that fails the pre-check, or that the engine refused and is
   * unchanged since, stays an unsaved edit (the engine lays the rest over the current values).
   */
  protected commit(): void {
    const f = this.form();
    if (!f) return;
    const tree = f.tree as unknown as Record<string, (() => { errors(): readonly unknown[] }) | undefined>;
    const ready = Object.entries(f.changes()).filter(([k]) => !tree[k]?.().errors().length);
    if (ready.length) void this.store.apply(this.section(), Object.fromEntries(ready));
  }

  protected resetField(key: string): void {
    this.form()?.discard(key);
    void this.store.reset(this.section(), key);
  }

  protected resetSection(): void {
    this.form()?.discard();
    void this.store.reset(this.section());
  }
}

/** The page component of a page that shows one engine section: `load: () => import(...).then((m) => m.PdSettingsSectionPage)`. */
@Component({
  selector: 'pd-settings-section-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdSettingsSection],
  template: `<pd-settings-section [section]="page.id" />`,
})
export class PdSettingsSectionPage {
  protected readonly page = inject(SETTINGS_PAGE);
}
