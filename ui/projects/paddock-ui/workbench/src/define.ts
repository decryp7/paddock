import type { LayoutArea, ViewLayout } from '@decryp7/paddock-ui';
import type { LazyPart, Live, Message, ShellContext } from '@decryp7/paddock-ui/shell';

/*
 * The contribution kinds the workbench adds to the shell's registry (ADR 0013 §2; plan W1). Same rules as the shell's
 * `define*` (ADR 0012 §2): each returns a frozen value and validates nothing; every check lives in the kinds (`kinds.ts`)
 * and is reported by `assertContributionsValid`, for every instance at once. Static data plus loaders; live functions
 * take the app's context as an argument, so a `*.contributions.ts` file imports no store or component (plan §9).
 */

/**
 * A dockable view (ADR 0013 §2): an Explorer-adjacent tool window, a document, an analysis panel or a status pane. Its
 * component and providers load on first show, in an injector of their own, so a pop-out window composes only what it
 * shows (rule 34).
 */
export interface ViewDef<T extends object = object> extends LazyPart {
  /** Stable, namespaced id (`pn.code`): the dock panel id, the key in saved layouts and perspectives. */
  readonly id: string;
  readonly title: Message;
  /** `namespace:name` of an icon in the app's or the shell's icon registry. */
  readonly icon: string;
  /**
   * What the view needs (ADR 0011): its area, minimum and preferred size, priority. Required: the dock's `planLayout`
   * never squeezes a view below `min`, it tabs or auto-hides the lowest priority instead.
   */
  readonly layout: ViewLayout;
  /**
   * A document that needs the full height of the window (a home screen, a tree): it opens in a column of its own at the
   * window's edge instead of beside the other documents.
   */
  readonly fullHeight?: boolean;
  /** The view can be open several times: panels `<id>#<key>` render it too (see `panelId`). Defaults to one panel. */
  readonly instances?: boolean;
  /** Its submenu in View; without one it is listed at the top level of View. */
  readonly category?: Message;
  /** `false`: reached from inside another view only; left out of the View menu but offered by the palette. */
  readonly listed?: boolean;
  /** Ascending in the View menu and the palette's view list; shell and workbench views use multiples of 100. */
  readonly order?: number;
  /** The view is offered (View menu, palette, activity items that show it) only while this holds. Defaults to always. */
  readonly when?: Live<T, boolean>;
  /**
   * Opens the view in the background when `when` starts to hold (a viewer for the kind of file just opened). Without it a
   * view with `when` is only offered. A view whose `when` stops holding is always closed.
   */
  readonly autoOpen?: boolean;
  /** `false`: the user cannot close the panel (it may still be moved, tabbed and auto-hidden). Defaults to true. */
  readonly closable?: boolean;
  /** `false`: the panel cannot be moved to its own window. Defaults to true. */
  readonly popOut?: boolean;
}

/** One entry of an activity item's fly-out menu, built on every opening. */
export interface ActivityMenuEntry {
  /** Unique within the menu. */
  readonly id: string;
  /** The entry's text (built at run time, so already in the window's language). */
  readonly label: string;
  readonly icon?: string;
  /** A check mark (the view is open). */
  readonly checked?: boolean;
  /** Key in command syntax shown as a hint (`Mod+B`). */
  readonly keybinding?: string;
  readonly run: ActivityTarget;
}

/** What an activity button (or a fly-out entry) does: show a view, run a command, or show an Explorer provider's section. */
export type ActivityTarget = { readonly view: string } | { readonly command: string; readonly args?: unknown } | { readonly explorer: string };

/**
 * A button in the activity bar (ADR 0013 §1): shows a view, runs a command, focuses an Explorer section, or opens a
 * fly-out menu. Every live function is evaluated defensively: one that throws hides the button (`when`) or drops that one
 * extra (`badge`, `active`, `pulse`, `disabled`, `menu`), never the bar (Debrief's activity bar rule).
 */
