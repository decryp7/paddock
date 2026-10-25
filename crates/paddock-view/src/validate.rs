//! The limits validator (ADR 0002, "user input style"): a document that passes can be rendered without surprises; one
//! that fails says which node broke which limit. Never panics; recursion stops at [`MAX_DEPTH`].

use crate::model::*;
use crate::{
    MAX_ARGS, MAX_CHART_POINTS, MAX_DEPTH, MAX_ENCODED, MAX_GRAPH_EDGES, MAX_GRAPH_NODES, MAX_ID,
    MAX_INLINE_ROWS, MAX_NODES, MAX_SAFE_INT, MAX_STRING, MAX_URL, ViewError,
};
use std::collections::HashSet;

/// Checks a document against the limits and rules of ADR 0002.
pub fn validate(doc: &ViewDoc) -> Result<(), ViewError> {
    if doc.schema != SCHEMA {
        return Err(ViewError::UnsupportedSchema {
            found: doc.schema,
            supported: SCHEMA,
        });
    }
    let mut c = Checker::default();
    c.safe_int("etag", doc.etag)?;
    c.label("title", &doc.title)?;
    c.node(&doc.root, "root", 1)?;
    // Last: the cheap checks above bound what is left to count.
    // An encoding that fails is as unusable as one that is too large.
    let bytes = postcard::experimental::serialized_size(doc).unwrap_or(usize::MAX);
    if bytes > MAX_ENCODED {
        return Err(ViewError::TooLarge { bytes });
    }
    Ok(())
}

/// Checks an action the host received from the UI (or is about to send): ids and arguments within limits.
pub fn validate_action(action: &ViewAction) -> Result<(), ViewError> {
    let c = Checker::default();
    c.id("view", &action.view)?;
    c.id("action", &action.action)?;
    c.args("args", &action.args)
}

/// Walks a document once, counting and remembering ids.
#[derive(Default)]
struct Checker {
    nodes: usize,
    ids: HashSet<String>,
}

fn invalid(path: &str, reason: impl Into<String>) -> ViewError {
    ViewError::Invalid {
        path: path.to_string(),
        reason: reason.into(),
    }
}

impl Checker {
    fn count(&mut self, path: &str, n: usize) -> Result<(), ViewError> {
        self.nodes = self.nodes.saturating_add(n);
        if self.nodes > MAX_NODES {
            return Err(ViewError::TooManyNodes {
                path: path.to_string(),
            });
        }
        Ok(())
    }

    fn safe_int(&self, path: &str, v: u64) -> Result<(), ViewError> {
        if v > MAX_SAFE_INT {
            return Err(invalid(
                path,
                "a number above 2^53 - 1 cannot cross JavaScript exactly",
            ));
        }
        Ok(())
    }

    /// A string within the limit (may be empty).
    fn text(&self, path: &str, s: &str) -> Result<(), ViewError> {
        if s.len() > MAX_STRING {
            return Err(ViewError::StringTooLong {
                path: path.to_string(),
                len: s.len(),
            });
        }
        Ok(())
    }

    /// A string within the limit that shows something.
    fn label(&self, path: &str, s: &str) -> Result<(), ViewError> {
        self.text(path, s)?;
        if s.trim().is_empty() {
            return Err(ViewError::Unlabelled {
                path: path.to_string(),
            });
        }
        Ok(())
    }

    /// A non-empty identifier of at most [`MAX_ID`] bytes.
    fn id(&self, path: &str, s: &str) -> Result<(), ViewError> {
        if s.is_empty() || s.len() > MAX_ID {
            return Err(invalid(path, format!("an id is 1 to {MAX_ID} bytes")));
        }
        Ok(())
    }

    /// A key unique in `seen`.
    fn unique(path: &str, seen: &mut HashSet<String>, id: &str) -> Result<(), ViewError> {
        if !seen.insert(id.to_string()) {
            return Err(ViewError::DuplicateId {
                path: path.to_string(),
                id: id.to_string(),
            });
        }
        Ok(())
    }

    fn args(&self, path: &str, args: &paddock_ipc::Json) -> Result<(), ViewError> {
        let bytes = serde_json::to_vec(&args.0).map_or(usize::MAX, |b| b.len());
        if bytes > MAX_ARGS {
            return Err(invalid(
                path,
                format!("arguments larger than {MAX_ARGS} bytes"),
            ));
        }
        Ok(())
    }

