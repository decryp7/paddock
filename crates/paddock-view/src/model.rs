//! The wire types of a view (ADR 0002). Pure data: JSON (camelCase, for the UI) and postcard (the IPC) both.
//!
//! Wire rules (the family's): no `untagged`, `flatten`, internal tags or `skip_serializing_if`. Enums are externally
//! tagged (`{ "text": { ... } }` in JSON, a unit variant is its name), which postcard encodes by variant index (it cannot
//! decode adjacently or internally tagged enums); a variant is only ever
//! added at the end of an enum (a minor change of `schema`). Every `u64` crosses JavaScript as a number, so the validator
//! keeps them below 2^53.

use crate::grant::GrantRef;
use paddock_ipc::Json;
use serde::{Deserialize, Serialize};

/// The schema version this build renders.
pub const SCHEMA: u16 = 1;

/// A view: what a plugin returns, the host validates and the UI renders. Never HTML, never Markdown.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewDoc {
    /// The schema version ([`SCHEMA`]).
    pub schema: u16,
    /// The view's title (a text label).
    pub title: String,
    /// The content.
    pub root: Node,
    /// The version of this document; a [`ViewPatch`] names the one it was made against and the one it produces.
    #[cfg_attr(test, ts(type = "number"))]
    pub etag: u64,
}

/// One node of the tree: an optional id (what a [`ViewPatch`] addresses) and what it shows.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    /// Empty: the node cannot be patched on its own. Otherwise unique in the document (at most 128 bytes).
    pub id: String,
    /// What the node shows.
    pub kind: NodeKind,
}

impl Node {
    /// A node without an id.
    pub fn new(kind: NodeKind) -> Self {
        Node {
            id: String::new(),
            kind,
        }
    }

    /// The same node with an id.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// A plain text node.
    pub fn text(text: impl Into<String>) -> Self {
        Node::new(NodeKind::Text {
            text: text.into(),
            tone: Tone::Neutral,
            style: TextStyle::Body,
        })
    }

    /// A vertical stack.
    pub fn column(children: Vec<Node>) -> Self {
        Node::new(NodeKind::Stack {
            dir: Dir::Column,
            children,
        })
    }
}

/// What a node shows. New kinds are added at the end; an unknown one shows "update the app to view this" for that node.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NodeKind {
    /// Children in a row or a column.
    Stack { dir: Dir, children: Vec<Node> },
    /// Plain text (bound as text, never as HTML).
    Text {
        text: String,
        tone: Tone,
        style: TextStyle,
    },
    /// A table.
    Table(Table),
    /// A tree (flat, in pre-order).
    Tree(Tree),
    /// A graph the host lays out.
    Graph(Graph),
    /// A diagram the host lays out.
    Diagram(Diagram),
    /// A form; it never submits by itself.
    Form(Form),
    /// Findings with states (accept, dismiss, suppress with a reason).
    ReviewList(ReviewList),
    /// A matrix of cells (requirement by case).
    Matrix(Matrix),
    /// A chart the host draws.
    Chart(ChartSpec),
    /// A short status text.
    Badge { text: String, tone: Tone },
    /// A progress bar; `value` is 0.0 to 1.0.
    Progress { value: f32, label: String },
    /// Nothing to show, with an optional way forward.
    Empty {
        icon: Icon,
        title: String,
        hint: String,
        action: Option<ActionRef>,
    },
    /// The standard "needs local AI" state (ADR 0004).
    NeedsAi(NeedsAi),
    /// A link: an action or a page in the OS browser.
    Link { label: String, target: Link },
}

/// Direction of a stack.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Dir {
    Row,
    Column,
}

/// A semantic tone; the renderer maps it to theme tokens (never a colour on the wire).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tone {
    #[default]
    Neutral,
    Info,
    Good,
    Warn,
    Bad,
}

/// How a text is set.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextStyle {
    #[default]
    Body,
    Heading,
    Subheading,
    Caption,
    /// Monospace, for identifiers and paths.
    Code,
}

/// The closed icon set (never a URL).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Icon {
    Info,
    Warning,
    Error,
    Check,
    Search,
    Folder,
    File,
    Table,
    Graph,
    Chart,
    Settings,
    Ai,
    Lock,
    Link,
}

