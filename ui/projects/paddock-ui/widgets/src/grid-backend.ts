import { InjectionToken } from '@angular/core';
import type { Cells, Edit, LayoutView, PreviewRequest } from '../generated/widgets';

/** A grid surface's view: the layout holds rectangles (`Cells`), `arrangements` and `moves` are filled. */
export type GridView<W = unknown, K extends string = string, S = unknown> = LayoutView<S, W, Cells, K>;

/** The answer to a preview: the request's `seq` and the view the gesture would give (nothing is stored). */
export interface PreviewReply<W = unknown, K extends string = string, S = unknown> {
  /** The request's counter; the grid drops a reply that is not the latest. */
  readonly seq: number;
  /** The previewed view. */
  readonly view: GridView<W, K, S>;
}

/**
 * The engine, as the grid sees it (ADR 0044 §2 and §6). Each method resolves with the engine's answer or rejects with the
 * `WidgetError` the engine returned (what a Tauri `invoke` does): `stale` makes the grid reload and retry once, `blocked`
 * becomes words under the bar. The mock backend of the gallery answers from `generated/grid-cases.json` and is never the
 * reference for behaviour.
 */
export interface WidgetGridBackend<W = unknown, K extends string = string, S = unknown> {
  /** What a gesture would give on `request.revision`, in the app's own process; stores nothing. */
  preview(request: PreviewRequest): Promise<PreviewReply<W, K, S>>;
  /** Applies an edit (an `arrange` edit names the revision it was computed on) and returns the new view. */
  edit(edit: Edit<W>, revision: number): Promise<GridView<W, K, S>>;
  /** Reads the view again (after a `stale` answer). */
  reload(): Promise<GridView<W, K, S>>;
}

/** The backend of every `pd-widget-grid` below it. */
export const PD_WIDGET_GRID_BACKEND = new InjectionToken<WidgetGridBackend>('PD_WIDGET_GRID_BACKEND');