    fn action(&mut self, path: &str, a: &ActionRef) -> Result<(), ViewError> {
        self.id(&format!("{path}.id"), &a.id)?;
        self.label(&format!("{path}.label"), &a.label)?;
        if let Some(q) = &a.confirm {
            self.label(&format!("{path}.confirm"), q)?;
        }
        self.args(&format!("{path}.args"), &a.args)
    }

    fn opt_action(&mut self, path: &str, a: &Option<ActionRef>) -> Result<(), ViewError> {
        a.as_ref().map_or(Ok(()), |a| self.action(path, a))
    }

    fn link(&mut self, path: &str, l: &Link) -> Result<(), ViewError> {
        match l {
            Link::Action(a) => self.action(path, a),
            Link::OpenInBrowser { url, label } => {
                self.label(&format!("{path}.label"), label)?;
                web_url(&format!("{path}.url"), url)
            }
        }
    }

    fn opt_link(&mut self, path: &str, l: &Option<Link>) -> Result<(), ViewError> {
        l.as_ref().map_or(Ok(()), |l| self.link(path, l))
    }

    fn node(&mut self, node: &Node, path: &str, depth: usize) -> Result<(), ViewError> {
        if depth > MAX_DEPTH {
            return Err(ViewError::TooDeep {
                path: path.to_string(),
            });
        }
        self.count(path, 1)?;
        if !node.id.is_empty() {
            self.id(&format!("{path}.id"), &node.id)?;
            if !self.ids.insert(node.id.clone()) {
                return Err(ViewError::DuplicateId {
                    path: path.to_string(),
                    id: node.id.clone(),
                });
            }
        }
        match &node.kind {
            NodeKind::Stack { children, .. } => {
                for (i, child) in children.iter().enumerate() {
                    self.node(child, &format!("{path}.children[{i}]"), depth + 1)?;
                }
                Ok(())
            }
            NodeKind::Text { text, .. } => self.text(&format!("{path}.text"), text),
            NodeKind::Table(t) => self.table(t, path),
            NodeKind::Tree(t) => self.tree(t, path, depth),
            NodeKind::Graph(g) => self.graph(g, path),
            NodeKind::Diagram(d) => self.diagram(d, path),
            NodeKind::Form(f) => self.form(f, path),
            NodeKind::ReviewList(r) => self.review(r, path),
            NodeKind::Matrix(m) => self.matrix(m, path),
            NodeKind::Chart(c) => self.chart(c, path),
            NodeKind::Badge { text, .. } => self.label(&format!("{path}.text"), text),
            NodeKind::Progress { value, label } => {
                if !(0.0..=1.0).contains(value) {
                    return Err(invalid(
                        &format!("{path}.value"),
                        "progress is a number from 0 to 1",
                    ));
                }
                self.text(&format!("{path}.label"), label)
            }
            NodeKind::Empty {
                title,
                hint,
                action,
                ..
            } => {
                self.label(&format!("{path}.title"), title)?;
                self.text(&format!("{path}.hint"), hint)?;
                self.opt_action(&format!("{path}.action"), action)
            }
            NodeKind::NeedsAi(n) => {
                self.label(&format!("{path}.feature"), &n.feature)?;
                self.text(&format!("{path}.reason"), &n.reason)?;
                self.action(&format!("{path}.setup"), &n.setup)
            }
            NodeKind::Link { label, target } => {
                self.label(&format!("{path}.label"), label)?;
                self.link(&format!("{path}.target"), target)
            }
        }
    }

    fn cells(&mut self, path: &str, rows: &[Vec<Cell>], columns: usize) -> Result<(), ViewError> {
        self.count(path, rows.len())?;
        for (r, row) in rows.iter().enumerate() {
            if row.len() != columns {
                return Err(invalid(
                    &format!("{path}[{r}]"),
                    format!("{} cells for {columns} columns", row.len()),
                ));
            }
            for (c, cell) in row.iter().enumerate() {
                let p = format!("{path}[{r}][{c}]");
                self.text(&p, &cell.text)?;
                self.opt_link(&format!("{p}.link"), &cell.link)?;
            }
        }
        Ok(())
    }

