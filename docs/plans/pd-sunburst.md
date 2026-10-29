# `pd-sunburst`: a drillable ring chart for hierarchies (paddock-ui)

- Status: spec proposal, 2026-10-28, revised after the independent review of `098daa12a`; first consumer Dyno's disk map
  ([apps/dyno/docs/design/disk-map.md](../../apps/dyno/docs/design/disk-map.md))
- Entry point: `@decryp7/paddock-ui/chart` (beside `pd-chart`, sharing `ring.ts` and the token reader)
- Also in this change: tree rows for `pd-table` (section 7), the accessible companion

## 1. Why a new component, and why not ECharts

Share first: `pd-chart` draws line, area, bar, stacked bar and donut through ECharts, and ECharts has a `sunburst`
series with `nodeClick: 'rootToNode'`. It is not used here because:

- **Keyboard and screen readers.** A canvas has no focusable segments; the drill model needs a roving focus over
  segments with tree semantics (`aria-level`, `aria-posinset`). SVG paths can carry them; a canvas cannot.
- **Bounded, monotone live updates.** The consumer feeds deltas while a scan runs; the component must fold small
  segments by angle, cap the rendered count and never tween between ticks. ECharts re-lays out the whole tree per
  `setOption` and animates by default.
- **One colour rule.** The ring-order and CVD rules of `ring.ts` must apply to branch colours; with ECharts they would
  be re-implemented in an option mapper anyway.

So: SVG, a pure layout core, a thin Angular component. At most 2,000 `<path>` elements by contract.

## 2. Data model (generic)

```ts
export interface PdSunburstNode {
  readonly id: string;                 // stable across updates (ADR 0017: keys survive refreshes)
  readonly label: string;
  readonly value: number;              // >= 0; the node's total including children
  readonly children?: readonly PdSunburstNode[];
  readonly hasMore?: boolean;          // children exist but are not loaded: drilling emits `loadChildren`
  readonly kind?: 'item' | 'hidden' | 'pending' | 'other';
                                       // hidden: hatched, not drillable; pending: ghost ("not counted yet");
                                       // other: consumer-side aggregate, neutral, drill emits `openOther`
  readonly detail?: string;            // one line for the centre and the aria-label ("297,155 items")
}
```

Odd input never throws (family rule): `NaN`, negative or non-finite values count as 0; children whose values exceed
the parent are scaled to fit (no warning is logged); a root or folder whose own value is 0, NaN or overflowed draws from the sum of its
children; duplicate ids are skipped; nothing drawable is the empty state; cycles are impossible (a tree of values);
an unknown `centre` id falls back to the root.

## 3. Inputs and outputs

