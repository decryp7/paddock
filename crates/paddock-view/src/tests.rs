//! Tests of the view model: round trips through both encodings, every validator rule, patches, the decode guards, a
//! never-panics property and the generated TypeScript.

use crate::*;
use paddock_ipc::Json;
use proptest::prelude::*;
use serde_json::json;

fn action(id: &str) -> ActionRef {
    ActionRef { id: id.into(), label: format!("Do {id}"), confirm: Some("Sure?".into()), args: Json(json!({"n": 1, "f": 1.5})), writes: true }
}

fn cell(t: &str) -> Cell {
    Cell { text: t.into(), tone: Tone::Good, link: Some(Link::OpenInBrowser { url: "https://example.org/a?b=1#c".into(), label: "Docs".into() }) }
}

fn gnode(id: &str) -> GNode {
    GNode { id: id.into(), label: id.to_uppercase(), tone: Tone::Info, action: Some(action("open")) }
}

fn field(id: &str, kind: FieldKind) -> Field {
    Field { id: id.into(), label: format!("Field {id}"), help: "help".into(), required: true, kind }
}

/// A document with every node kind and every field kind.
fn sample() -> ViewDoc {
    let table = Table {
        columns: vec![
            Column { key: "a".into(), label: "A".into(), sortable: true },
            Column { key: "b".into(), label: "B".into(), sortable: false },
        ],
        rows: RowSource::Inline(vec![vec![cell("1"), Cell::text("2")]]),
        row_action: Some(action("row")),
        sort: Some(Sort { column: "a".into(), descending: true }),
    };
    let paged = Table {
        columns: vec![Column { key: "a".into(), label: "A".into(), sortable: false }],
        rows: RowSource::Paged { total: 5000, page_size: 100, page: 2, rows: vec![vec![Cell::text("x")]] },
        row_action: None,
        sort: None,
    };
    let tree = Tree {
        lazy: true,
        nodes: vec![
            TreeNode { id: "t0".into(), label: "root".into(), depth: 0, tone: Tone::Neutral, icon: Some(Icon::Folder), expandable: false, action: None },
            TreeNode { id: "t1".into(), label: "child".into(), depth: 1, tone: Tone::Warn, icon: None, expandable: true, action: Some(action("pick")) },
            TreeNode { id: "t2".into(), label: "sibling".into(), depth: 0, tone: Tone::Bad, icon: Some(Icon::File), expandable: false, action: None },
        ],
    };
    let graph = Graph {
        nodes: vec![gnode("a"), gnode("b")],
        edges: vec![GEdge { from: "a".into(), to: "b".into(), label: "calls".into() }],
        layout: Layout::Layered,
    };
    let diagram = Diagram {
        kind: DiagramKind::Sequence,
        model: DiagramModel {
            elements: vec![
                DiagramElement { id: "u".into(), label: "User".into(), detail: String::new(), group: "ctx".into() },
                DiagramElement { id: "s".into(), label: "System".into(), detail: "d".into(), group: String::new() },
            ],
            relations: vec![DiagramRelation { from: "u".into(), to: "s".into(), label: "asks".into() }],
        },
    };
    let form = Form {
        fields: vec![
            field("t", FieldKind::Text { value: "v".into(), placeholder: "p".into(), multiline: true }),
            field("n", FieldKind::Number { value: 2.5, min: Some(0.0), max: Some(10.0) }),
            field("c", FieldKind::Choice { value: "x".into(), options: vec![ChoiceOption { value: "x".into(), label: "X".into() }] }),
            field("g", FieldKind::Toggle { value: true }),
            field("p", FieldKind::Path { value: Some(GrantRef("src".into())), folder: true }),
            field("s", FieldKind::SecretRef { value: Some(SecretRef("api-key".into())) }),
        ],
        submit: action("save"),
    };
    let review = ReviewList {
        items: vec![Finding {
            id: "f1".into(),
            title: "Leak".into(),
            detail: "d".into(),
            location: "a.rs:3".into(),
            tone: Tone::Bad,
            state: "open".into(),
            link: Some(Link::Action(action("go"))),
        }],
        states: vec![StateDef { id: "open".into(), label: "Open".into(), tone: Tone::Warn, actions: vec![action("dismiss")], needs_reason: true }],
    };
    let matrix = Matrix {
        rows: vec![Label { id: "r1".into(), text: "R1".into() }],
        cols: vec![Label { id: "c1".into(), text: "C1".into() }, Label { id: "c2".into(), text: "C2".into() }],
        cells: vec![MatrixCell { row: 0, col: 1, text: "ok".into(), tone: Tone::Good, action: Some(action("m")) }],
    };
    let chart = ChartSpec {
        kind: ChartKind::Line,
        title: "Load".into(),
        categories: vec!["a".into(), "b".into()],
        series: vec![Series { name: "cpu".into(), values: vec![0.5, 1.25] }],
    };
    let kinds = vec![
        Node::new(NodeKind::Text { text: "hi".into(), tone: Tone::Info, style: TextStyle::Heading }),
        Node::new(NodeKind::Table(table)).with_id("tbl"),
        Node::new(NodeKind::Table(paged)),
        Node::new(NodeKind::Tree(tree)),
        Node::new(NodeKind::Graph(graph)),
        Node::new(NodeKind::Diagram(diagram)),
        Node::new(NodeKind::Form(form)),
        Node::new(NodeKind::ReviewList(review)),
        Node::new(NodeKind::Matrix(matrix)),
        Node::new(NodeKind::Chart(chart)),
        Node::new(NodeKind::Badge { text: "new".into(), tone: Tone::Good }),
        Node::new(NodeKind::Progress { value: 0.25, label: "25%".into() }),
        Node::new(NodeKind::Empty { icon: Icon::Search, title: "Nothing".into(), hint: "Try".into(), action: Some(action("retry")) }),
        Node::new(NodeKind::NeedsAi(NeedsAi { feature: "Explain".into(), reason: "No model".into(), setup: ActionRef::new("setup", "Set up Pitwall") })),
        Node::new(NodeKind::Link { label: "Site".into(), target: Link::OpenInBrowser { url: "http://x.test".into(), label: "Site".into() } }),
        Node::text("leaf").with_id("leaf"),
    ];
    ViewDoc {
        schema: SCHEMA,
        title: "Sample".into(),
        root: Node::new(NodeKind::Stack { dir: Dir::Row, children: kinds }).with_id("root"),
        etag: 7,
    }
}

