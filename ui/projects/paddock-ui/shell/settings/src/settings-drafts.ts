import { computed, inject, Injectable, Injector, linkedSignal, untracked, type Signal, type WritableSignal } from '@angular/core';
import { form, validate, type FieldTree } from '@angular/forms/signals';
import type { SettingValue } from '../generated/settings';
import { precheck, sameValue } from './settings-model';
import { SettingsStore } from './settings.store';

/** A section's values as its form edits them. */
export type SectionValues = Record<string, SettingValue>;

/** The form of one section: its model (applied values with the user's edits on top) and what is not saved yet. */
export interface SectionForm {
  readonly id: string;
  /** The Signal Form over `model` (rule 37); each field's errors are the pre-check's or the engine's refusal. */
  readonly tree: FieldTree<SectionValues>;
  readonly model: WritableSignal<SectionValues>;
  /** Keys whose edit differs from the applied value. */
  readonly dirtyKeys: Signal<readonly string[]>;
  /** The unsaved edits: key → value. */
  changes(): SectionValues;
  /** Drops the edit of `key`, or every edit. */
  discard(key?: string): void;
}

/**
 * The unsaved edits of a Settings window, one Signal Form per section, kept while the window is open (moving between
 * pages keeps them). A form's model follows the engine's values (another window's change shows at once) except where the
 * user has an edit. Provided per Settings window with the store.
 */
@Injectable()
export class SettingsDrafts {
  private readonly store = inject(SettingsStore);
  private readonly injector = inject(Injector);
  /** Made once per section and kept for the window's life (a form holds its section's edits). */
  private readonly made = new Map<string, SectionForm>();
  /** One form per loaded section (no signal is written here: the forms are derived from the sections). */
  private readonly forms = computed<ReadonlyMap<string, SectionForm>>(() => {
    const out = new Map<string, SectionForm>();
    for (const s of this.store.sections()) {
      let f = this.made.get(s.id);
      // Building a form is construction, not a dependency: a section's schema is fixed once it is registered.
      if (!f) this.made.set(s.id, (f = untracked(() => this.make(s.id))));
      out.set(s.id, f);
    }
    return out;
  });

  /** Ids of the sections with unsaved edits, in the engine's order. */
  readonly dirtySections = computed(() => [...this.forms().values()].filter((f) => f.dirtyKeys().length > 0).map((f) => f.id));

  /** The form of section `id`, or null while the section is not loaded. */
  formFor(id: string): SectionForm | null {
    return this.forms().get(id) ?? null;
  }

  /** Drops every unsaved edit. */
  discardAll(): void {
    for (const f of this.forms().values()) f.discard();
  }

  private make(id: string): SectionForm {
    const store = this.store;
    const applied = computed<SectionValues>(() => ({ ...(store.section(id)?.values ?? {}) }));
    // The engine's values, keeping the user's edits: a key the user changed keeps its draft until it is applied or dropped.
    const model = linkedSignal<SectionValues, SectionValues>({
      source: applied,
      computation: (now, previous) => {
        if (!previous) return { ...now };
        const out = { ...now };
        for (const [k, v] of Object.entries(previous.value)) if (!sameValue(v, previous.source[k]) && !sameValue(v, now[k])) out[k] = v;
        return out;
      },
    });
    const fields = store.section(id)?.fields ?? [];
    const tree = form(
      model,
      (path) => {
        for (const field of fields) {
          const at = (path as unknown as Record<string, Parameters<typeof validate<SettingValue>>[0]>)[field.key];
          if (!at) continue;
          validate<SettingValue>(at, (ctx) => {
            const value = ctx.value();
            const refused = store.refusalOf(id, field.key, value);
            if (refused) return { kind: 'engine', message: refused };
            const early = precheck(field, value);
            return early ? { kind: 'precheck', message: early } : undefined;
          });
        }
      },
      { injector: this.injector },
    );
    const dirtyKeys = computed(() => {
      const now = applied();
      return Object.entries(model()).filter(([k, v]) => !sameValue(v, now[k])).map(([k]) => k);
    });
    return {
      id,
      tree,
      model,
      dirtyKeys,
      changes: () => Object.fromEntries(dirtyKeys().map((k) => [k, model()[k] as SettingValue])),
      discard: (key?: string) => {
        const now = applied();
        model.update((m) => (key === undefined ? { ...now } : { ...m, [key]: now[key] as SettingValue }));
      },
    };
  }
}