| Input | Type, default | Meaning |
|---|---|---|
| `root` | `PdSunburstNode \| null` | the hierarchy; `null` with `state='loading'` shows the skeleton |
| `centre` | `model<string \| null>(null)` | id of the node in the middle (two-way: the breadcrumb and the table share it) |
| `label` | `string` (required) | accessible name of the chart ("Disk usage of Macintosh HD") |
| `rings` | `number`, 4 (1-6) | levels drawn below the centre |
| `minAngle` | `number`, 1.5 (degrees; min 1) | smaller segments fold into "n smaller items" per parent (0.6° left 1 px slivers between the gaps) |
| `maxChildren` | `number`, 48 | per parent, before folding |
| `maxSegments` | `number`, 1000 (hard 2000) | when exceeded, `minAngle` rises until it fits; `folded` output reports it |
| `scale` | `number \| null`, null | fixed denominator (a volume's used bytes) so arcs only grow during a live fill; applies only while the root is the centre (a drilled folder fills its own circle); `null` = the centre's value (folders, network shares) |
| `colourBy` | `'branch' \| ((node) => number \| 'neutral')`, `'branch'` | branch: the 5 largest children of the centre take ring-ordered chart slots, the rest neutral; a function maps to palette slots by identity |
| `format` | `(value: number) => string`, **required** | value text (centre, labels, aria); no default, because the component is not about bytes (Dyno passes `formatBytes`) |
| `state` | `'ready' \| 'loading' \| 'empty' \| 'error'`, `'ready'` | loading: skeleton ring at final size; empty and error render `SuEmptyState` / `SuErrorState` with projected actions |
| `emptyTitle`, `emptyText`, `errorText` | strings | state copy |
| `live` | `boolean`, false | while true: no tweening, updates coalesced to 4 per second |
| `highlight` | `string \| null`, null | marks a node as if hovered, without emitting `hoverChange` (the consumer's row hover) |
| `motion` | `'auto' \| 'reduce'`, `'auto'` | `auto` follows `reducedMotion()` from `pd-chart` |

| Output | Payload | When |
|---|---|---|
| `centreChange` | `string \| null` | drill in or out (also through the model) |
| `hoverChange` | `PdSunburstPoint \| null` | pointer or focus moves (`{ node, path: ids[], share: value / parent }`) |
| `activate` | `PdSunburstPoint` | Enter or click on a non-drillable segment (hidden space) |
| `loadChildren` | `string` | drilling into a node with `hasMore` (the consumer fetches and updates `root`) |
| `openOther` | `{ parent: string, ids: string[] }` | drilling into a folded or `other` segment |
| `contextMenu` | `{ point, x, y }` | right-click, Shift+F10, Menu key; the consumer opens its CDK menu (no actions are built in) |
| `folded` | `{ segments: number, minAngle: number }` | the cap raised the minimum angle |

Content projection: `ng-template[pdSunburstCentre]` (context: the centre or hovered point) replaces the default centre
(name 13/600, value 28/600 tabular with a 13 px unit, share and detail 12 muted, "Up to <parent>" link).

## 4. Pure core (unit-tested, no Angular)

- `sunburstLayout(root, centreId, opts) → { arcs: PdArc[], folded, scale }`: angles clockwise from 12 o'clock,
  `PdArc { id, depth, a0, a1, r0, r1, slot, tone, kind }`; sizes add up per parent; deterministic for equal values (ties
  by id).
- `arcPath(arc, gapPx)`: annular sector with a constant pixel gap at every radius (1.8 px), full ring as two circles.
- `branchSlots(children, palette)`: `ringOrder(palette, 5)` from `ring.ts`, neutral beyond; alternate sibling tone
  (12 % towards `--pd-text`).
- `labelFits(arc, text)`: curved label only when the arc length at the ring's mid radius fits the text plus 18 px and
  the chosen ink reaches 4.5:1 on the fill.
- `nextFocus(arcs, current, key)`: the tree keyboard model (Up/Down among siblings only, stopping at the ends; Right to
  the first child, Left to the parent: the only keys that change level; Home/End among siblings; type-ahead). No
  Backspace binding (it belongs to text fields); going up a centre is the consumer's command (`⌘↑` / `Alt+Up`).

## 5. Accessibility

- **The consumer's table is the declared accessible equivalent** (WCAG 1.1.1, 2.1.1): every consumer pairs the chart
  with a list or table that carries every value. The SVG tree semantics below are **experimental**: ARIA tree roles on
  SVG elements are not verified with VoiceOver in WKWebView (macOS shells) or with Narrator in WebView2, so they are
  tested per platform and nothing depends on them.

- The `<svg>` has `role="tree"` and `aria-label`; each segment is a `role="treeitem"` `<path>` with `tabindex` -1/0
  (roving), `aria-level`, `aria-setsize`, `aria-posinset`, `aria-expanded` (true when its children are drawn),
  `aria-label` = "label, value, share of parent, detail". Folded segments say "37 smaller items, 1.2 GB".
- Focus ring: 2 px accent arc outline 3 px outside the segment (≥ 3:1 on every surface, SC 2.4.13). Hover: a 2 px text
  outline plus fade of non-path segments to 32 %.
- A polite live region announces the new centre after a drill ("Developer, 92.6 GB, 6 folders"), not hover.
- Hit targets: segments under 24 px in both directions are reachable by keyboard and through the table; the pointer
  still works on them (the minimum angle keeps them at least 3 px).
- Never colour alone: direct labels, the centre text, and the consumer's list or table with matching swatches; the
  gallery story shows the pattern.
- Drilling is a 180 ms opacity cross-fade (100 ms with reduced motion); there is no geometric tween in 1.0. Skeleton static under reduced motion.

## 6. Tests

- Layout: sums and angles (property test over random trees), folding by angle and by count, the segment cap,
  monotone arcs with a fixed `scale` when values only grow, ids stable across updates, odd input (NaN, negative,
  empty, a single node, a 10,000-child folder) gives a defined result.
- Colour: the token test runs `branchSlots` over both palettes: ring-adjacent ΔE ≥ 20 in all four visions with both
  tones (today 21.7 dark, 23.5 light), every fill ≥ 3:1 on `--pd-surface-1` (today 6.58 and 3.54), label ink ≥ 4.5:1.
- Keyboard: the tree model over a fixture (every key, wrap rules, type-ahead); drill and back through the model.
- Component (100 % coverage like every component): inputs to DOM, outputs, states, `hasMore` → `loadChildren`, live
  coalescing with a fake clock (`PD_CLOCK_SOURCE`), reduced motion with `data-motion="reduce"`.
- Gallery: a story per state, both themes; Playwright screenshot and axe; a performance test with a 2,000-segment update
  (layout + DOM patch under one 16 ms frame on CI's reference runner, DOM `<path>` count ≤ `maxSegments`).

## 7. Companion: tree rows in `pd-table`

The accessible alternative needs hierarchy in the table: an optional `children: (row) => readonly T[] | null`, `expanded`
model (ids), `level` indentation of 20 px, a chevron cell, `role="treegrid"` with `aria-level` and `aria-expanded`,
Right/Left to open and close, and virtualization over the flattened visible rows. Generic: Debrief's Explorer and
Pitcrew's job steps could use it. Groups (`groupBy`) stay as they are.

## 8. Public API and docs

New exports in `chart/src/public-api` (`PdSunburst`, `PdSunburstNode`, `PdSunburstPoint`, `sunburstLayout`), a
CHANGELOG entry, a design-guide paragraph in §7 Charts ("Sunburst: hierarchies only; four rings; colour by branch with
`ring.ts`; never colour alone"), and the gallery page.