/// A button-like reference to something the plugin does. The host sends [`ViewAction`] `{ view, action: id, args }`
/// when the user chooses it (plus keys of its own: `row` for a table row, `finding` for a review item, `node` for a
/// graph node or tree node, `values` for a form's fields).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRef {
    /// The plugin's action id. `page` and `expand` are the host's names for paging and lazy trees.
    pub id: String,
    /// The text the user reads (a button or a menu entry); required, so every interactive node is labelled.
    pub label: String,
    /// When set, the host asks this question before it sends the action.
    pub confirm: Option<String>,
    /// Arguments the plugin gets back.
    #[cfg_attr(test, ts(type = "unknown"))]
    pub args: Json,
    /// The action changes something (the UI badges it; Pacenotes never exposes it over MCP).
    pub writes: bool,
}

impl ActionRef {
    /// A read-only action without arguments or confirmation.
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        ActionRef {
            id: id.into(),
            label: label.into(),
            confirm: None,
            args: Json::default(),
            writes: false,
        }
    }
}

/// A link: an action, or a page the host confirms and opens in the OS browser.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Link {
    Action(ActionRef),
    /// An `http` or `https` URL.
    OpenInBrowser {
        url: String,
        label: String,
    },
}

/// A table.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub columns: Vec<Column>,
    pub rows: RowSource,
    /// Chosen by activating a row (the host adds `row`: the index in the shown rows).
    pub row_action: Option<ActionRef>,
    /// How the rows are ordered now.
    pub sort: Option<Sort>,
}

/// A table column.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    /// Unique among the columns; what [`Sort`] names.
    pub key: String,
    /// The header text.
    pub label: String,
    /// The user may ask for this order (the host sends the action `sort` with `column` and `descending`).
    pub sortable: bool,
    /// Where the column's text sits; numbers are [`Align::End`] (right-aligned, tabular).
    pub align: Align,
    /// The unit of the column's values ("ms", "lines"), shown beside the header; empty for none.
    pub unit: String,
}

/// Where a table column's text sits.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    /// The reading start (left in left-to-right text): the default, for words.
    #[default]
    Start,
    /// The reading end: numbers, so the digits line up.
    End,
}

/// The order of a table.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    /// A column key.
    pub column: String,
    pub descending: bool,
}

/// One table cell.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    pub text: String,
    pub tone: Tone,
    pub link: Option<Link>,
}

impl Cell {
    /// A neutral cell without a link.
    pub fn text(text: impl Into<String>) -> Self {
        Cell {
            text: text.into(),
            tone: Tone::Neutral,
            link: None,
        }
    }
}

/// Where a table's rows are.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RowSource {
    /// All rows (at most [`crate::MAX_INLINE_ROWS`]).
    Inline(Vec<Vec<Cell>>),
    /// A window onto `total` rows: the host asks for another page with the action `page` (`page` = the number).
    Paged {
        #[cfg_attr(test, ts(type = "number"))]
        total: u64,
        page_size: u32,
        /// The page shown now (from 0).
        page: u32,
        /// The rows of that page (at most `page_size`).
        rows: Vec<Vec<Cell>>,
    },
}

/// A tree, flat and in pre-order: a node's children follow it with `depth + 1`.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tree {
    pub nodes: Vec<TreeNode>,
    /// Children of `expandable` nodes are fetched with the action `expand` (`node` = the id).
    pub lazy: bool,
}

/// One node of a [`Tree`].
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    /// Unique in the tree.
    pub id: String,
    pub label: String,
    /// 0 for a top-level node; at most one more than the node before it.
    pub depth: u16,
    pub tone: Tone,
    pub icon: Option<Icon>,
    /// Its children are not loaded yet (only in a lazy tree).
    pub expandable: bool,
    /// Chosen by activating the node (the host adds `node`).
    pub action: Option<ActionRef>,
}

/// A graph.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub nodes: Vec<GNode>,
    pub edges: Vec<GEdge>,
    pub layout: Layout,
}

/// A graph node.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GNode {
    /// Unique in the graph.
    pub id: String,
    pub label: String,
    pub tone: Tone,
    /// Chosen by activating the node (the host adds `node`).
    pub action: Option<ActionRef>,
}

/// A directed graph edge between two node ids.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GEdge {
    pub from: String,
    pub to: String,
    /// May be empty.
    pub label: String,
}

/// How the host lays a graph out.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Layout {
    Layered,
    Force,
    Radial,
}

/// A diagram.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagram {
    pub kind: DiagramKind,
    pub model: DiagramModel,
}

/// The notation of a diagram.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DiagramKind {
    C4,
    Class,
    Sequence,
    Flow,
}

/// A diagram's content: elements and the relations between them (for a sequence diagram, in time order).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagramModel {
    pub elements: Vec<DiagramElement>,
    pub relations: Vec<DiagramRelation>,
}