fn with(f: impl FnOnce(&mut ViewDoc)) -> ViewDoc {
    let mut d = sample();
    f(&mut d);
    d
}

/// The root's child kinds, to edit.
fn child(d: &mut ViewDoc, i: usize) -> &mut NodeKind {
    match &mut d.root.kind {
        NodeKind::Stack { children, .. } => &mut children[i].kind,
        _ => unreachable!(),
    }
}

fn doc_of(kind: NodeKind) -> ViewDoc {
    ViewDoc { schema: SCHEMA, title: "T".into(), root: Node::new(kind), etag: 1 }
}

fn nested(levels: usize) -> Node {
    let mut n = Node::text("x");
    for _ in 0..levels {
        n = Node::column(vec![n]);
    }
    n
}

#[test]
fn the_sample_is_valid_and_round_trips_through_json_and_postcard() {
    let doc = sample();
    validate(&doc).expect("valid");
    let json = to_json(&doc).unwrap();
    assert_eq!(from_json::<ViewDoc>(&json).unwrap(), doc);
    let bytes = to_postcard(&doc).unwrap();
    assert_eq!(from_postcard::<ViewDoc>(&bytes).unwrap(), doc);
}

#[test]
fn json_is_camel_case_and_externally_tagged() {
    let v: serde_json::Value = serde_json::from_str(&to_json(&sample()).unwrap()).unwrap();
    assert_eq!(v["root"]["kind"]["stack"]["dir"], "row");
    let table = &v["root"]["kind"]["stack"]["children"][1]["kind"]["table"];
    assert!(table["rowAction"].is_object());
    assert!(table["rows"]["inline"].is_array());
    // Arguments are the JSON value itself, not text.
    assert_eq!(table["rowAction"]["args"], json!({"n": 1, "f": 1.5}));
}

#[test]
fn actions_and_patches_round_trip_through_both_encodings() {
    let a = ViewAction { view: "v".into(), action: "page".into(), args: Json(json!({"page": 3, "nested": {"k": [1, null, "s"]}})) };
    assert_eq!(from_json::<ViewAction>(&to_json(&a).unwrap()).unwrap(), a);
    assert_eq!(from_postcard::<ViewAction>(&to_postcard(&a).unwrap()).unwrap(), a);
    for p in [
        ViewPatch::Replace { base_etag: 1, etag: 2, id: "n".into(), node: Node::text("t") },
        ViewPatch::Doc(sample()),
    ] {
        assert_eq!(from_json::<ViewPatch>(&to_json(&p).unwrap()).unwrap(), p);
        assert_eq!(from_postcard::<ViewPatch>(&to_postcard(&p).unwrap()).unwrap(), p);
    }
}

#[test]
fn every_enum_variant_round_trips() {
    for t in [Tone::Neutral, Tone::Info, Tone::Good, Tone::Warn, Tone::Bad] {
        assert_eq!(from_postcard::<Tone>(&to_postcard(&t).unwrap()).unwrap(), t);
    }
    for i in [Icon::Info, Icon::Warning, Icon::Error, Icon::Check, Icon::Search, Icon::Folder, Icon::File, Icon::Table, Icon::Graph, Icon::Chart, Icon::Settings, Icon::Ai, Icon::Lock, Icon::Link] {
        assert_eq!(from_json::<Icon>(&to_json(&i).unwrap()).unwrap(), i);
    }
    for k in [ChartKind::Line, ChartKind::Bar, ChartKind::Area, ChartKind::Pie, ChartKind::Scatter] {
        assert_eq!(from_postcard::<ChartKind>(&to_postcard(&k).unwrap()).unwrap(), k);
    }
    for k in [DiagramKind::C4, DiagramKind::Class, DiagramKind::Sequence, DiagramKind::Flow] {
        assert_eq!(from_postcard::<DiagramKind>(&to_postcard(&k).unwrap()).unwrap(), k);
    }
    for l in [Layout::Layered, Layout::Force, Layout::Radial] {
        assert_eq!(from_postcard::<Layout>(&to_postcard(&l).unwrap()).unwrap(), l);
    }
    for s in [TextStyle::Body, TextStyle::Heading, TextStyle::Subheading, TextStyle::Caption, TextStyle::Code] {
        assert_eq!(from_postcard::<TextStyle>(&to_postcard(&s).unwrap()).unwrap(), s);
    }
    assert_eq!(Tone::default(), Tone::Neutral);
    assert_eq!(TextStyle::default(), TextStyle::Body);
}

#[test]
fn the_builders_make_plain_nodes() {
    let n = Node::column(vec![Node::text("a")]).with_id("c");
    assert_eq!(n.id, "c");
    assert!(matches!(n.kind, NodeKind::Stack { dir: Dir::Column, .. }));
    let a = ActionRef::new("i", "L");
    assert!(!a.writes && a.confirm.is_none());
}

// ---- validate ----

#[test]
fn a_schema_this_build_does_not_render_is_refused() {
    let e = validate(&with(|d| d.schema = 2)).unwrap_err();
    assert_eq!(e, ViewError::UnsupportedSchema { found: 2, supported: SCHEMA });
    assert!(e.to_string().contains("schema 2"));
}

#[test]
fn numbers_that_javascript_cannot_hold_are_refused() {
    assert!(validate(&with(|d| d.etag = MAX_SAFE_INT)).is_ok());
    assert!(matches!(validate(&with(|d| d.etag = MAX_SAFE_INT + 1)), Err(ViewError::Invalid { .. })));
    let paged = |total| {
        doc_of(NodeKind::Table(Table {
            columns: vec![],
            rows: RowSource::Paged { total, page_size: 10, page: 0, rows: vec![] },
            row_action: None,
            sort: None,
        }))
    };
    assert!(validate(&paged(MAX_SAFE_INT)).is_ok());
    assert!(matches!(validate(&paged(MAX_SAFE_INT + 1)), Err(ViewError::Invalid { .. })));
}

#[test]
fn depth_is_limited_and_the_error_names_the_path() {
    let ok = ViewDoc { root: nested(MAX_DEPTH - 2), ..sample() };
    // nested(n) has n + 1 levels of stacks/leaf; MAX_DEPTH - 2 stacks and a leaf = MAX_DEPTH - 1 levels.
    assert!(validate(&ok).is_ok());
    let at_limit = ViewDoc { root: nested(MAX_DEPTH - 1), ..sample() };
    assert!(validate(&at_limit).is_ok());
    let deep = ViewDoc { root: nested(MAX_DEPTH), ..sample() };
    let e = validate(&deep).unwrap_err();
    let ViewError::TooDeep { path } = &e else { panic!("{e:?}") };
    assert!(path.starts_with("root.children[0]") && path.matches("children").count() == MAX_DEPTH);
    assert!(e.to_string().contains("nested deeper"));
    // A very deep hand-built document is refused without recursing to the bottom.
    let very = ViewDoc { root: nested(1000), ..sample() };
    assert!(matches!(validate(&very), Err(ViewError::TooDeep { .. })));
}

