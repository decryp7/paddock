# 2. Declarative view contributions: plugins describe, host apps render

- Status: proposed (2026-10-23)
- Related: ADR 0001; `@decryp7/paddock-ui` (Angular); Pacenotes architecture-direction ("plugins never ship web code")

## Context

Pacenotes plugins (review lists, requirement matrices, call graphs, diagrams), Dyno tier 3 (benchmark results, sensor pages) and
some Debrief plugins want UI. Shipping web code from plugins would break the CSP (no `unsafe-eval`, no remote origins), the
no-`innerHTML` rule, theming, accessibility and the "Angular does not re-implement domain rules" rule.

## Decision

A plugin returns a **`ViewDoc`**: pure data, validated in Rust (`paddock-view`), rendered by a generic Angular
`<pd-view>` component set in `@decryp7/paddock-ui`, with every text bound as text, never HTML. The same JSON schema is
mirrored in `paddock-ui`'s TypeScript types in the same change (AGENTS.md contract rule).

```rust
pub struct ViewDoc { pub schema: u16, pub title: String, pub root: Node, pub etag: u64 }
pub enum Node {
    Stack { dir: Dir, children: Vec<Node> },
    Text { text: String, tone: Tone, style: TextStyle },          // plain text; Markdown is NOT supported
    Table(Table), Tree(Tree), Graph(Graph), Diagram(Diagram),
    Form(Form), ReviewList(ReviewList), Matrix(Matrix),
    Chart(ChartSpec),            // data series + kind; the host draws with ECharts
    Badge { text: String, tone: Tone }, Progress { value: f32, label: String },
    Empty { icon: Icon, title: String, hint: String, action: Option<ActionRef> },
    NeedsAi(NeedsAi),            // the standard "Needs local AI" state (ADR 0004); the host shows the reason and its own Open AI settings
}
pub struct Table  { pub columns: Vec<Column>, pub rows: RowSource, pub row_action: Option<ActionRef>, pub sort: Option<Sort> }
pub enum RowSource { Inline(Vec<Vec<Cell>>), Paged { total: u64, page_size: u32 } } // paged -> host calls ViewAction{ "page", n }
pub struct Tree   { pub nodes: Vec<TreeNode>, pub lazy: bool }             // children fetched via ViewAction "expand"
pub struct Graph  { pub nodes: Vec<GNode>, pub edges: Vec<GEdge>, pub layout: Layout /* Layered | Force | Radial */ }
pub struct Diagram{ pub kind: DiagramKind /* C4 | Class | Sequence | Flow */, pub model: DiagramModel } // host lays out; export to draw.io is a plugin action
pub struct Form   { pub fields: Vec<Field>, pub submit: ActionRef } // text, number, choice, toggle, path (grant-checked), secret-ref
pub struct ReviewList { pub items: Vec<Finding>, pub states: Vec<StateDef> } // accept/dismiss/suppress with reason, via ViewAction
pub struct Matrix { pub rows: Vec<Label>, pub cols: Vec<Label>, pub cells: Vec<MatrixCell> } // e.g. requirement x case
pub struct ActionRef { pub id: String, pub confirm: Option<String>, pub args: Json }
```

**Interaction** is request/response: the host renders, the user acts, the host sends `ViewAction { view, action, args }`, the
plugin answers a `ViewPatch` (replace a subtree by node id, or a whole new doc with a new `etag`). No push from plugins in v1;
a host-side `refresh` interval is allowed. Anything long runs as a job (`Report`), not inside a view call.

**Limits (enforced by `paddock-view::validate`, user input style):** depth 16, 20,000 nodes per doc, 1 MiB per doc, strings
8 KiB, graphs 5,000 nodes / 20,000 edges (larger must be paged or aggregated by the plugin), inline table rows 2,000
(use `Paged`). An invalid doc is a typed error attributed to the plugin and shows the panel's inline error with Retry.

**Security:** icons come from a closed enum, not URLs; links are `ActionRef`s or `OpenInBrowser(url)` that the host confirms and
opens in the OS browser; paths in forms are `GrantRef`s chosen via the host's dialog; secrets fields hold references only. A
form never auto-submits. Views are read-only unless the plugin has the matching permission; write-capable actions are marked
`writes: true` in the manifest so the UI can badge them and Pacenotes never exposes them over MCP.

**Theming and a11y:** nodes carry semantic tone (`Neutral | Info | Good | Warn | Bad`), never colours; the renderer maps to CSS
custom properties and the CVD-validated palette. Every interactive node has a text label.

## Consequences

- New view kinds are added to the enum (minor version of `schema`); the renderer ignores nothing silently: an unknown node
  shows "update the app to view this" for that node only.
- The host app owns rendering, so Debrief, Pacenotes and Dyno look identical.
- Rich custom visuals (timelines like Debrief's) remain built in; plugins cannot add them. Revisit if a real need appears.

## Open questions

Graph layout: host-side layered layout (elkjs) vs plugin-supplied coordinates for large diagrams; draw.io round-trip stays a plugin concern.
