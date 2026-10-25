/**
 * `@decryp7/paddock-ui/view`: `<pd-view>` (plan X51, ADR 0002), the generic renderer of the declarative views plugins return
 * (`paddock-view` in Rust). A plugin sends a `ViewDoc`: pure data. This entry validates it against the same limits on receipt,
 * draws every node kind with the family's components (stack, text, table with sort and paging, lazy tree, graph, diagram, form,
 * review list, matrix, chart, badge, progress, empty, needs-AI, link), binds all text as text and maps tones to the design
 * tokens. What the user does leaves as a `ViewAction`; the host answers with a patch. Confirmations, the OS browser and the path and
 * secret pickers are the host's (`VIEW_HOST`); the view opens nothing itself. An app that has the window's Pitwall state gives a
 * `needsAi` node its own live component with `VIEW_NEEDS_AI`; without it the node shows the plugin's reason and the app's
 * `VIEW_OPEN_AI_SETTINGS` button (never the plugin's own `setup` action, rule 19).
 *
 * ```ts
 * providers: [{ provide: VIEW_HOST, useValue: myHost }]              // confirm, openInBrowser, pickPath, pickSecret
 * ```
 * ```html
 * <pd-view view="pacenotes.review" [doc]="doc()" [busy]="working()" (action)="send($event)" (retry)="reload()" #v />
 * ```
 */
export { PdView } from './src/view';
export { PdViewNode } from './src/node';
export { PdViewButton, PdViewLink } from './src/button';
export { PdViewWrites, writesText } from './src/writes';
export { PdViewTable, tableWindow, type TableWindow } from './src/table';
export { PdViewTree, visibleRows, type TreeRow } from './src/tree';
export { PdViewScene, diagramScene, graphScene } from './src/scene';
export { PdViewForm, fieldProblem, initialValues } from './src/form';
export { PdViewReview, reviewCounts, reviewRows, type ReviewRow } from './src/review';
export { PdViewMatrix, matrixIndex } from './src/matrix';
export { PdViewChart, chartSpecOf, chartWords, scatterOption } from './src/chart';
export { PdViewTone, TONE_CSS, toneClass } from './src/tone';
export { ViewStore, type ViewFailure, type ViewStoreType } from './src/view.store';
export { ViewRuntime, type ViewRuntimeContext } from './src/runtime';
export { NO_VIEW_HOST, VIEW_HOST, VIEW_NEEDS_AI, VIEW_OPEN_AI_SETTINGS, type ViewConfirmRequest, type ViewHost, type ViewOpenRequest, type ViewPickRequest, type ViewPicked } from './src/host';
export { applyViewPatch, type PatchProblem, type PatchResult } from './src/patch';
export { VIEW_EXTRA_LIMITS, isWebUrl, validateViewAction, validateViewDoc, type ViewIssue } from './src/limits';
export { sampleViewDoc } from './src/sample';
export { ICON_GLYPH, KNOWN_KINDS, TONE_GLYPH, glyphFor, mergeArgs, primaryNode, shapeOf, toneOf, toneWord, unknownKindName, type KnownKind, type NodeShape } from './src/model';
export { MARK_W, detailLines, hubOf, layers, layoutGraph, layoutSequence, ringsFrom, type Lifeline, type LayoutEdge, type LayoutNode, type Scene, type SceneEdge, type SceneNode } from './src/layout';
// The wire types, generated from the Rust model by `PADDOCK_WRITE_TS=1 cargo test -p paddock-view --lib generated`.
export * from './generated/view';