#[test]
fn the_node_count_is_limited() {
    let leaves = |n: usize| ViewDoc { root: Node::column((0..n).map(|_| Node::text("x")).collect()), ..sample() };
    assert!(validate(&leaves(MAX_NODES - 1)).is_ok());
    let e = validate(&leaves(MAX_NODES)).unwrap_err();
    assert!(matches!(e, ViewError::TooManyNodes { .. }), "{e:?}");
    assert!(e.to_string().contains("20000"));
    // Rows count too.
    let rows = |n| {
        let row = vec![Cell::text("x")];
        doc_of(NodeKind::Table(Table {
            columns: vec![Column { key: "a".into(), label: "A".into(), sortable: false }],
            rows: RowSource::Inline(vec![row; n]),
            row_action: None,
            sort: None,
        }))
    };
    let two = ViewDoc { root: Node::column(vec![Node::new(rows(MAX_INLINE_ROWS).root.kind); 10]), ..sample() };
    assert!(matches!(validate(&two), Err(ViewError::TooManyNodes { .. })));
}

#[test]
fn the_encoded_size_is_limited() {
    // 200 strings of 7 KiB are under every other limit and over 1 MiB together.
    let big = "x".repeat(7 * 1024);
    let doc = ViewDoc { root: Node::column((0..200).map(|_| Node::text(big.clone())).collect()), ..sample() };
    let e = validate(&doc).unwrap_err();
    assert!(matches!(e, ViewError::TooLarge { bytes } if bytes > MAX_ENCODED), "{e:?}");
    assert!(e.to_string().contains("encoded"));
    assert!(matches!(to_json(&doc), Err(ViewError::TooLarge { .. })));
    assert!(matches!(to_postcard(&doc), Err(ViewError::TooLarge { .. })));
}

#[test]
fn strings_are_limited() {
    let long = "x".repeat(MAX_STRING + 1);
    let e = validate(&doc_of(NodeKind::Text { text: long.clone(), tone: Tone::Neutral, style: TextStyle::Body })).unwrap_err();
    assert_eq!(e, ViewError::StringTooLong { path: "root.text".into(), len: MAX_STRING + 1 });
    assert!(e.to_string().contains("8192"));
    assert!(validate(&doc_of(NodeKind::Text { text: "x".repeat(MAX_STRING), tone: Tone::Neutral, style: TextStyle::Body })).is_ok());
    assert!(matches!(validate(&with(|d| d.title = long.clone())), Err(ViewError::StringTooLong { .. })));
}

#[test]
fn ids_are_bounded_and_unique() {
    let e = validate(&with(|d| d.root.id = "x".repeat(MAX_ID + 1))).unwrap_err();
    assert!(matches!(e, ViewError::Invalid { .. }));
    let dup = ViewDoc { root: Node::column(vec![Node::text("a").with_id("same"), Node::text("b").with_id("same")]), ..sample() };
    let e = validate(&dup).unwrap_err();
    assert_eq!(e, ViewError::DuplicateId { path: "root.children[1]".into(), id: "same".into() });
    assert!(e.to_string().contains("used twice"));
    // Nodes without an id never clash.
    let none = ViewDoc { root: Node::column(vec![Node::text("a"), Node::text("b")]), ..sample() };
    assert!(validate(&none).is_ok());
}

#[test]
fn interactive_nodes_need_a_label() {
    let blank = |f: &dyn Fn(&mut ViewDoc)| validate(&with(|d| f(d))).unwrap_err();
    assert!(matches!(blank(&|d| d.title = "  ".into()), ViewError::Unlabelled { .. }));
    let e = blank(&|d| {
        *child(d, 12) = NodeKind::Empty { icon: Icon::Info, title: "t".into(), hint: String::new(), action: Some(ActionRef::new("a", " ")) }
    });
    assert_eq!(e, ViewError::Unlabelled { path: "root.children[12].action.label".into() });
    assert!(e.to_string().contains("text label"));
    assert!(matches!(blank(&|d| *child(d, 10) = NodeKind::Badge { text: String::new(), tone: Tone::Neutral }), ViewError::Unlabelled { .. }));
    assert!(matches!(
        blank(&|d| *child(d, 14) = NodeKind::Link { label: String::new(), target: Link::Action(action("a")) }),
        ViewError::Unlabelled { .. }
    ));
    assert!(matches!(
        blank(&|d| *child(d, 14) = NodeKind::Link { label: "L".into(), target: Link::OpenInBrowser { url: "https://a.b".into(), label: String::new() } }),
        ViewError::Unlabelled { .. }
    ));
    assert!(matches!(blank(&|d| *child(d, 12) = NodeKind::Empty { icon: Icon::Info, title: String::new(), hint: String::new(), action: None }), ViewError::Unlabelled { .. }));
    // An action needs an id, a confirm question that says something, and bounded arguments.
    let mut a = action("x");
    a.id = String::new();
    assert!(matches!(validate_node_action(a), ViewError::Invalid { .. }));
    let mut a = action("x");
    a.confirm = Some(String::new());
    assert!(matches!(validate_node_action(a), ViewError::Unlabelled { .. }));
    let mut a = action("x");
    a.args = Json(json!("y".repeat(MAX_ARGS + 1)));
    assert!(matches!(validate_node_action(a), ViewError::Invalid { .. }));
    let mut a = action("x");
    a.args = Json(json!("y".repeat(MAX_ARGS - 100)));
    assert!(validate_doc_action(a).is_ok());
}

fn validate_doc_action(a: ActionRef) -> Result<(), ViewError> {
    validate(&doc_of(NodeKind::NeedsAi(NeedsAi { feature: "F".into(), reason: String::new(), setup: a })))
}

fn validate_node_action(a: ActionRef) -> ViewError {
    validate_doc_action(a).unwrap_err()
}

