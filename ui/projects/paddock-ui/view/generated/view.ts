// Generated from the Rust wire types of paddock-view (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-view --lib generated.

/** The schema version these types describe (`ViewDoc.schema`). */
export const VIEW_SCHEMA = 1;

/** The limits the engine enforces (`paddock-view::validate`); the UI may rely on them. */
export const VIEW_LIMITS = { maxDepth: 16, maxNodes: 20000, maxEncodedBytes: 1048576, maxStringBytes: 8192, maxGraphNodes: 5000, maxGraphEdges: 20000, maxInlineRows: 2000 } as const;

export type ViewDoc = { 
/**
 * The schema version ([`SCHEMA`]).
 */
schema: number, 
/**
 * The view's title (a text label).
 */
title: string, 
/**
 * The content.
 */
root: Node, 
/**
 * The version of this document; a [`ViewPatch`] names the one it was made against and the one it produces.
 */
etag: number, };

export type Node = { 
/**
 * Empty: the node cannot be patched on its own. Otherwise unique in the document (at most 128 bytes).
 */
id: string, 
/**
 * What the node shows.
 */
kind: NodeKind, };

export type NodeKind = { "stack": { dir: Dir, children: Array<Node>, } } | { "text": { text: string, tone: Tone, style: TextStyle, } } | { "table": Table } | { "tree": Tree } | { "graph": Graph } | { "diagram": Diagram } | { "form": Form } | { "reviewList": ReviewList } | { "matrix": Matrix } | { "chart": ChartSpec } | { "badge": { text: string, tone: Tone, } } | { "progress": { value: number, label: string, } } | { "empty": { icon: Icon, title: string, hint: string, action: ActionRef | null, } } | { "needsAi": NeedsAi } | { "link": { label: string, target: Link, } };

export type Dir = "row" | "column";

export type Tone = "neutral" | "info" | "good" | "warn" | "bad";

export type TextStyle = "body" | "heading" | "subheading" | "caption" | "code";

export type Icon = "info" | "warning" | "error" | "check" | "search" | "folder" | "file" | "table" | "graph" | "chart" | "settings" | "ai" | "lock" | "link";

export type ActionRef = { 
/**
 * The plugin's action id. `page` and `expand` are the host's names for paging and lazy trees.
 */
id: string, 
/**
 * The text the user reads (a button or a menu entry); required, so every interactive node is labelled.
 */
label: string, 
/**
 * When set, the host asks this question before it sends the action.
 */
confirm: string | null, 
/**
 * Arguments the plugin gets back.
 */
args: unknown, 
/**
 * The action changes something (the UI badges it; Pacenotes never exposes it over MCP).
 */
writes: boolean, };

export type Link = { "action": ActionRef } | { "openInBrowser": { url: string, label: string, } };

export type Table = { columns: Array<Column>, rows: RowSource, 
/**
 * Chosen by activating a row (the host adds `row`: the index in the shown rows).
 */
rowAction: ActionRef | null, 
/**
 * How the rows are ordered now.
 */
sort: Sort | null, };

export type Column = { 
/**
 * Unique among the columns; what [`Sort`] names.
 */
key: string, 
/**
 * The header text.
 */
label: string, 
/**
 * The user may ask for this order (the host sends the action `sort` with `column` and `descending`).
 */
sortable: boolean, 
/**
 * Where the column's text sits; numbers are [`Align::End`] (right-aligned, tabular).
 */
align: Align, 
/**
 * The unit of the column's values ("ms", "lines"), shown beside the header; empty for none.
 */
unit: string, };

export type Align = "start" | "end";

export type Sort = { 
/**
 * A column key.
 */
column: string, descending: boolean, };

export type Cell = { text: string, tone: Tone, link: Link | null, };

export type RowSource = { "inline": Array<Array<Cell>> } | { "paged": { total: number, pageSize: number, 
/**
 * The page shown now (from 0).
 */
page: number, 
/**
 * The rows of that page (at most `page_size`).
 */
rows: Array<Array<Cell>>, } };

export type Tree = { nodes: Array<TreeNode>, 
/**
 * Children of `expandable` nodes are fetched with the action `expand` (`node` = the id).
 */
lazy: boolean, };

