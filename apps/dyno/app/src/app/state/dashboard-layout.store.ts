import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { DashboardLayout, Widget, WidgetKind, WidgetMetric, WidgetSize } from '../core/api.types';
import { Notifier } from '../shared/notifier';
import { MAX_WIDGETS, applyPreset, moveWidget, newWidget, removeWidget, resizeWidget } from '../features/widgets/widget-model';
import { SettingsStore } from './settings.store';

interface LayoutState {
  /** The widgets being edited, or `null` when not editing. */
  draft: Widget[] | null;
}

const EMPTY: DashboardLayout = { version: 1, widgets: [], presets: [] };

/** The customisable dashboard: the saved layout (from Settings) and the edit session on top of it. Saving goes through Settings. */
export const DashboardLayoutStore = signalStore(
  { providedIn: 'root' },
  withState<LayoutState>({ draft: null }),
  withComputed((s, settings = inject(SettingsStore)) => {
    const layout = computed(() => settings.settings()?.dashboard ?? EMPTY);
    return {
      layout,
      editing: computed(() => s.draft() !== null),
      widgets: computed(() => s.draft() ?? layout().widgets),
      presets: computed(() => layout().presets),
      dirty: computed(() => s.draft() !== null && JSON.stringify(s.draft()) !== JSON.stringify(layout().widgets)),
      canAdd: computed(() => (s.draft()?.length ?? 0) < MAX_WIDGETS),
    };
  }),
  withMethods((store, settings = inject(SettingsStore), notifier = inject(Notifier)) => {
    const edit = (f: (w: Widget[]) => Widget[]) => patchState(store, (s) => (s.draft ? { draft: f(s.draft) } : {}));
    return {
      startEdit(): void {
        patchState(store, { draft: structuredClone(store.layout().widgets) });
      },
      cancel(): void {
        patchState(store, { draft: null });
      },
      add(kind: WidgetKind, metric: WidgetMetric): void {
        if (store.canAdd()) edit((w) => [...w, newWidget(kind, metric, w)]);
      },
      remove(id: string): void {
        edit((w) => removeWidget(w, id));
      },
      resize(id: string, size: WidgetSize): void {
        edit((w) => resizeWidget(w, id, size));
      },
      setMetric(id: string, metric: WidgetMetric): void {
        edit((w) => w.map((x) => (x.id === id ? { ...x, metric } : x)));
      },
      move(from: number, to: number): void {
        edit((w) => moveWidget(w, from, to));
      },
      /** Shows a preset's widgets in the edit session (nothing is saved until Save). */
      usePreset(id: string): void {
        const next = applyPreset(store.layout(), id);
        if (next !== store.layout()) patchState(store, { draft: structuredClone(next.widgets) });
      },
      /** Back to the Overview preset. */
      reset(): void {
        this.usePreset('overview');
      },
      async save(): Promise<boolean> {
        const draft = store.draft();
        const s = settings.settings();
        if (!draft || !s) return false;
        const ok = await settings.save({ ...s, dashboard: { ...store.layout(), widgets: draft } }, 'Dashboard saved');
        if (ok) patchState(store, { draft: null });
        return ok;
      },
      /** Saves the current widgets as one of the user's own presets (the built-in ones are the engine's). */
      async saveAsPreset(name: string): Promise<boolean> {
        const draft = store.draft();
        const s = settings.settings();
        const clean = name.trim();
        if (!draft || !s || !clean) return false;
        const id = `custom-${clean.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`;
        const layout = store.layout();
        const presets = [...layout.presets.filter((p) => p.id !== id), { id, name: clean.slice(0, 64), builtin: false, widgets: structuredClone(draft) }];
        const ok = await settings.save({ ...s, dashboard: { ...layout, widgets: draft, presets } }, `Saved the preset “${clean}”`);
        if (ok) patchState(store, { draft: null });
        else notifier.show('The preset could not be saved.', 'bad');
        return ok;
      },
    };
  }),
);