#[test]
fn open_in_browser_takes_http_and_https_only() {
    let link = |url: &str| validate(&doc_of(NodeKind::Link { label: "L".into(), target: Link::OpenInBrowser { url: url.into(), label: "L".into() } }));
    for ok in ["http://a.b", "https://a.b/c?d#e", "HTTPS://A.B", "https://a.b:8080/x", "https://[::1]/x"] {
        assert!(link(ok).is_ok(), "{ok}");
    }
    for bad in [
        "javascript:alert(1)",
        "file:///etc/passwd",
        "ftp://a.b",
        "data:text/html,x",
        "https://",
        "http:///x",
        "https://user@a.b",
        "https://good.com@evil.com/",
        "https://a b",
        "https://a.b/\n",
        "https://:80/x",
        "//a.b",
        "",
        "ünïcödé://x",
        "https://a.b/\u{a0}",
    ] {
        let e = link(bad).unwrap_err();
        assert!(matches!(e, ViewError::BadUrl { ref path } if path == "root.target.url"), "{bad}: {e:?}");
    }
    assert!(link("https://a.b").is_ok());
    let e = link(&format!("https://a.b/{}", "x".repeat(MAX_URL))).unwrap_err();
    assert!(matches!(e, ViewError::StringTooLong { .. }));
    assert!(ViewError::BadUrl { path: "p".into() }.to_string().contains("http and https"));
}