    fn table(&mut self, t: &Table, path: &str) -> Result<(), ViewError> {
        let mut keys = HashSet::new();
        for (i, col) in t.columns.iter().enumerate() {
            let p = format!("{path}.columns[{i}]");
            self.id(&format!("{p}.key"), &col.key)?;
            self.label(&format!("{p}.label"), &col.label)?;
            Self::unique(&p, &mut keys, &col.key)?;
        }
        if let Some(s) = &t.sort
            && !keys.contains(&s.column)
        {
            return Err(invalid(
                &format!("{path}.sort"),
                format!("no column \"{}\"", s.column),
            ));
        }
        self.opt_action(&format!("{path}.rowAction"), &t.row_action)?;
        let p = format!("{path}.rows");
        match &t.rows {
            RowSource::Inline(rows) => {
                if rows.len() > MAX_INLINE_ROWS {
                    return Err(ViewError::TooManyRows {
                        path: p,
                        rows: rows.len(),
                    });
                }
                self.cells(&p, rows, t.columns.len())
            }
            RowSource::Paged {
                total,
                page_size,
                page,
                rows,
            } => {
                self.safe_int(&format!("{p}.total"), *total)?;
                if *page_size == 0 || *page_size as usize > MAX_INLINE_ROWS {
                    return Err(invalid(
                        &p,
                        format!("a page is 1 to {MAX_INLINE_ROWS} rows"),
                    ));
                }
                if rows.len() > *page_size as usize {
                    return Err(ViewError::TooManyRows {
                        path: p,
                        rows: rows.len(),
                    });
                }
                let first = u64::from(*page) * u64::from(*page_size);
                if first > 0 && first >= *total {
                    return Err(invalid(&p, "the page is beyond the last row"));
                }
                if rows.len() as u64 > total.saturating_sub(first) {
                    return Err(invalid(&p, "more rows on the page than the total allows"));
                }
                self.cells(&p, rows, t.columns.len())
            }
        }
    }

    fn tree(&mut self, t: &Tree, path: &str, depth: usize) -> Result<(), ViewError> {
        self.count(path, t.nodes.len())?;
        let mut seen = HashSet::new();
        let mut prev: Option<u16> = None;
        for (i, n) in t.nodes.iter().enumerate() {
            let p = format!("{path}.nodes[{i}]");
            self.id(&format!("{p}.id"), &n.id)?;
            Self::unique(&p, &mut seen, &n.id)?;
            self.label(&format!("{p}.label"), &n.label)?;
            let allowed = prev.map_or(0, |d| d.saturating_add(1));
            if n.depth > allowed {
                return Err(invalid(
                    &p,
                    "a tree node is at most one level below the node before it",
                ));
            }
            if depth + usize::from(n.depth) + 1 > MAX_DEPTH {
                return Err(ViewError::TooDeep { path: p });
            }
            if n.expandable && !t.lazy {
                return Err(invalid(&p, "only a lazy tree has expandable nodes"));
            }
            self.opt_action(&format!("{p}.action"), &n.action)?;
            prev = Some(n.depth);
        }
        Ok(())
    }

    /// Nodes and edges of a graph or diagram: counts, then the ids that edges name.
    fn graph_shape(path: &str, nodes: usize, edges: usize) -> Result<(), ViewError> {
        if nodes > MAX_GRAPH_NODES || edges > MAX_GRAPH_EDGES {
            return Err(ViewError::GraphTooLarge {
                path: path.to_string(),
                nodes,
                edges,
            });
        }
        Ok(())
    }

    fn edge(
        path: &str,
        ids: &HashSet<&str>,
        from: &str,
        to: &str,
        label: &str,
        c: &Checker,
    ) -> Result<(), ViewError> {
        c.text(&format!("{path}.label"), label)?;
        for (end, id) in [("from", from), ("to", to)] {
            if !ids.contains(id) {
                return Err(invalid(
                    &format!("{path}.{end}"),
                    format!("no node \"{id}\""),
                ));
            }
        }
        Ok(())
    }