/// A box, class, actor or step.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagramElement {
    /// Unique in the diagram.
    pub id: String,
    pub label: String,
    /// Members of a class, the description of a C4 element; may be empty.
    pub detail: String,
    /// The boundary or package it sits in; may be empty.
    pub group: String,
}

/// A line between two elements.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagramRelation {
    pub from: String,
    pub to: String,
    /// May be empty.
    pub label: String,
}

/// A form. It never submits by itself: the user chooses `submit`, and the host sends the action with `values`
/// (`{ field id: value }`).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Form {
    pub fields: Vec<Field>,
    pub submit: ActionRef,
}

/// One form field.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// Unique in the form.
    pub id: String,
    pub label: String,
    /// May be empty.
    pub help: String,
    pub required: bool,
    pub kind: FieldKind,
}

/// What a field holds.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum FieldKind {
    Text {
        value: String,
        placeholder: String,
        multiline: bool,
    },
    Number {
        value: f64,
        min: Option<f64>,
        max: Option<f64>,
    },
    Choice {
        value: String,
        options: Vec<ChoiceOption>,
    },
    Toggle {
        value: bool,
    },
    /// A file or folder chosen with the host's dialog; the plugin only ever sees the grant.
    Path {
        // ts-rs cannot derive `GrantRef` (validating serde attributes); on the wire it is the string `GrantRef` in view.ts.
        #[cfg_attr(test, ts(type = "GrantRef | null"))]
        value: Option<GrantRef>,
        folder: bool,
    },
    /// A reference to a secret held by the host (never the secret itself).
    SecretRef {
        value: Option<SecretRef>,
    },
}

/// One option of a choice field.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceOption {
    pub value: String,
    pub label: String,
}

/// The name of a secret in the host's keychain.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecretRef(pub String);

/// Findings to triage.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewList {
    pub items: Vec<Finding>,
    pub states: Vec<StateDef>,
}

/// One finding.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// Unique in the list.
    pub id: String,
    pub title: String,
    /// May be empty.
    pub detail: String,
    /// Where it is (a path, a line); may be empty.
    pub location: String,
    pub tone: Tone,
    /// The id of one of the list's states.
    pub state: String,
    pub link: Option<Link>,
}

/// A state a finding can be in, with what the user may do from it.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDef {
    /// Unique in the list.
    pub id: String,
    pub label: String,
    pub tone: Tone,
    /// The actions offered for a finding in this state (the host adds `finding`, and `reason` when `needs_reason`).
    pub actions: Vec<ActionRef>,
    /// The host asks the user for a reason before it sends an action of this state.
    pub needs_reason: bool,
}

/// A matrix; cells are sparse.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Matrix {
    pub rows: Vec<Label>,
    pub cols: Vec<Label>,
    pub cells: Vec<MatrixCell>,
}

/// A row or column heading.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub id: String,
    pub text: String,
}

/// One cell of a matrix, at a row and a column index.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixCell {
    pub row: u32,
    pub col: u32,
    pub text: String,
    pub tone: Tone,
    pub action: Option<ActionRef>,
}

/// A chart the host draws with its chart library.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChartSpec {
    pub kind: ChartKind,
    pub title: String,
    /// The x axis (or the slices); every series has one value per category.
    pub categories: Vec<String>,
    pub series: Vec<Series>,
    /// The unit of the values ("s", "%"), for the value axis and the summary; empty for none.
    pub unit: String,
}

/// The kind of a chart.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChartKind {
    Line,
    Bar,
    Area,
    Pie,
    Scatter,
}

/// One data series.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Series {
    pub name: String,
    pub values: Vec<f64>,
}

/// The standard "needs local AI" state (ADR 0004).
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NeedsAi {
    /// What needs it ("Explain this log").
    pub feature: String,
    /// Why it cannot run now; may be empty.
    pub reason: String,
    /// The way forward ("Set up Pitwall").
    pub setup: ActionRef,
}

/// What the host sends when the user acts on a view.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewAction {
    /// The view's id (the contribution it belongs to).
    pub view: String,
    /// The [`ActionRef::id`].
    pub action: String,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub args: Json,
}

/// What the plugin answers a [`ViewAction`] with.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ViewPatch {
    /// Replace the subtree with this id.
    Replace {
        /// The etag of the document the patch was made against.
        #[cfg_attr(test, ts(type = "number"))]
        base_etag: u64,
        /// The etag after the patch (not the base one).
        #[cfg_attr(test, ts(type = "number"))]
        etag: u64,
        id: String,
        node: Node,
    },
    /// A whole new document; its etag differs from the current one.
    Doc(ViewDoc),
}