#[test]
fn tables_check_columns_rows_paging_and_sort() {
    let table = |f: &dyn Fn(&mut Table)| {
        let NodeKind::Table(mut t) = (match sample().root.kind {
            NodeKind::Stack { mut children, .. } => children.remove(1).kind,
            _ => unreachable!(),
        }) else {
            unreachable!()
        };
        f(&mut t);
        validate(&doc_of(NodeKind::Table(t)))
    };
    assert!(table(&|_| {}).is_ok());
    assert!(matches!(table(&|t| t.columns[1].key = "a".into()), Err(ViewError::DuplicateId { .. })));
    assert!(matches!(table(&|t| t.columns[0].key = String::new()), Err(ViewError::Invalid { .. })));
    assert!(matches!(table(&|t| t.columns[0].label = String::new()), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(table(&|t| t.sort = Some(Sort { column: "zz".into(), descending: false })), Err(ViewError::Invalid { .. })));
    assert!(matches!(table(&|t| t.row_action = Some(ActionRef::new("", "L"))), Err(ViewError::Invalid { .. })));
    let e = table(&|t| t.rows = RowSource::Inline(vec![vec![Cell::text("only one")]])).unwrap_err();
    assert!(matches!(e, ViewError::Invalid { ref path, .. } if path == "root.rows[0]"), "{e:?}");
    let e = table(&|t| {
        t.rows = RowSource::Inline(vec![vec![Cell::text("1"), Cell { text: "x".repeat(MAX_STRING + 1), tone: Tone::Neutral, link: None }]])
    })
    .unwrap_err();
    assert!(matches!(e, ViewError::StringTooLong { ref path, .. } if path == "root.rows[0][1]"));
    let e = table(&|t| {
        t.rows = RowSource::Inline(vec![vec![Cell { text: "1".into(), tone: Tone::Neutral, link: Some(Link::OpenInBrowser { url: "ftp://x".into(), label: "L".into() }) }, Cell::text("2")]])
    })
    .unwrap_err();
    assert!(matches!(e, ViewError::BadUrl { .. }));
    let many = vec![vec![Cell::text("1"), Cell::text("2")]; MAX_INLINE_ROWS + 1];
    let e = table(&|t| t.rows = RowSource::Inline(many.clone())).unwrap_err();
    assert_eq!(e, ViewError::TooManyRows { path: "root.rows".into(), rows: MAX_INLINE_ROWS + 1 });
    assert!(e.to_string().contains("paged"));
    let exactly = vec![vec![Cell::text("1"), Cell::text("2")]; MAX_INLINE_ROWS];
    assert!(table(&|t| t.rows = RowSource::Inline(exactly.clone())).is_ok());
    // Paging.
    let paged = |total: u64, page_size: u32, page: u32, rows: usize| {
        table(&|t| t.rows = RowSource::Paged { total, page_size, page, rows: vec![vec![Cell::text("1"), Cell::text("2")]; rows] })
    };
    assert!(paged(100, 10, 9, 10).is_ok());
    assert!(paged(0, 10, 0, 0).is_ok());
    assert!(matches!(paged(100, 0, 0, 0), Err(ViewError::Invalid { .. })));
    assert!(matches!(paged(100, MAX_INLINE_ROWS as u32 + 1, 0, 0), Err(ViewError::Invalid { .. })));
    assert!(matches!(paged(100, 10, 0, 11), Err(ViewError::TooManyRows { .. })));
    assert!(matches!(paged(100, 10, 10, 0), Err(ViewError::Invalid { .. })));
    assert!(matches!(paged(15, 10, 1, 6), Err(ViewError::Invalid { .. })));
    assert!(matches!(paged(15, 10, 1, 5), Ok(())));
}

#[test]
fn trees_are_flat_and_ordered() {
    let tree = |f: &dyn Fn(&mut Tree)| {
        let mut t = match child(&mut sample(), 3).clone() {
            NodeKind::Tree(t) => t,
            _ => unreachable!(),
        };
        f(&mut t);
        validate(&doc_of(NodeKind::Tree(t)))
    };
    assert!(tree(&|_| {}).is_ok());
    assert!(matches!(tree(&|t| t.nodes[0].id = t.nodes[1].id.clone()), Err(ViewError::DuplicateId { .. })));
    assert!(matches!(tree(&|t| t.nodes[0].id = String::new()), Err(ViewError::Invalid { .. })));
    assert!(matches!(tree(&|t| t.nodes[0].label = String::new()), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(tree(&|t| t.nodes[0].depth = 1), Err(ViewError::Invalid { .. })));
    assert!(matches!(tree(&|t| t.nodes[1].depth = 2), Err(ViewError::Invalid { .. })));
    assert!(matches!(tree(&|t| t.lazy = false), Err(ViewError::Invalid { .. })));
    assert!(matches!(tree(&|t| t.nodes[1].action = Some(ActionRef::new("a", ""))), Err(ViewError::Unlabelled { .. })));
    // Depth counts the tree's own levels.
    let chain = |n: usize| {
        let nodes = (0..n)
            .map(|i| TreeNode { id: format!("n{i}"), label: "n".into(), depth: i as u16, tone: Tone::Neutral, icon: None, expandable: false, action: None })
            .collect();
        validate(&doc_of(NodeKind::Tree(Tree { nodes, lazy: false })))
    };
    assert!(chain(MAX_DEPTH - 1).is_ok());
    assert!(matches!(chain(MAX_DEPTH), Err(ViewError::TooDeep { .. })));
}

#[test]
fn graphs_and_diagrams_check_size_ids_and_edges() {
    let graph = |nodes: usize, edges: usize| {
        let ns = (0..nodes).map(|i| GNode { id: format!("n{i}"), label: "n".into(), tone: Tone::Neutral, action: None }).collect();
        let es = (0..edges).map(|_| GEdge { from: "n0".into(), to: "n0".into(), label: String::new() }).collect();
        validate(&doc_of(NodeKind::Graph(Graph { nodes: ns, edges: es, layout: Layout::Force })))
    };
    assert!(graph(MAX_GRAPH_NODES, MAX_GRAPH_EDGES).is_ok());
    let e = graph(MAX_GRAPH_NODES + 1, 0).unwrap_err();
    assert_eq!(e, ViewError::GraphTooLarge { path: "root".into(), nodes: MAX_GRAPH_NODES + 1, edges: 0 });
    assert!(e.to_string().contains("5000"));
    assert!(matches!(graph(1, MAX_GRAPH_EDGES + 1), Err(ViewError::GraphTooLarge { .. })));
    let g = |f: &dyn Fn(&mut Graph)| {
        let mut g = Graph { nodes: vec![gnode("a"), gnode("b")], edges: vec![GEdge { from: "a".into(), to: "b".into(), label: "l".into() }], layout: Layout::Radial };
        f(&mut g);
        validate(&doc_of(NodeKind::Graph(g)))
    };
    assert!(g(&|_| {}).is_ok());
    assert!(matches!(g(&|g| g.nodes[1].id = "a".into()), Err(ViewError::DuplicateId { .. })));
    assert!(matches!(g(&|g| g.nodes[1].label = String::new()), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(g(&|g| g.nodes[1].id = String::new()), Err(ViewError::Invalid { .. })));
    assert!(matches!(g(&|g| g.nodes[0].action = Some(ActionRef::new("x", ""))), Err(ViewError::Unlabelled { .. })));
    let e = g(&|g| g.edges[0].to = "zz".into()).unwrap_err();
    assert!(matches!(e, ViewError::Invalid { ref path, .. } if path == "root.edges[0].to"));
    assert!(matches!(g(&|g| g.edges[0].from = "zz".into()), Err(ViewError::Invalid { .. })));
    assert!(matches!(g(&|g| g.edges[0].label = "x".repeat(MAX_STRING + 1)), Err(ViewError::StringTooLong { .. })));
    // Diagrams share the rules.
    let d = |f: &dyn Fn(&mut DiagramModel)| {
        let NodeKind::Diagram(mut d) = child(&mut sample(), 5).clone() else { unreachable!() };
        f(&mut d.model);
        validate(&doc_of(NodeKind::Diagram(d)))
    };
    assert!(d(&|_| {}).is_ok());
    assert!(matches!(d(&|m| m.elements[1].id = "u".into()), Err(ViewError::DuplicateId { .. })));
    assert!(matches!(d(&|m| m.elements[1].id = String::new()), Err(ViewError::Invalid { .. })));
    assert!(matches!(d(&|m| m.elements[1].label = String::new()), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(d(&|m| m.elements[1].detail = "x".repeat(MAX_STRING + 1)), Err(ViewError::StringTooLong { .. })));
    assert!(matches!(d(&|m| m.elements[1].group = "x".repeat(MAX_STRING + 1)), Err(ViewError::StringTooLong { .. })));
    assert!(matches!(d(&|m| m.relations[0].from = "zz".into()), Err(ViewError::Invalid { .. })));
    assert!(matches!(
        d(&|m| m.relations = vec![m.relations[0].clone(); MAX_GRAPH_EDGES + 1]),
        Err(ViewError::GraphTooLarge { .. })
    ));
}

#[test]
fn forms_check_fields() {
    let form = |f: &dyn Fn(&mut Form)| {
        let NodeKind::Form(mut form) = child(&mut sample(), 6).clone() else { unreachable!() };
        f(&mut form);
        validate(&doc_of(NodeKind::Form(form)))
    };
    assert!(form(&|_| {}).is_ok());
    assert!(matches!(form(&|f| f.fields[1].id = "t".into()), Err(ViewError::DuplicateId { .. })));
    assert!(matches!(form(&|f| f.fields[1].id = String::new()), Err(ViewError::Invalid { .. })));
    assert!(matches!(form(&|f| f.fields[1].label = String::new()), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(form(&|f| f.fields[1].help = "x".repeat(MAX_STRING + 1)), Err(ViewError::StringTooLong { .. })));
    assert!(matches!(form(&|f| f.submit.label = String::new()), Err(ViewError::Unlabelled { .. })));
    let kind = |k: FieldKind| form(&|f| f.fields[0].kind = k.clone());
    let long = "x".repeat(MAX_STRING + 1);
    assert!(matches!(kind(FieldKind::Text { value: long.clone(), placeholder: String::new(), multiline: false }), Err(ViewError::StringTooLong { .. })));
    assert!(matches!(kind(FieldKind::Text { value: String::new(), placeholder: long.clone(), multiline: false }), Err(ViewError::StringTooLong { .. })));
    let num = |value: f64, min: Option<f64>, max: Option<f64>| kind(FieldKind::Number { value, min, max });
    assert!(num(1.0, None, None).is_ok());
    assert!(num(1.0, Some(1.0), Some(1.0)).is_ok());
    assert!(matches!(num(f64::NAN, None, None), Err(ViewError::Invalid { .. })));
    assert!(matches!(num(1.0, Some(f64::NEG_INFINITY), None), Err(ViewError::Invalid { .. })));
    assert!(matches!(num(1.0, None, Some(f64::INFINITY)), Err(ViewError::Invalid { .. })));
    assert!(matches!(num(1.0, Some(2.0), Some(1.0)), Err(ViewError::Invalid { .. })));
    assert!(matches!(num(0.5, Some(1.0), None), Err(ViewError::Invalid { .. })));
    assert!(matches!(num(5.0, None, Some(1.0)), Err(ViewError::Invalid { .. })));
    let opt = |v: &str| ChoiceOption { value: v.into(), label: v.to_uppercase() };
    let choice = |value: &str, options: Vec<ChoiceOption>| kind(FieldKind::Choice { value: value.into(), options });
    assert!(choice("", vec![opt("a")]).is_ok());
    assert!(choice("a", vec![opt("a"), opt("b")]).is_ok());
    assert!(matches!(choice("z", vec![opt("a")]), Err(ViewError::Invalid { .. })));
    assert!(matches!(choice("a", vec![opt("a"), opt("a")]), Err(ViewError::DuplicateId { .. })));
    assert!(matches!(choice("a", vec![ChoiceOption { value: "a".into(), label: String::new() }]), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(choice("a", vec![ChoiceOption { value: long.clone(), label: "L".into() }]), Err(ViewError::StringTooLong { .. })));
    assert!(kind(FieldKind::Toggle { value: false }).is_ok());
    assert!(kind(FieldKind::Path { value: None, folder: false }).is_ok());
    assert!(matches!(kind(FieldKind::Path { value: Some(GrantRef(String::new())), folder: false }), Err(ViewError::Invalid { .. })));
    assert!(kind(FieldKind::SecretRef { value: None }).is_ok());
    assert!(matches!(kind(FieldKind::SecretRef { value: Some(SecretRef(String::new())) }), Err(ViewError::Invalid { .. })));
}

#[test]
fn review_lists_check_states() {
    let review = |f: &dyn Fn(&mut ReviewList)| {
        let NodeKind::ReviewList(mut r) = child(&mut sample(), 7).clone() else { unreachable!() };
        f(&mut r);
        validate(&doc_of(NodeKind::ReviewList(r)))
    };
    assert!(review(&|_| {}).is_ok());
    let e = review(&|r| r.items[0].state = "gone".into()).unwrap_err();
    assert!(matches!(e, ViewError::Invalid { ref path, .. } if path == "root.items[0].state"));
    assert!(matches!(review(&|r| r.states.push(r.states[0].clone())), Err(ViewError::DuplicateId { .. })));
    assert!(matches!(review(&|r| r.states[0].id = String::new()), Err(ViewError::Invalid { .. })));
    assert!(matches!(review(&|r| r.states[0].label = String::new()), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(review(&|r| r.states[0].actions[0].label = String::new()), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(review(&|r| r.items.push(r.items[0].clone())), Err(ViewError::DuplicateId { .. })));
    assert!(matches!(review(&|r| r.items[0].id = String::new()), Err(ViewError::Invalid { .. })));
    assert!(matches!(review(&|r| r.items[0].title = String::new()), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(review(&|r| r.items[0].detail = "x".repeat(MAX_STRING + 1)), Err(ViewError::StringTooLong { .. })));
    assert!(matches!(review(&|r| r.items[0].location = "x".repeat(MAX_STRING + 1)), Err(ViewError::StringTooLong { .. })));
    assert!(matches!(review(&|r| r.items[0].link = Some(Link::OpenInBrowser { url: "x".into(), label: "L".into() })), Err(ViewError::BadUrl { .. })));
}

#[test]
fn matrices_check_indexes_and_labels() {
    let matrix = |f: &dyn Fn(&mut Matrix)| {
        let NodeKind::Matrix(mut m) = child(&mut sample(), 8).clone() else { unreachable!() };
        f(&mut m);
        validate(&doc_of(NodeKind::Matrix(m)))
    };
    assert!(matrix(&|_| {}).is_ok());
    assert!(matrix(&|m| m.cells[0].row = 1).is_err());
    assert!(matches!(matrix(&|m| m.cells[0].col = 2), Err(ViewError::Invalid { .. })));
    assert!(matches!(matrix(&|m| m.cells.push(m.cells[0].clone())), Err(ViewError::Invalid { .. })));
    assert!(matches!(matrix(&|m| m.cols[1].id = "c1".into()), Err(ViewError::DuplicateId { .. })));
    assert!(matches!(matrix(&|m| m.rows[0].id = String::new()), Err(ViewError::Invalid { .. })));
    assert!(matches!(matrix(&|m| m.rows[0].text = String::new()), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(matrix(&|m| m.cells[0].text = "x".repeat(MAX_STRING + 1)), Err(ViewError::StringTooLong { .. })));
    assert!(matches!(matrix(&|m| m.cells[0].action = Some(ActionRef::new("", "L"))), Err(ViewError::Invalid { .. })));
}

#[test]
fn charts_check_series() {
    let chart = |f: &dyn Fn(&mut ChartSpec)| {
        let NodeKind::Chart(mut c) = child(&mut sample(), 9).clone() else { unreachable!() };
        f(&mut c);
        validate(&doc_of(NodeKind::Chart(c)))
    };
    assert!(chart(&|_| {}).is_ok());
    assert!(matches!(chart(&|c| c.title = "x".repeat(MAX_STRING + 1)), Err(ViewError::StringTooLong { .. })));
    assert!(matches!(chart(&|c| c.categories[0] = "x".repeat(MAX_STRING + 1)), Err(ViewError::StringTooLong { .. })));
    assert!(matches!(chart(&|c| c.series[0].name = String::new()), Err(ViewError::Unlabelled { .. })));
    assert!(matches!(chart(&|c| c.series[0].values.pop().map(drop).unwrap_or(())), Err(ViewError::Invalid { .. })));
    assert!(matches!(chart(&|c| c.series[0].values[0] = f64::NAN), Err(ViewError::Invalid { .. })));
    let many = |n: usize| {
        chart(&|c| {
            c.categories = vec!["c".into(); n];
            c.series = vec![Series { name: "s".into(), values: vec![1.0; n] }];
        })
    };
    assert!(many(MAX_CHART_POINTS).is_ok());
    assert!(matches!(many(MAX_CHART_POINTS + 1), Err(ViewError::Invalid { .. })));
}

#[test]
fn progress_text_and_empty_checks() {
    let p = |value: f32| validate(&doc_of(NodeKind::Progress { value, label: String::new() }));
    assert!(p(0.0).is_ok() && p(1.0).is_ok());
    assert!(matches!(p(1.1), Err(ViewError::Invalid { .. })));
    assert!(matches!(p(-0.1), Err(ViewError::Invalid { .. })));
    assert!(matches!(p(f32::NAN), Err(ViewError::Invalid { .. })));
    assert!(matches!(
        validate(&doc_of(NodeKind::Progress { value: 0.5, label: "x".repeat(MAX_STRING + 1) })),
        Err(ViewError::StringTooLong { .. })
    ));
    assert!(matches!(
        validate(&doc_of(NodeKind::Empty { icon: Icon::Info, title: "t".into(), hint: "x".repeat(MAX_STRING + 1), action: None })),
        Err(ViewError::StringTooLong { .. })
    ));
    assert!(matches!(
        validate(&doc_of(NodeKind::NeedsAi(NeedsAi { feature: String::new(), reason: String::new(), setup: action("s") }))),
        Err(ViewError::Unlabelled { .. })
    ));
    assert!(matches!(
        validate(&doc_of(NodeKind::NeedsAi(NeedsAi { feature: "f".into(), reason: "x".repeat(MAX_STRING + 1), setup: action("s") }))),
        Err(ViewError::StringTooLong { .. })
    ));
}

#[test]
fn view_actions_are_bounded() {
    let a = |view: &str, action: &str, args: Json| ViewAction { view: view.into(), action: action.into(), args };
    assert!(validate_action(&a("v", "page", Json(json!({"page": 1})))).is_ok());
    assert!(validate_action(&a("", "page", Json::default())).is_err());
    assert!(validate_action(&a("v", "", Json::default())).is_err());
    assert!(validate_action(&a("v", &"x".repeat(MAX_ID + 1), Json::default())).is_err());
    assert!(validate_action(&a("v", "x", Json(json!("y".repeat(MAX_ARGS))))).is_err());
}

// ---- patches ----

fn two_children() -> ViewDoc {
    ViewDoc {
        root: Node::column(vec![Node::text("a").with_id("a"), Node::column(vec![Node::text("deep").with_id("deep")]).with_id("box")]).with_id("root"),
        etag: 1,
        ..sample()
    }
}

#[test]
fn a_patch_replaces_a_subtree_by_id() {
    let mut d = two_children();
    let patch = ViewPatch::Replace { base_etag: 1, etag: 2, id: "deep".into(), node: Node::text("new").with_id("deep") };
    apply_patch(&mut d, patch).unwrap();
    assert_eq!(d.etag, 2);
    let NodeKind::Stack { children, .. } = &d.root.kind else { unreachable!() };
    let NodeKind::Stack { children: inner, .. } = &children[1].kind else { unreachable!() };
    assert!(matches!(&inner[0].kind, NodeKind::Text { text, .. } if text == "new"));
    // The root can be replaced too.
    apply_patch(&mut d, ViewPatch::Replace { base_etag: 2, etag: 3, id: "root".into(), node: Node::text("only") }).unwrap();
    assert!(matches!(d.root.kind, NodeKind::Text { .. }));
}

#[test]
fn a_patch_that_cannot_apply_leaves_the_document_alone() {
    let before = two_children();
    let try_patch = |p: ViewPatch| {
        let mut d = before.clone();
        let e = apply_patch(&mut d, p).unwrap_err();
        assert_eq!(d, before);
        e
    };
    let replace = |base_etag, etag, id: &str, node| ViewPatch::Replace { base_etag, etag, id: id.into(), node };
    let e = try_patch(replace(9, 10, "a", Node::text("x")));
    assert_eq!(e, PatchError::Stale { have: 1, patch_base: 9 });
    assert!(e.to_string().contains("version 9"));
    let e = try_patch(replace(1, 1, "a", Node::text("x")));
    assert_eq!(e, PatchError::SameEtag { etag: 1 });
    assert!(e.to_string().contains("new one"));
    let e = try_patch(replace(1, 2, "nope", Node::text("x")));
    assert_eq!(e, PatchError::UnknownNode { id: "nope".into() });
    assert!(e.to_string().contains("nope"));
    assert!(matches!(try_patch(replace(1, 2, "", Node::text("x"))), PatchError::UnknownNode { .. }));
    // A patch that would make the document invalid: a duplicate id.
    let e = try_patch(replace(1, 2, "a", Node::text("x").with_id("box")));
    assert!(matches!(e, PatchError::Invalid(ViewError::DuplicateId { .. })));
    assert!(e.to_string().contains("invalid"));
    assert!(std::error::Error::source(&e).is_some());
    assert!(std::error::Error::source(&PatchError::SameEtag { etag: 1 }).is_none());
    let e = try_patch(ViewPatch::Doc(ViewDoc { schema: 9, etag: 5, ..two_children() }));
    assert!(matches!(e, PatchError::Invalid(ViewError::UnsupportedSchema { .. })));
    let e = try_patch(ViewPatch::Doc(two_children()));
    assert_eq!(e, PatchError::SameEtag { etag: 1 });
}

#[test]
fn a_patch_with_a_whole_document_replaces_it() {
    let mut d = two_children();
    let new = ViewDoc { title: "New".into(), etag: 5, ..two_children() };
    apply_patch(&mut d, ViewPatch::Doc(new.clone())).unwrap();
    assert_eq!(d, new);
}

#[test]
fn ids_below_the_depth_limit_are_not_found() {
    let mut d = sample();
    let mut deep = nested(MAX_DEPTH + 3);
    // Give the deepest leaf an id: it is beyond what the validator allows, so it cannot be patched either.
    fn leaf(n: &mut Node) -> &mut Node {
        if matches!(n.kind, NodeKind::Stack { .. }) {
            let NodeKind::Stack { children, .. } = &mut n.kind else { unreachable!() };
            leaf(&mut children[0])
        } else {
            n
        }
    }
    leaf(&mut deep).id = "bottom".into();
    d.root = deep;
    let e = apply_patch(&mut d, ViewPatch::Replace { base_etag: 7, etag: 8, id: "bottom".into(), node: Node::text("x") }).unwrap_err();
    assert_eq!(e, PatchError::UnknownNode { id: "bottom".into() });
}

// ---- codec ----

#[test]
fn decoding_refuses_oversize_trailing_and_broken_input() {
    let big = "x".repeat(MAX_ENCODED + 1);
    assert!(matches!(from_json::<ViewDoc>(&big), Err(ViewError::TooLarge { .. })));
    assert!(matches!(from_postcard::<ViewDoc>(big.as_bytes()), Err(ViewError::TooLarge { .. })));
    assert!(matches!(from_json::<ViewDoc>("{"), Err(ViewError::Decode(_))));
    assert!(matches!(from_postcard::<ViewDoc>(&[]), Err(ViewError::Decode(_))));
    let mut bytes = to_postcard(&sample()).unwrap();
    bytes.push(0);
    let e = from_postcard::<ViewDoc>(&bytes).unwrap_err();
    assert!(matches!(e, ViewError::Decode(ref m) if m.contains("after the value")));
    assert!(e.to_string().contains("does not decode"));
}

#[test]
fn encoding_a_value_serde_cannot_write_is_a_decode_error() {
    struct Bad;
    impl serde::Serialize for Bad {
        fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("no"))
        }
    }
    assert!(matches!(to_json(&Bad), Err(ViewError::Decode(_))));
    assert!(matches!(to_postcard(&Bad), Err(ViewError::Decode(_))));
}

#[test]
fn nested_documents_stop_the_decoder_before_the_stack_does() {
    let ok = ViewDoc { root: nested(DECODE_DEPTH as usize - 2), ..sample() };
    let bytes = to_postcard(&ok).unwrap();
    assert!(from_postcard::<ViewDoc>(&bytes).is_ok());
    // serde_json stops at 128 JSON levels (four per stack), still well above the validator's 16 nodes.
    let json = to_json(&ViewDoc { root: nested(MAX_DEPTH - 1), ..sample() }).unwrap();
    assert!(from_json::<ViewDoc>(&json).is_ok());
    let too = ViewDoc { root: nested(DECODE_DEPTH as usize + 5), ..sample() };
    let e = from_postcard::<ViewDoc>(&to_postcard(&too).unwrap()).unwrap_err();
    // (postcard reports a custom error without its text.)
    assert!(matches!(e, ViewError::Decode(_)), "{e:?}");
    assert!(matches!(from_json::<ViewDoc>(&to_json(&too).unwrap()), Err(ViewError::Decode(_))));
    // The guard is released after a failure: the next decode works.
    assert!(from_postcard::<ViewDoc>(&bytes).is_ok());
    // A document at the decode limit that is hand-made as bytes: postcard of 1 MiB of "one child" stacks.
    let mut hostile = Vec::new();
    for _ in 0..200_000 {
        // id "" (0), kind variant 0 (stack), dir 0, one child (1)
        hostile.extend_from_slice(&[0, 0, 0, 1]);
    }
    let mut doc = to_postcard(&ViewDoc { root: Node::text("x"), ..sample() }).unwrap();
    doc.truncate(1 + 1 + 6); // schema(1 byte varint), title length + "Sample"
    doc.extend(hostile);
    assert!(doc.len() < MAX_ENCODED);
    assert!(matches!(from_postcard::<ViewDoc>(&doc), Err(ViewError::Decode(_))));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn decoding_and_validating_arbitrary_input_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..2048), text in ".{0,512}") {
        if let Ok(doc) = from_postcard::<ViewDoc>(&bytes) {
            let _ = validate(&doc);
        }
        if let Ok(doc) = from_json::<ViewDoc>(&text) {
            let _ = validate(&doc);
        }
        let _ = from_postcard::<ViewPatch>(&bytes);
        let _ = from_postcard::<ViewAction>(&bytes);
        let _ = from_json::<ViewPatch>(&text);
    }

    #[test]
    fn mutated_valid_documents_never_panic_and_valid_ones_round_trip(flip in proptest::collection::vec((0usize..4096, any::<u8>()), 0..8)) {
        let mut bytes = to_postcard(&sample()).unwrap();
        for (at, v) in flip {
            let n = bytes.len();
            bytes[at % n] = v;
        }
        if let Ok(doc) = from_postcard::<ViewDoc>(&bytes) {
            if validate(&doc).is_ok() {
                // NaN never validates, so equality holds.
                prop_assert_eq!(from_postcard::<ViewDoc>(&to_postcard(&doc).unwrap()).unwrap(), doc.clone());
                prop_assert_eq!(from_json::<ViewDoc>(&to_json(&doc).unwrap()).unwrap(), doc);
            }
        }
    }

    #[test]
    fn patching_with_arbitrary_ids_never_panics(id in ".{0,20}", base in 0u64..3, etag in 0u64..3) {
        let mut d = two_children();
        let _ = apply_patch(&mut d, ViewPatch::Replace { base_etag: base, etag, id, node: Node::text("x") });
    }
}

// ---- generated TypeScript ----

const REGENERATE: &str = "PADDOCK_WRITE_TS=1 cargo test -p paddock-view --lib generated";

fn view_ts() -> String {
    use ts_rs::TS;
    let decls = [
        ViewDoc::decl(),
        Node::decl(),
        NodeKind::decl(),
        Dir::decl(),
        Tone::decl(),
        TextStyle::decl(),
        Icon::decl(),
        ActionRef::decl(),
        Link::decl(),
        Table::decl(),
        Column::decl(),
        Sort::decl(),
        Cell::decl(),
        RowSource::decl(),
        Tree::decl(),
        TreeNode::decl(),
        Graph::decl(),
        GNode::decl(),
        GEdge::decl(),
        Layout::decl(),
        Diagram::decl(),
        DiagramKind::decl(),
        DiagramModel::decl(),
        DiagramElement::decl(),
        DiagramRelation::decl(),
        Form::decl(),
        Field::decl(),
        FieldKind::decl(),
        ChoiceOption::decl(),
        GrantRef::decl(),
        SecretRef::decl(),
        ReviewList::decl(),
        Finding::decl(),
        StateDef::decl(),
        Matrix::decl(),
        Label::decl(),
        MatrixCell::decl(),
        ChartSpec::decl(),
        ChartKind::decl(),
        Series::decl(),
        NeedsAi::decl(),
        ViewAction::decl(),
        ViewPatch::decl(),
    ];
    let mut out = format!(
        "// Generated from the Rust wire types of paddock-view (ts-rs). Do not edit: {REGENERATE}.\n\n\
         /** The schema version these types describe (`ViewDoc.schema`). */\n\
         export const VIEW_SCHEMA = {SCHEMA};\n\n\
         /** The limits the engine enforces (`paddock-view::validate`); the UI may rely on them. */\n\
         export const VIEW_LIMITS = {{ maxDepth: {}, maxNodes: {}, maxEncodedBytes: {}, maxStringBytes: {}, maxGraphNodes: {}, maxGraphEdges: {}, maxInlineRows: {} }} as const;\n\n",
        MAX_DEPTH, MAX_NODES, MAX_ENCODED, MAX_STRING, MAX_GRAPH_NODES, MAX_GRAPH_EDGES, MAX_INLINE_ROWS
    );
    for d in decls {
        out.push_str(&format!("export {d}\n\n"));
    }
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

#[test]
fn generated_typescript_is_current() {
    use paddock_commands::testing::{WriteMode, sync_generated};
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/projects/paddock-ui/view/generated/view.ts");
    sync_generated(&path, &view_ts(), WriteMode::from_env()).unwrap_or_else(|e| panic!("{e}"));
}