    fn graph(&mut self, g: &Graph, path: &str) -> Result<(), ViewError> {
        Self::graph_shape(path, g.nodes.len(), g.edges.len())?;
        self.count(path, g.nodes.len())?;
        let mut seen = HashSet::new();
        for (i, n) in g.nodes.iter().enumerate() {
            let p = format!("{path}.nodes[{i}]");
            self.id(&format!("{p}.id"), &n.id)?;
            Self::unique(&p, &mut seen, &n.id)?;
            self.label(&format!("{p}.label"), &n.label)?;
            self.opt_action(&format!("{p}.action"), &n.action)?;
        }
        let ids: HashSet<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        for (i, e) in g.edges.iter().enumerate() {
            Self::edge(
                &format!("{path}.edges[{i}]"),
                &ids,
                &e.from,
                &e.to,
                &e.label,
                self,
            )?;
        }
        Ok(())
    }

    fn diagram(&mut self, d: &Diagram, path: &str) -> Result<(), ViewError> {
        let m = &d.model;
        Self::graph_shape(path, m.elements.len(), m.relations.len())?;
        self.count(path, m.elements.len())?;
        let mut seen = HashSet::new();
        for (i, e) in m.elements.iter().enumerate() {
            let p = format!("{path}.elements[{i}]");
            self.id(&format!("{p}.id"), &e.id)?;
            Self::unique(&p, &mut seen, &e.id)?;
            self.label(&format!("{p}.label"), &e.label)?;
            self.text(&format!("{p}.detail"), &e.detail)?;
            self.text(&format!("{p}.group"), &e.group)?;
        }
        let ids: HashSet<&str> = m.elements.iter().map(|e| e.id.as_str()).collect();
        for (i, r) in m.relations.iter().enumerate() {
            Self::edge(
                &format!("{path}.relations[{i}]"),
                &ids,
                &r.from,
                &r.to,
                &r.label,
                self,
            )?;
        }
        Ok(())
    }

    fn form(&mut self, f: &Form, path: &str) -> Result<(), ViewError> {
        self.count(path, f.fields.len())?;
        let mut seen = HashSet::new();
        for (i, field) in f.fields.iter().enumerate() {
            let p = format!("{path}.fields[{i}]");
            self.id(&format!("{p}.id"), &field.id)?;
            Self::unique(&p, &mut seen, &field.id)?;
            self.label(&format!("{p}.label"), &field.label)?;
            self.text(&format!("{p}.help"), &field.help)?;
            self.field_kind(&field.kind, &format!("{p}.kind"))?;
        }
        self.action(&format!("{path}.submit"), &f.submit)
    }

    fn field_kind(&self, kind: &FieldKind, p: &str) -> Result<(), ViewError> {
        match kind {
            FieldKind::Text {
                value, placeholder, ..
            } => {
                self.text(&format!("{p}.value"), value)?;
                self.text(&format!("{p}.placeholder"), placeholder)
            }
            FieldKind::Number { value, min, max } => {
                let all = [Some(*value), *min, *max];
                if all.iter().flatten().any(|n| !n.is_finite()) {
                    return Err(invalid(p, "numbers are finite"));
                }
                if min.zip(*max).is_some_and(|(lo, hi)| lo > hi)
                    || min.is_some_and(|lo| *value < lo)
                    || max.is_some_and(|hi| *value > hi)
                {
                    return Err(invalid(p, "the value is outside min and max"));
                }
                Ok(())
            }
            FieldKind::Choice { value, options } => {
                let mut seen = HashSet::new();
                for (i, o) in options.iter().enumerate() {
                    let op = format!("{p}.options[{i}]");
                    self.text(&format!("{op}.value"), &o.value)?;
                    self.label(&format!("{op}.label"), &o.label)?;
                    Self::unique(&op, &mut seen, &o.value)?;
                }
                if !value.is_empty() && !seen.contains(value) {
                    return Err(invalid(&format!("{p}.value"), "not one of the options"));
                }
                Ok(())
            }
            FieldKind::Toggle { .. } => Ok(()),
            // A `GrantRef` is valid by construction (see `GrantRef::new`).
            FieldKind::Path { .. } => Ok(()),
            FieldKind::SecretRef { value } => value
                .as_ref()
                .map_or(Ok(()), |s| self.id(&format!("{p}.value"), &s.0)),
        }
    }