export interface ActivityItemDef<T extends object = object> {
  /** Stable, namespaced id; `data-activity-id` on the button. */
  readonly id: string;
  /** Accessible name and tooltip. */
  readonly title: Message;
  readonly icon: string;
  /** Ascending, top to bottom within its section; higher orders collapse into More first. */
  readonly order: number;
  /** `bottom`: pinned at the foot of the bar (Settings). Defaults to `top`. */
  readonly section: 'top' | 'bottom';
  readonly when?: Live<T, boolean>;
  /** A short count drawn on the icon (null or '' draws none). */
  readonly badge?: Live<T, string | number | null>;
  /** What the badge counts, as its accessible name ("3 problems"). */
  readonly badgeLabel?: Live<T, string | null>;
  /** Highlighted while true. Defaults to: its view is open, or its Explorer section is shown. */
  readonly active?: Live<T, boolean>;
  /** A toggle button: `aria-pressed` follows `active`. */
  readonly toggle?: boolean;
  /** A small pulsing dot while true (following live data). */
  readonly pulse?: Live<T, boolean>;
  /** Shown but inert while this returns a reason, which becomes the tooltip. */
  readonly disabled?: Live<T, string | null>;
  /** Key shown in the tooltip (`Mod+B`); defaults to the bound key of its command. */
  readonly keybinding?: string;
  readonly run: ActivityTarget | { readonly menu: Live<T, readonly ActivityMenuEntry[]> };
}

/**
 * One node of an Explorer tree: display data only (the engine decides what the tree contains). Ids are unique within the
 * provider; names and paths are drawn middle-truncated with the full value in a tooltip (rule 18).
 */
export interface ExplorerNode {
  /** Unique within the provider and stable across reloads (selection and expansion are kept by id). */
  readonly id: string;
  readonly label: string;
  /** Secondary text after the label (a count, a branch). */
  readonly description?: string;
  /** Full value for the tooltip when the label is shortened (a path). */
  readonly tooltip?: string;
  readonly icon?: string;
  /** Whether the node has children (a chevron; `children` is asked on expand). */
  readonly expandable: boolean;
  /** `path`: truncated in the middle, keeping the file name (default for labels with a separator); `end`: an ellipsis at the end. */
  readonly truncate?: 'middle' | 'end';
  /** A short badge after the label (a git status letter, a count). */
  readonly badge?: string;
  /** Accessible name of the badge ("modified"). */
  readonly badgeLabel?: string;
  /** Drawn as unavailable (a source that is gone), with this reason as the tooltip. */
  readonly problem?: string;
}

/** What an Explorer provider is asked for: one page, from a cursor, cancellable. */
export interface ExplorerRequest {
  /** The previous page's `next`, or absent for the first page. */
  readonly cursor?: string;
  /** At most this many nodes. */
  readonly limit: number;
  /** Aborted when the node collapses, the section closes, or a newer request replaces this one. */
  readonly signal: AbortSignal;
}

/** One page of nodes; `next` when there are more (a "Show more" row asks for it). */
export interface ExplorerPage<N extends ExplorerNode = ExplorerNode> {
  readonly nodes: readonly N[];
  readonly next?: string;
  /** How many there are in all, when the provider knows ("Show 200 more of 12,000"). */
  readonly total?: number;
}

/** An action in an Explorer node's context menu. */
export interface ExplorerAction {
  /** Unique within the menu. */
  readonly id: string;
  readonly label: string;
  readonly icon?: string;
  /** Separators fall between groups. */
  readonly group?: string;
  /** Shown but inert with this reason as the tooltip. */
  readonly disabled?: string;
  /** A destructive action is drawn in the danger colour and asks first through the app. */
  readonly danger?: boolean;
}

/** What a node offers to a drag: a MIME type and a string payload (paths, ids). */
export interface ExplorerDragData {
  readonly type: string;
  readonly data: string;
}

/**
 * A section of the Explorer side bar (ADR 0013 §2): a lazy, paged, cancellable tree. The provider only fetches and
 * describes nodes (typically through an engine command); what the tree contains is the engine's decision.
 */
