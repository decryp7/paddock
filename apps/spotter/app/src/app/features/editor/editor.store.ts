import { computed, inject } from '@angular/core';
import { patchState, signalStore, withComputed, withMethods, withState } from '@ngrx/signals';
import type { DiscoveredFeed, ImportFormat, ImportResult, Preset, SourceKind, SourcePreview, WatchDraft, WatchSummary } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { blankDraft } from '../../core/draft';
import { SettingsStore } from '../../state/settings.store';
import { idle, pending, settle, type Loadable } from '../../state/loadable';
import { draftFor, missingFields, suggestName } from './draft-utils';

export type EditorStep = 'source' | 'configure' | 'versions' | 'notify' | 'review';
export const STEPS: { id: EditorStep; label: string }[] = [
  { id: 'source', label: 'Source' },
  { id: 'configure', label: 'Configure' },
  { id: 'versions', label: 'Versions' },
  { id: 'notify', label: 'Notify' },
  { id: 'review', label: 'Review' },
];

/** How long edits are collected before the source is tested again. */
export const PREVIEW_DEBOUNCE_MS = 500;

interface EditorState {
  step: EditorStep;
  editing: boolean;
  kinds: Loadable<SourceKind[]>;
  presets: Loadable<Preset[]>;
  draft: WatchDraft;
  secrets: Record<string, string>;
  secretsSet: string[];
  loadedDraft: Loadable<WatchDraft>;
  preview: Loadable<SourcePreview>;
  saving: boolean;
  problems: string[];
  feeds: DiscoveredFeed[] | null;
  discovering: boolean;
  discoverError: string | null;
  imported: ImportResult | null;
  importing: boolean;
  importError: string | null;
}