export type TreeNode = { 
/**
 * Unique in the tree.
 */
id: string, label: string, 
/**
 * 0 for a top-level node; at most one more than the node before it.
 */
depth: number, tone: Tone, icon: Icon | null, 
/**
 * Its children are not loaded yet (only in a lazy tree).
 */
expandable: boolean, 
/**
 * Chosen by activating the node (the host adds `node`).
 */
action: ActionRef | null, };

export type Graph = { nodes: Array<GNode>, edges: Array<GEdge>, layout: Layout, };

export type GNode = { 
/**
 * Unique in the graph.
 */
id: string, label: string, tone: Tone, 
/**
 * Chosen by activating the node (the host adds `node`).
 */
action: ActionRef | null, };

export type GEdge = { from: string, to: string, 
/**
 * May be empty.
 */
label: string, };

export type Layout = "layered" | "force" | "radial";

export type Diagram = { kind: DiagramKind, model: DiagramModel, };

export type DiagramKind = "c4" | "class" | "sequence" | "flow";

export type DiagramModel = { elements: Array<DiagramElement>, relations: Array<DiagramRelation>, };

export type DiagramElement = { 
/**
 * Unique in the diagram.
 */
id: string, label: string, 
/**
 * Members of a class, the description of a C4 element; may be empty.
 */
detail: string, 
/**
 * The boundary or package it sits in; may be empty.
 */
group: string, };

export type DiagramRelation = { from: string, to: string, 
/**
 * May be empty.
 */
label: string, };

export type Form = { fields: Array<Field>, submit: ActionRef, };

export type Field = { 
/**
 * Unique in the form.
 */
id: string, label: string, 
/**
 * May be empty.
 */
help: string, required: boolean, kind: FieldKind, };

export type FieldKind = { "text": { value: string, placeholder: string, multiline: boolean, } } | { "number": { value: number, min: number | null, max: number | null, } } | { "choice": { value: string, options: Array<ChoiceOption>, } } | { "toggle": { value: boolean, } } | { "path": { value: GrantRef | null, folder: boolean, } } | { "secretRef": { value: SecretRef | null, } };

export type ChoiceOption = { value: string, label: string, };

export type GrantRef = string;

export type SecretRef = string;

export type ReviewList = { items: Array<Finding>, states: Array<StateDef>, };

export type Finding = { 
/**
 * Unique in the list.
 */
id: string, title: string, 
/**
 * May be empty.
 */
detail: string, 
/**
 * Where it is (a path, a line); may be empty.
 */
location: string, tone: Tone, 
/**
 * The id of one of the list's states.
 */
state: string, link: Link | null, };

export type StateDef = { 
/**
 * Unique in the list.
 */
id: string, label: string, tone: Tone, 
/**
 * The actions offered for a finding in this state (the host adds `finding`, and `reason` when `needs_reason`).
 */
actions: Array<ActionRef>, 
/**
 * The host asks the user for a reason before it sends an action of this state.
 */
needsReason: boolean, };

export type Matrix = { rows: Array<Label>, cols: Array<Label>, cells: Array<MatrixCell>, };

export type Label = { id: string, text: string, };

export type MatrixCell = { row: number, col: number, text: string, tone: Tone, action: ActionRef | null, };

export type ChartSpec = { kind: ChartKind, title: string, 
/**
 * The x axis (or the slices); every series has one value per category.
 */
categories: Array<string>, series: Array<Series>, 
/**
 * The unit of the values ("s", "%"), for the value axis and the summary; empty for none.
 */
unit: string, };

export type ChartKind = "line" | "bar" | "area" | "pie" | "scatter";

export type Series = { name: string, values: Array<number>, };

export type NeedsAi = { 
/**
 * What needs it ("Explain this log").
 */
feature: string, 
/**
 * Why it cannot run now; may be empty.
 */
reason: string, 
/**
 * The plugin's own wording of a way forward ("Set up Pitwall"). The host does NOT send it to the plugin (rule 19: a repair never
 * depends on the thing that is broken): the host shows the reason and its own "Open AI settings", so this is kept only for the
 * wire shape.
 */
setup: ActionRef, };

export type ViewAction = { 
/**
 * The view's id (the contribution it belongs to).
 */
view: string, 
/**
 * The [`ActionRef::id`].
 */
action: string, args: unknown, };

export type ViewPatch = { "replace": { 
/**
 * The etag of the document the patch was made against.
 */
baseEtag: number, 
/**
 * The etag after the patch (not the base one).
 */
etag: number, id: string, node: Node, } } | { "doc": ViewDoc };