    fn review(&mut self, r: &ReviewList, path: &str) -> Result<(), ViewError> {
        self.count(path, r.items.len())?;
        let mut states = HashSet::new();
        for (i, s) in r.states.iter().enumerate() {
            let p = format!("{path}.states[{i}]");
            self.id(&format!("{p}.id"), &s.id)?;
            Self::unique(&p, &mut states, &s.id)?;
            self.label(&format!("{p}.label"), &s.label)?;
            for (a, action) in s.actions.iter().enumerate() {
                self.action(&format!("{p}.actions[{a}]"), action)?;
            }
        }
        let mut seen = HashSet::new();
        for (i, f) in r.items.iter().enumerate() {
            let p = format!("{path}.items[{i}]");
            self.id(&format!("{p}.id"), &f.id)?;
            Self::unique(&p, &mut seen, &f.id)?;
            self.label(&format!("{p}.title"), &f.title)?;
            self.text(&format!("{p}.detail"), &f.detail)?;
            self.text(&format!("{p}.location"), &f.location)?;
            if !states.contains(&f.state) {
                return Err(invalid(
                    &format!("{p}.state"),
                    format!("no state \"{}\"", f.state),
                ));
            }
            self.opt_link(&format!("{p}.link"), &f.link)?;
        }
        Ok(())
    }

    fn matrix(&mut self, m: &Matrix, path: &str) -> Result<(), ViewError> {
        self.count(path, m.cells.len())?;
        for (name, labels) in [("rows", &m.rows), ("cols", &m.cols)] {
            let mut seen = HashSet::new();
            for (i, l) in labels.iter().enumerate() {
                let p = format!("{path}.{name}[{i}]");
                self.id(&format!("{p}.id"), &l.id)?;
                Self::unique(&p, &mut seen, &l.id)?;
                self.label(&format!("{p}.text"), &l.text)?;
            }
        }
        let mut at = HashSet::new();
        for (i, c) in m.cells.iter().enumerate() {
            let p = format!("{path}.cells[{i}]");
            if c.row as usize >= m.rows.len() || c.col as usize >= m.cols.len() {
                return Err(invalid(&p, "the cell is outside the matrix"));
            }
            if !at.insert((c.row, c.col)) {
                return Err(invalid(&p, "two cells at the same place"));
            }
            self.text(&format!("{p}.text"), &c.text)?;
            self.opt_action(&format!("{p}.action"), &c.action)?;
        }
        Ok(())
    }

    fn chart(&mut self, c: &ChartSpec, path: &str) -> Result<(), ViewError> {
        self.text(&format!("{path}.title"), &c.title)?;
        for (i, cat) in c.categories.iter().enumerate() {
            self.text(&format!("{path}.categories[{i}]"), cat)?;
        }
        let points = c.series.iter().map(|s| s.values.len()).sum::<usize>();
        if points > MAX_CHART_POINTS {
            return Err(invalid(
                path,
                format!("{points} values; the limit is {MAX_CHART_POINTS}"),
            ));
        }
        self.count(path, c.series.len())?;
        for (i, s) in c.series.iter().enumerate() {
            let p = format!("{path}.series[{i}]");
            self.label(&format!("{p}.name"), &s.name)?;
            if s.values.len() != c.categories.len() {
                return Err(invalid(
                    &p,
                    format!(
                        "{} values for {} categories",
                        s.values.len(),
                        c.categories.len()
                    ),
                ));
            }
            if s.values.iter().any(|v| !v.is_finite()) {
                return Err(invalid(&p, "values are finite numbers"));
            }
        }
        Ok(())
    }
}

/// An `http` or `https` URL with a host, no user information, no spaces or control characters.
fn web_url(path: &str, url: &str) -> Result<(), ViewError> {
    let bad = || ViewError::BadUrl {
        path: path.to_string(),
    };
    if url.len() > MAX_URL {
        return Err(ViewError::StringTooLong {
            path: path.to_string(),
            len: url.len(),
        });
    }
    let lower = url.get(..8).unwrap_or(url).to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &url[8..]
    } else if lower.starts_with("http://") {
        &url[7..]
    } else {
        return Err(bad());
    };
    if url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(bad());
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') || authority.starts_with(':') {
        return Err(bad());
    }
    Ok(())
}