export interface ExplorerProviderDef<T extends object = object, N extends ExplorerNode = ExplorerNode> {
  readonly id: string;
  readonly title: Message;
  readonly icon?: string;
  /** Ascending, top to bottom in the side bar. */
  readonly order: number;
  readonly when?: Live<T, boolean>;
  /** The top-level nodes. */
  readonly roots: (ctx: ShellContext<T>, request: ExplorerRequest) => Promise<ExplorerPage<N>>;
  /** A node's children; required when any node is expandable. */
  readonly children?: (node: N, ctx: ShellContext<T>, request: ExplorerRequest) => Promise<ExplorerPage<N>>;
  /** Activation (Enter, double click): open the node's document, reveal it. */
  readonly open?: (node: N, ctx: ShellContext<T>) => void | Promise<void>;
  /** The context menu of a node. */
  readonly actions?: (node: N, ctx: ShellContext<T>) => readonly ExplorerAction[];
  /** Runs a context-menu action. */
  readonly run?: (action: string, node: N, ctx: ShellContext<T>) => void | Promise<void>;
  /** What dragging a node carries; without it nodes are not draggable. */
  readonly drag?: (node: N) => ExplorerDragData | null;
  /**
   * What the tree depends on besides the engine's own events (the selected file of an outline): when its value changes,
   * the section reloads from its roots. Read through signals, so it is tracked.
   */
  readonly changes?: Live<T, unknown>;
  /** A text for the empty tree ("Open a folder to start"); a command button may follow. */
  readonly empty?: { readonly text: Message; readonly command?: string };
}

/** One result in the palette: display data plus what the provider needs to act on it. */
export interface PaletteItem {
  /** Unique within the provider's results. */
  readonly id: string;
  readonly label: string;
  /** Secondary text on the right (a path, a category). */
  readonly detail?: string;
  readonly icon?: string;
  /** Key binding in command syntax, shown as a hint. */
  readonly keybinding?: string;
  /** Character ranges `[start, end)` of `label` to highlight (from the engine's matcher). */
  readonly highlights?: readonly (readonly [number, number])[];
  /** Shown but inert with this reason. */
  readonly disabled?: string;
}

/** What a palette provider is asked: the query without its prefix, a cap and a signal (aborted on the next keystroke). */
export interface PaletteRequest {
  readonly limit: number;
  readonly signal: AbortSignal;
}

/**
 * A source of palette results (ADR 0013 §2): go to file, go to symbol, search everything. Chosen by a one-character
 * prefix typed first (`@`, `#`, `>`); the provider without a prefix answers plain text. Matching is the engine's work
 * (rule 20): a provider calls an engine command (a `names_search`, see `namesPaletteProvider`), takes the
 * `AbortSignal`, and returns at most `limit` results.
 */
export interface PaletteProviderDef<T extends object = object, I extends PaletteItem = PaletteItem> {
  readonly id: string;
  readonly title: Message;
  /** One symbol (`@`, `#`, `>`, `:`), or `''` for plain text (one provider at most per prefix). */
  readonly prefix: string;
  /** The input's placeholder while this provider is chosen. */
  readonly placeholder?: Message;
  /** Ascending in the prefix list shown for an empty query. */
  readonly order: number;
  readonly when?: Live<T, boolean>;
  /** Results for the query (prefix removed). Must honour `request.signal`; a rejected promise shows an inline error. */
  readonly search: (query: string, ctx: ShellContext<T>, request: PaletteRequest) => Promise<readonly I[]>;
  /** Accepting a result (Enter, click). */
  readonly accept: (item: I, ctx: ShellContext<T>) => void | Promise<void>;
  /** Wait this long after the last keystroke before searching (engine queries); 0 for in-memory providers. */
  readonly debounceMs?: number;
}

/** A named arrangement of views (ADR 0013 §2): Understand, Review, Graph. Ctrl+1..9 switches between the first nine. */
export interface PerspectiveDef<T extends object = object> {
  readonly id: string;
  readonly title: Message;
  readonly icon: string;
  /** One line on what it is for. */
  readonly description?: Message;
  readonly order: number;
  /** The views it opens (others are closed); each is laid out by its declared layout. */
  readonly views: readonly string[];
  /** The view that takes focus after switching; defaults to the first document. */
  readonly focus?: string;
  readonly when?: Live<T, boolean>;
}

