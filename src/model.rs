use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub label: String,
    #[serde(default = "function_kind")]
    pub kind: String,
    #[serde(default)]
    pub defined: bool,
    #[serde(default = "unknown_language")]
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

fn function_kind() -> String {
    "function".into()
}
fn unknown_language() -> String {
    "unknown".into()
}

impl Node {
    /// A callable node.
    ///
    /// The identity and the label are separate arguments because a display
    /// name is a label: two translation-unit-local callables may share one
    /// while remaining different callables, so only the identity may key the
    /// graph.
    pub fn callable(
        id: impl Into<String>,
        label: impl Into<String>,
        defined: bool,
        source: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: function_kind(),
            defined,
            language: "llvm".into(),
            source,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub source: String,
    pub target: String,
    #[serde(default = "direct_call")]
    pub kind: String,
    #[serde(default = "one")]
    pub call_count: u64,
}

fn direct_call() -> String {
    "direct-call".into()
}
fn one() -> u64 {
    1
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: BTreeMap<String, Node>,
    pub edges: BTreeMap<(String, String, String), Edge>,
    pub inputs: Vec<String>,
}

impl Graph {
    pub fn add_node(&mut self, node: Node) {
        match self.nodes.get(&node.id) {
            Some(previous) if previous.defined || !node.defined => {}
            _ => {
                self.nodes.insert(node.id.clone(), node);
            }
        }
    }

    pub fn add_edge(&mut self, source: &str, target: &str, kind: &str) {
        let key = (source.to_owned(), target.to_owned(), kind.to_owned());
        self.edges
            .entry(key)
            .and_modify(|edge| edge.call_count += 1)
            .or_insert_with(|| Edge {
                source: source.into(),
                target: target.into(),
                kind: kind.into(),
                call_count: 1,
            });
    }

    pub fn merge(&mut self, other: Graph) {
        for node in other.nodes.into_values() {
            self.add_node(node);
        }
        for edge in other.edges.into_values() {
            let key = (edge.source.clone(), edge.target.clone(), edge.kind.clone());
            self.edges
                .entry(key)
                .and_modify(|old| old.call_count += edge.call_count)
                .or_insert(edge);
        }
        for input in other.inputs {
            if !self.inputs.contains(&input) {
                self.inputs.push(input);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Metadata {
    pub generated_at_unix_ms: u128,
    #[serde(default)]
    pub inputs: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AnalysisSummary {
    pub node_count: usize,
    pub edge_count: usize,
    pub defined_function_count: usize,
    pub entry_points: Vec<String>,
    pub cycles: Vec<Vec<String>>,
    pub cycle_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: String,
    pub metadata: Metadata,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub analysis: Option<AnalysisSummary>,
}

impl Document {
    pub fn from_graph(graph: &Graph, analysis: AnalysisSummary) -> Self {
        let generated_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Self {
            schema_version: "1.0".into(),
            metadata: Metadata {
                generated_at_unix_ms,
                inputs: graph.inputs.clone(),
            },
            nodes: graph.nodes.values().cloned().collect(),
            edges: graph.edges.values().cloned().collect(),
            analysis: Some(analysis),
        }
    }

    pub fn into_graph(self) -> Graph {
        let mut graph = Graph {
            inputs: self.metadata.inputs,
            ..Graph::default()
        };
        for node in self.nodes {
            graph.add_node(node);
        }
        for edge in self.edges {
            graph.edges.insert(
                (edge.source.clone(), edge.target.clone(), edge.kind.clone()),
                edge,
            );
        }
        graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis;

    #[test]
    fn definition_wins_and_document_round_trips() {
        let mut graph = Graph::default();
        graph.add_node(Node::callable("f", "f", false, None));
        graph.add_node(Node::callable("f", "f", true, Some("f.c".into())));
        assert!(graph.nodes["f"].defined);

        let document = Document::from_graph(&graph, analysis::summary(&graph));
        let encoded = serde_json::to_string(&document).unwrap();
        let decoded: Document = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.into_graph().nodes, graph.nodes);
    }
}
