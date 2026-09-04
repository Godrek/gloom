use crate::model::{AnalysisSummary, Graph};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub fn adjacency(graph: &Graph) -> BTreeMap<String, BTreeSet<String>> {
    let mut links: BTreeMap<_, BTreeSet<_>> = graph
        .nodes
        .keys()
        .map(|n| (n.clone(), BTreeSet::new()))
        .collect();
    for edge in graph.edges.values() {
        links
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.target.clone());
    }
    links
}

struct Tarjan<'a> {
    links: &'a BTreeMap<String, BTreeSet<String>>,
    next: usize,
    indices: BTreeMap<String, usize>,
    low: BTreeMap<String, usize>,
    stack: Vec<String>,
    on_stack: BTreeSet<String>,
    components: Vec<Vec<String>>,
}

impl Tarjan<'_> {
    fn visit(&mut self, node: &str) {
        let current = self.next;
        self.next += 1;
        self.indices.insert(node.into(), current);
        self.low.insert(node.into(), current);
        self.stack.push(node.into());
        self.on_stack.insert(node.into());
        for target in self.links.get(node).into_iter().flatten() {
            if !self.indices.contains_key(target) {
                self.visit(target);
                let value = self.low[node].min(self.low[target]);
                self.low.insert(node.into(), value);
            } else if self.on_stack.contains(target) {
                let value = self.low[node].min(self.indices[target]);
                self.low.insert(node.into(), value);
            }
        }
        if self.low[node] == self.indices[node] {
            let mut component = Vec::new();
            loop {
                let item = self.stack.pop().expect("Tarjan stack cannot be empty");
                self.on_stack.remove(&item);
                component.push(item.clone());
                if item == node {
                    break;
                }
            }
            component.sort();
            self.components.push(component);
        }
    }
}

pub fn strongly_connected_components(graph: &Graph) -> Vec<Vec<String>> {
    let links = adjacency(graph);
    let mut state = Tarjan {
        links: &links,
        next: 0,
        indices: BTreeMap::new(),
        low: BTreeMap::new(),
        stack: vec![],
        on_stack: BTreeSet::new(),
        components: vec![],
    };
    for node in links.keys() {
        if !state.indices.contains_key(node) {
            state.visit(node);
        }
    }
    state.components.sort();
    state.components
}

pub fn cycles(graph: &Graph) -> Vec<Vec<String>> {
    let links = adjacency(graph);
    strongly_connected_components(graph)
        .into_iter()
        .filter(|c| c.len() > 1 || links[&c[0]].contains(&c[0]))
        .collect()
}

pub fn entry_points(graph: &Graph) -> Vec<String> {
    let mut incoming = BTreeSet::new();
    for edge in graph.edges.values() {
        incoming.insert(&edge.target);
    }
    graph
        .nodes
        .values()
        .filter(|n| n.defined && !incoming.contains(&n.id))
        .map(|n| n.id.clone())
        .collect()
}

/// Resolves a selector a user typed to one node identity.
///
/// A display name is a label, so several nodes may carry it: a callable
/// private to its translation unit keeps an identity of its own in every unit
/// that declares one. A selector that names an identity exactly selects that
/// node; otherwise it is matched against labels, and an ambiguous label is
/// reported with the identities to choose from rather than resolved by picking
/// one of them.
pub fn resolve(graph: &Graph, selector: &str) -> Result<String, String> {
    if graph.nodes.contains_key(selector) {
        return Ok(selector.to_owned());
    }
    let matched = graph
        .nodes
        .values()
        .filter(|node| node.label == selector)
        .map(|node| node.id.as_str())
        .collect::<Vec<_>>();
    match matched.as_slice() {
        [] => Err(format!("unknown function '{selector}'")),
        [id] => Ok((*id).to_owned()),
        several => Err(format!(
            "function '{selector}' is ambiguous; select one of: {}",
            several.join(", ")
        )),
    }
}

pub fn reachable(graph: &Graph, start: &str) -> Result<Vec<String>, String> {
    if !graph.nodes.contains_key(start) {
        return Err(format!("unknown function '{start}'"));
    }
    let links = adjacency(graph);
    let mut seen = BTreeSet::from([start.to_owned()]);
    let mut queue = VecDeque::from([start.to_owned()]);
    while let Some(node) = queue.pop_front() {
        for target in &links[&node] {
            if seen.insert(target.clone()) {
                queue.push_back(target.clone());
            }
        }
    }
    seen.remove(start);
    Ok(seen.into_iter().collect())
}

pub fn shortest_path(graph: &Graph, start: &str, end: &str) -> Result<Option<Vec<String>>, String> {
    for name in [start, end] {
        if !graph.nodes.contains_key(name) {
            return Err(format!("unknown function '{name}'"));
        }
    }
    let links = adjacency(graph);
    let mut seen = BTreeSet::from([start.to_owned()]);
    let mut queue = VecDeque::from([(start.to_owned(), vec![start.to_owned()])]);
    while let Some((node, path)) = queue.pop_front() {
        if node == end {
            return Ok(Some(path));
        }
        for target in &links[&node] {
            if seen.insert(target.clone()) {
                let mut next = path.clone();
                next.push(target.clone());
                queue.push_back((target.clone(), next));
            }
        }
    }
    Ok(None)
}

pub fn summary(graph: &Graph) -> AnalysisSummary {
    let cycles = cycles(graph);
    AnalysisSummary {
        node_count: graph.nodes.len(),
        edge_count: graph.edges.len(),
        defined_function_count: graph.nodes.values().filter(|n| n.defined).count(),
        entry_points: entry_points(graph),
        cycle_count: cycles.len(),
        cycles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Node;

    fn sample() -> Graph {
        let mut graph = Graph::default();
        for name in ["main", "a", "b", "leaf"] {
            graph.add_node(Node::callable(name, name, true, None));
        }
        for (source, target) in [("main", "a"), ("a", "b"), ("b", "a"), ("b", "leaf")] {
            graph.add_edge(source, target, "direct-call");
        }
        graph
    }

    #[test]
    fn finds_entries_cycles_reachability_and_paths() {
        let graph = sample();
        assert_eq!(entry_points(&graph), ["main"]);
        assert_eq!(cycles(&graph), [vec!["a".to_owned(), "b".to_owned()]]);
        assert_eq!(reachable(&graph, "main").unwrap(), ["a", "b", "leaf"]);
        assert_eq!(
            shortest_path(&graph, "main", "leaf").unwrap().unwrap(),
            ["main", "a", "b", "leaf"]
        );
        assert_eq!(shortest_path(&graph, "leaf", "main").unwrap(), None);
    }
}