/** The add/edit wizard's state: the draft, the live test of its source, and saving. Provided per page. */
export const EditorStore = signalStore(
  withState<EditorState>({
    step: 'source',
    editing: false,
    kinds: idle<SourceKind[]>(),
    presets: idle<Preset[]>(),
    draft: blankDraft(),
    secrets: {},
    secretsSet: [],
    loadedDraft: idle<WatchDraft>(),
    preview: idle<SourcePreview>(),
    saving: false,
    problems: [],
    feeds: null,
    discovering: false,
    discoverError: null,
    imported: null,
    importing: false,
    importError: null,
  }),
  withComputed((s) => {
    const kind = computed(() => s.kinds().value?.find((k) => k.id === s.draft().source.kind) ?? null);
    return {
      kind,
      missing: computed(() => missingFields(kind(), s.draft().source.values)),
      stepIndex: computed(() => STEPS.findIndex((x) => x.id === s.step())),
    };
  }),
  withMethods((store) => {
    const backend = inject(BACKEND);
    const settings = inject(SettingsStore);
    let timer: ReturnType<typeof setTimeout> | undefined;
    let run = 0;

    const runPreview = async (): Promise<void> => {
      const mine = ++run;
      patchState(store, { preview: pending(store.preview()) });
      const next = await settle(store.preview(), () => backend.testSource(store.draft(), store.secrets()));
      if (mine === run) patchState(store, { preview: next });
    };

    const schedule = (): void => {
      clearTimeout(timer);
      if ((store.step() !== 'configure' && store.step() !== 'versions') || store.missing().length) return;
      timer = setTimeout(() => void runPreview(), PREVIEW_DEBOUNCE_MS);
    };

    const update = (fn: (d: WatchDraft) => WatchDraft): void => {
      patchState(store, { draft: fn(store.draft()), problems: [] });
      schedule();
    };

    return {
      runPreview,
      update,
      async loadCatalog(): Promise<void> {
        patchState(store, { kinds: pending(store.kinds()), presets: pending(store.presets()) });
        const [kinds, presets] = await Promise.all([settle(store.kinds(), () => backend.sourceKinds()), settle(store.presets(), () => backend.presets())]);
        patchState(store, { kinds, presets });
      },
      /** Loads an existing watch for editing (and starts at the source's form). */
      async loadWatch(id: string): Promise<void> {
        patchState(store, { editing: true, loadedDraft: pending(store.loadedDraft()) });
        const next = await settle(store.loadedDraft(), async () => {
          const d = await backend.getWatch(id);
          patchState(store, { draft: d.spec, secretsSet: d.secretsSet, secrets: {}, step: 'configure' });
          return d.spec;
        });
        patchState(store, { loadedDraft: next });
        if (next.value) void runPreview();
      },
      setStep(step: EditorStep): void {
        patchState(store, { step });
        schedule();
        if (step === 'configure' || step === 'versions') {
          if (!store.missing().length && store.preview().value === null) void runPreview();
        }
      },
      next(): void {
        const i = store.stepIndex();
        const to = STEPS[Math.min(i + 1, STEPS.length - 1)]?.id ?? 'review';
        this.setStep(to);
        if (to === 'review' && !store.draft().name.trim()) update((d) => ({ ...d, name: suggestName(store.kind(), d.source.values) }));
      },
      back(): void {
        this.setStep(STEPS[Math.max(store.stepIndex() - 1, 0)]?.id ?? 'source');
      },
      pickKind(kind: SourceKind): void {
        patchState(store, { draft: draftFor(blankDraft(), kind, settings.settings().value?.defaultIntervalSecs ?? 6 * 3600), secrets: {}, preview: idle(), step: 'configure' });
      },
      pickPreset(p: Preset): void {
        const kind = store.kinds().value?.find((k) => k.id === p.draft.source.kind);
        const interval = settings.settings().value?.defaultIntervalSecs ?? p.draft.intervalSecs;
        patchState(store, { draft: { ...structuredClone(p.draft), id: null, intervalSecs: interval }, secrets: {}, preview: idle(), step: 'configure' });
        if (kind) schedule();
      },
      setValue(key: string, value: string): void {
        update((d) => ({ ...d, source: { ...d.source, values: { ...d.source.values, [key]: value } } }));
      },
      setSecret(key: string, value: string): void {
        patchState(store, { secrets: { ...store.secrets(), [key]: value }, problems: [] });
        schedule();
      },
      /** Removes a stored secret on save. */
      clearSecret(key: string): void {
        patchState(store, { secrets: { ...store.secrets(), [key]: '' }, secretsSet: store.secretsSet().filter((k) => k !== key) });
      },
      async discover(url: string): Promise<void> {
        patchState(store, { discovering: true, discoverError: null, feeds: null });
        try {
          patchState(store, { feeds: await backend.discoverFeeds(url.trim()) });
        } catch (e) {
          const err = toCommandError(e);
          patchState(store, { discoverError: [err.message, ...err.problems].join(' ') });
        } finally {
          patchState(store, { discovering: false });
        }
      },
      pickFeed(feed: DiscoveredFeed): void {
        const kind = store.kinds().value?.find((k) => k.id === 'feed');
        if (!kind) return;
        patchState(store, {
          draft: { ...draftFor(blankDraft({ name: feed.title }), kind, settings.settings().value?.defaultIntervalSecs ?? 6 * 3600), source: { kind: 'feed', values: { url: feed.url } } },
          secrets: {},
          preview: idle<SourcePreview>(),
          step: 'configure',
        });
        schedule();
      },
      async importList(format: ImportFormat, text: string): Promise<void> {
        patchState(store, { importing: true, importError: null, imported: null });
        try {
          patchState(store, { imported: await backend.importList(format, text) });
        } catch (e) {
          patchState(store, { importError: toCommandError(e).message });
        } finally {
          patchState(store, { importing: false });
        }
      },
      /** Saves every imported draft; returns how many were saved and the failures. */
      async saveImported(): Promise<{ saved: number; failed: string[] }> {
        const drafts = store.imported()?.drafts ?? [];
        let saved = 0;
        const failed: string[] = [];
        for (const d of drafts) {
          try {
            await backend.saveWatch(d, {});
            saved++;
          } catch (e) {
            failed.push(`${d.name}: ${toCommandError(e).message}`);
          }
        }
        return { saved, failed };
      },
      /** Saves the draft; on a refusal the engine's problems are kept for the page to show. */
      async save(): Promise<WatchSummary | null> {
        patchState(store, { saving: true, problems: [] });
        try {
          return await backend.saveWatch(store.draft(), store.secrets());
        } catch (e) {
          const err = toCommandError(e);
          patchState(store, { problems: err.problems.length ? err.problems : [err.message] });
          return null;
        } finally {
          patchState(store, { saving: false });
        }
      },
    };
  }),
);
