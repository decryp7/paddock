use std::collections::{BTreeMap, BTreeSet};

/// A project: a Cargo crate or an npm package (or a Tauri shell).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The package name.
    pub name: String,
    /// The Cargo workspace folder the crate belongs to (empty for npm).
    pub workspace: String,
    /// The folders of the projects this one depends on.
    pub deps: Vec<String>,
}

/// Projects by folder (repository-relative, `/`-separated) and the edges between them.
#[derive(Debug, Default, Clone)]
pub struct Graph {
    nodes: BTreeMap<String, Node>,
}

impl Graph {
    /// An empty graph.
    pub fn new() -> Graph {
        Graph::default()
    }

    /// Adds or replaces the project in `dir`.
    pub fn insert(&mut self, dir: &str, node: Node) {
        self.nodes.insert(dir.to_string(), node);
    }

    /// The project in `dir`.
    pub fn get(&self, dir: &str) -> Option<&Node> {
        self.nodes.get(dir)
    }

    /// Whether `dir` is a project.
    pub fn contains(&self, dir: &str) -> bool {
        self.nodes.contains_key(dir)
    }

    /// Every project, sorted by folder.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Node)> {
        self.nodes.iter().map(|(d, n)| (d.as_str(), n))
    }

    /// Adds the edge `from` to `to` unless `to` is empty, is `from` itself, `from` is not a project or the edge exists.
    pub fn add_edge(&mut self, from: &str, to: Option<&str>) {
        let (Some(to), Some(node)) = (to, self.nodes.get_mut(from)) else {
            return;
        };
        if to != from && !node.deps.iter().any(|d| d == to) {
            node.deps.push(to.to_string());
        }
    }

    /// The folder of the deepest project that contains `path` (the folder itself or a file below it).
    pub fn node_of(&self, path: &str) -> Option<&str> {
        self.nodes
            .keys()
            .filter(|d| {
                path == d.as_str()
                    || path
                        .strip_prefix(d.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
            })
            .max_by_key(|d| d.len())
            .map(String::as_str)
    }

    /// The reverse closure: every project that reaches one of `start` through the edges, `start` included.
    pub fn dependents(&self, start: &BTreeSet<String>) -> BTreeSet<String> {
        let mut rev: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for (d, n) in &self.nodes {
            for t in &n.deps {
                rev.entry(t.as_str()).or_default().push(d.as_str());
            }
        }
        let mut seen = start.clone();
        let mut todo: Vec<String> = start.iter().cloned().collect();
        while let Some(at) = todo.pop() {
            for r in rev.get(at.as_str()).into_iter().flatten() {
                if seen.insert((*r).to_string()) {
                    todo.push((*r).to_string());
                }
            }
        }
        seen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(name: &str, deps: &[&str]) -> Node {
        Node {
            name: name.into(),
            workspace: String::new(),
            deps: deps.iter().map(|d| d.to_string()).collect(),
        }
    }

    #[test]
    fn deepest_project_wins_and_prefixes_must_end_at_a_slash() {
        let mut g = Graph::new();
        g.insert("ui", node("ui", &[]));
        g.insert("ui/projects/x", node("x", &[]));
        assert_eq!(g.node_of("ui/projects/x/src/a.ts"), Some("ui/projects/x"));
        assert_eq!(g.node_of("ui/other.ts"), Some("ui"));
        assert_eq!(g.node_of("ui"), Some("ui"));
        assert_eq!(g.node_of("uix/a"), None);
        assert!(g.contains("ui") && !g.contains("uix"));
        assert_eq!(g.get("ui").map(|n| n.name.as_str()), Some("ui"));
    }

    #[test]
    fn edges_skip_self_duplicates_and_unknown_sources() {
        let mut g = Graph::new();
        g.insert("a", node("a", &[]));
        g.insert("b", node("b", &[]));
        g.add_edge("a", Some("b"));
        g.add_edge("a", Some("b"));
        g.add_edge("a", Some("a"));
        g.add_edge("a", None);
        g.add_edge("zzz", Some("a"));
        assert_eq!(g.get("a").unwrap().deps, ["b"]);
        assert_eq!(g.iter().count(), 2);
    }

    #[test]
    fn dependents_follow_edges_backwards_through_chains_and_cycles() {
        let mut g = Graph::new();
        g.insert("a", node("a", &["b"]));
        g.insert("b", node("b", &["c"]));
        g.insert("c", node("c", &["a"]));
        g.insert("d", node("d", &[]));
        g.insert("e", node("e", &["d"]));
        let got = g.dependents(&BTreeSet::from(["c".to_string()]));
        assert_eq!(
            got,
            BTreeSet::from(["a".to_string(), "b".to_string(), "c".to_string()])
        );
        let got = g.dependents(&BTreeSet::from(["d".to_string()]));
        assert_eq!(got, BTreeSet::from(["d".to_string(), "e".to_string()]));
        assert!(g.dependents(&BTreeSet::new()).is_empty());
    }
}
