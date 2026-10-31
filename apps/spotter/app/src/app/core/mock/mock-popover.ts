import type { KindInfo, Placement } from '@decryp7/paddock-ui/widgets';
import type { CommandError } from '../api.types';
import type { LoadingRows, PopoverStatus, PopoverWidget } from '../generated/widgets';
import type { PopoverEdit, PopoverState, PopoverValues, PopoverView } from '../widgets.types';
import fixtures from './popover-fixtures.json';

/**
 * The mock's popup layout and values (browser development and tests only). It decides nothing: the catalogue, the default
 * layout, the loading shape and every widget's state are what the Rust engine produced for the named scenarios
 * (`popover-fixtures.json`, written by `spotter-engine`'s `widgets` tests and checked on every run), so the mock cannot drift
 * from the engine. Only the layout edits the chooser sends are handled here, as the engine's `paddock-widgets` would.
 */
export type PopoverScenario = 'demo' | 'empty' | 'worst';
interface Fixture {
  readonly nowMs: number;
  readonly view: PopoverView;
  readonly shape: LoadingRows[];
  readonly scenarios: Record<PopoverScenario, { readonly byKind: Record<string, PopoverState>; readonly noticeWithoutNotificationsWidget: PopoverValues['notice']; readonly status: PopoverStatus }>;
}
const FIXTURE = fixtures as unknown as Fixture;
const KINDS: KindInfo<PopoverWidget>[] = FIXTURE.view.catalogue.kinds as KindInfo<PopoverWidget>[];
const MAX = FIXTURE.view.catalogue.surface.maxPlacements;
/** The popup a fresh install shows, as the engine's defaults say. */
export const DEFAULTS: [number, PopoverWidget][] = FIXTURE.view.layout.placements.map((p) => [p.span, p.widget]);
/** The kinds of [`DEFAULTS`], in order: specs read the default popup from here, never a hand-written list. */
export const DEFAULT_KINDS: PopoverWidget[] = DEFAULTS.map(([, kind]) => kind);
/** How many rows each kind draws, loading or ready (the engine's). */
export const LOADING_SHAPE: LoadingRows[] = FIXTURE.shape;

/** Times in the fixtures are the engine's test clock's; the mock moves them to its own now. */
const TIME_FIELDS = new Set(['asOf', 'staleSince', 'firstSeenMs', 'atMs', 'untilMs']);
function rebase<T>(value: T, delta: number): T {
  if (Array.isArray(value)) return value.map((v) => rebase(v, delta)) as T;
  if (value && typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, TIME_FIELDS.has(k) && typeof v === 'number' ? v + delta : rebase(v, delta)])) as T;
  }
  return value;
}

function refused(message: string): CommandError {
  return { code: 'badRequest', message, problems: [] };
}

const place = (widget: PopoverWidget, span: number): Placement<PopoverWidget> => ({ id: widget.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`), span, widget });

export class MockPopover {
  /** `shown`: the kinds to start with (a spec that needs particular widgets names them); the default is the fresh-install popup. */
  constructor(shown: readonly PopoverWidget[] = DEFAULT_KINDS) {
    this.placements = shown.map((kind) => place(kind, DEFAULTS.find(([, k]) => k === kind)?.[0] ?? KINDS.find((i) => i.kind === kind)?.spans[0] ?? 2));
  }

  private placements: Placement<PopoverWidget>[];
  private previous: Placement<PopoverWidget>[] | null = null;

  view(): PopoverView {
    return {
      layout: { version: 1, placements: this.placements.map((p) => ({ ...p })), presets: [] },
      catalogue: { surface: { id: 'popover', noun: 'popup', columns: 2, maxPlacements: MAX, minPlacements: 0, maxUserPresets: 0 }, kinds: KINDS },
      builtinPresets: [],
      notes: [],
      canUndo: this.previous !== null,
      editable: true,
    };
  }

  edit(edit: PopoverEdit): PopoverView {
    const list = [...this.placements];
    if (edit === 'undo') {
      if (!this.previous) throw refused('There is nothing to undo.');
      this.placements = this.previous;
      this.previous = null;
    } else if (edit === 'resetToDefaults') {
      this.previous = list;
      this.placements = DEFAULTS.map(([span, w]) => place(w, span));
    } else if ('add' in edit) {
      const info = KINDS.find((k) => k.kind === edit.add.widget);
      if (!info) throw refused('That widget does not exist.');
      if (list.some((p) => p.widget === edit.add.widget)) throw refused(`${info.title} is already shown.`);
      if (list.length >= MAX) throw refused(`At most ${MAX} widgets fit in the popup; remove one first.`);
      list.splice(Math.min(edit.add.at ?? list.length, list.length), 0, place(edit.add.widget, edit.add.span ?? info.spans[0] ?? 2));
      this.placements = list;
      this.previous = null;
    } else if ('remove' in edit) {
      const i = list.findIndex((p) => p.id === edit.remove.id);
      if (i < 0) throw refused('That widget is not in the popup any more.');
      list.splice(i, 1);
      this.previous = this.placements;
      this.placements = list;
    } else if ('move' in edit) {
      const i = list.findIndex((p) => p.id === edit.move.id);
      if (i < 0) throw refused('That widget is not in the popup any more.');
      const [p] = list.splice(i, 1);
      list.splice(Math.max(0, Math.min(edit.move.to, list.length)), 0, p!);
      this.placements = list;
      this.previous = null;
    } else if ('resize' in edit) {
      const i = list.findIndex((p) => p.id === edit.resize.id);
      const p = list[i];
      if (!p || !KINDS.find((k) => k.kind === p.widget)?.spans.includes(edit.resize.span)) throw refused('That width is not available for this widget.');
      list[i] = { ...p, span: edit.resize.span };
      this.placements = list;
      this.previous = null;
    } else {
      throw refused('The mock does not do that edit.');
    }
    return this.view();
  }

  /** The overall status of a scenario, as the engine decided it. */
  status(scenario: PopoverScenario): PopoverStatus {
    return FIXTURE.scenarios[scenario].status;
  }

  /** What the widgets show in a named scenario: the engine's output for it, for the placed kinds. */
  values(scenario: PopoverScenario, now: number): PopoverValues {
    const fx = FIXTURE.scenarios[scenario];
    const delta = now - FIXTURE.nowMs;
    const placements: PopoverValues['placements'] = this.placements.map((p) => ({ id: p.id, state: rebase(fx.byKind[p.widget]!, delta) }));
    const notice = this.placements.some((p) => p.widget === 'notifications') ? null : rebase(fx.noticeWithoutNotificationsWidget, delta);
    return { notice, placements };
  }
}