/** How much of the host's state a pop-out or dialog window receives (Debrief's `SharedScope`). */
export type SharedScope = 'full' | 'display';

/**
 * A piece of UI state shared between a host window and its pop-outs (ADR 0013 §1): the host sends `read`'s value when it
 * changes; a guest parses what it receives at the edge (`parse`, rule "parse, don't validate") and applies it. The engine
 * stays the source of truth for data; this is interaction state only (selection, query, cursor).
 */
export interface SharedStateDef<T extends object = object, V = unknown> {
  /** Stable, namespaced key. */
  readonly id: string;
  /** `display` values (theme, formats) reach every window; `full` values only pop-outs and windows that show the workspace. */
  readonly scope: SharedScope;
  readonly read: Live<T, V>;
  /** The value from another window, or `undefined` to ignore it (malformed). Must not throw. */
  readonly parse: (raw: unknown) => V | undefined;
  readonly apply: (value: V, ctx: ShellContext<T>) => void;
  /** Send at most this often (a cursor that follows the pointer). Defaults to every change. */
  readonly throttleMs?: number;
}

/** A context chip's value: what the model is told, and a short detail for the chip. */
export interface AssistantContextValue {
  /** What the assistant receives (bounded by the workbench; never HTML). */
  readonly text: string;
  /** The chip's detail ("main.rs:120-140"). */
  readonly detail?: string;
}

/**
 * A context chip of the assistant pane (ADR 0013 §2): the user's current selection, the open file. Tools stay in Rust;
 * the UI contributes only what the user sees and may remove before sending.
 */
export interface AssistantContextDef<T extends object = object> {
  readonly id: string;
  readonly label: Message;
  readonly icon?: string;
  /** The chip's value now, or null when there is nothing to add. */
  readonly value: Live<T, AssistantContextValue | null>;
  /** Attached to a new message unless the user removes it. Defaults to true. */
  readonly defaultOn?: boolean;
  readonly order: number;
}

/** Optional fields left out of a spec; `define*` fills what has a default. */
type Opt<T, K extends keyof T> = Omit<T, K> & Partial<Pick<T, K>>;

function frozen<T extends object>(value: T): T {
  return Object.freeze(value);
}

export function defineView<T extends object = object>(spec: ViewDef<T>): ViewDef<T> {
  return frozen({ ...spec });
}

export function defineActivityItem<T extends object = object>(spec: Opt<ActivityItemDef<T>, 'section'>): ActivityItemDef<T> {
  return frozen({ ...spec, section: spec.section ?? 'top' });
}

export function defineExplorerProvider<T extends object = object, N extends ExplorerNode = ExplorerNode>(spec: ExplorerProviderDef<T, N>): ExplorerProviderDef<T, N> {
  return frozen({ ...spec });
}

export function definePaletteProvider<T extends object = object, I extends PaletteItem = PaletteItem>(
  spec: Opt<PaletteProviderDef<T, I>, 'prefix'>,
): PaletteProviderDef<T, I> {
  return frozen({ ...spec, prefix: spec.prefix ?? '' });
}

export function definePerspective<T extends object = object>(spec: PerspectiveDef<T>): PerspectiveDef<T> {
  return frozen({ ...spec, views: Object.freeze([...spec.views]) });
}

export function defineSharedState<T extends object = object, V = unknown>(spec: SharedStateDef<T, V>): SharedStateDef<T, V> {
  return frozen({ ...spec });
}

export function defineAssistantContext<T extends object = object>(spec: Opt<AssistantContextDef<T>, 'order'>): AssistantContextDef<T> {
  return frozen({ ...spec, order: spec.order ?? 100 });
}

/** The areas a view may declare, for messages. */
export const VIEW_AREAS: readonly LayoutArea[] = ['document', 'side', 'analysis', 'status'];
