use crate::analysis;
use crate::contributor::EvidenceContributor;
use crate::llvm;
use crate::model::{Document, Graph};
use crate::snapshot::{
    Explanation, ExplanationHandle, NamedQueryResult, ObservationContext, ProgramEntityId,
    PublishedSnapshot,
};
use crate::viewer;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct Application;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Query {
    Summary,
    PotentialRecursiveCycles,
    Reachable { start: String },
    ShortestPath { start: String, end: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NamedQuery {
    Callees {
        caller_name: String,
        caller_entity_id: Option<ProgramEntityId>,
    },
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum QueryResult {
    Summary(crate::model::AnalysisSummary),
    PotentialRecursiveCycles(Vec<Vec<String>>),
    Reachable(Vec<String>),
    ShortestPath(Option<Vec<String>>),
}

impl Application {
    pub fn publish_snapshot<C: EvidenceContributor + ?Sized>(
        &self,
        inputs: &[PathBuf],
        context: ObservationContext,
        contributor: &C,
    ) -> Result<PublishedSnapshot, String> {
        let identity = contributor.identity();
        let contributions = inputs
            .iter()
            .map(|input| contributor.contribute(input, &context))
            .collect::<Result<Vec<_>, _>>()?;
        crate::snapshot::publish(contributions, identity, context)
    }

    pub fn query_snapshot(
        &self,
        snapshot: &PublishedSnapshot,
        query: NamedQuery,
    ) -> Result<NamedQueryResult, String> {
        match query {
            NamedQuery::Callees {
                caller_name,
                caller_entity_id,
            } => snapshot.query_callees(&caller_name, caller_entity_id.as_ref()),
        }
    }

    pub fn explain_snapshot(
        &self,
        snapshot: &PublishedSnapshot,
        handle: &ExplanationHandle,
    ) -> Result<Explanation, String> {
        snapshot.explain(handle)
    }

    pub fn export_snapshot_json(&self, snapshot: &PublishedSnapshot) -> Result<String, String> {
        let mut text = serde_json::to_string_pretty(snapshot).map_err(|error| error.to_string())?;
        text.push('\n');
        Ok(text)
    }

    /// Reads a published snapshot back from an export.
    ///
    /// Deserializing a `PublishedSnapshot` validates the document, so loading
    /// is exactly that: a coherent export becomes a snapshot and an incoherent
    /// one becomes the error `validate` reports for it.
    pub fn load_snapshot_json(&self, text: &str) -> Result<PublishedSnapshot, String> {
        serde_json::from_str(text).map_err(|error| error.to_string())
    }

    pub fn render_snapshot_viewer(&self, snapshot: &PublishedSnapshot) -> Result<String, String> {
        viewer::render_snapshot_html(snapshot)
    }

    pub fn build(
        &self,
        inputs: &[PathBuf],
        clang: &str,
        clang_flags: &[String],
    ) -> Result<Document, String> {
        let mut graph = Graph::default();
        for input in inputs {
            graph.merge(llvm::graph_from_path(input, clang, clang_flags)?);
        }
        Ok(Document::from_graph(&graph, analysis::summary(&graph)))
    }

    pub fn export_json(&self, document: &Document) -> Result<String, String> {
        let mut text = serde_json::to_string_pretty(document).map_err(|error| error.to_string())?;
        text.push('\n');
        Ok(text)
    }

    pub fn load_json(&self, text: &str) -> Result<Document, String> {
        let document: Document = serde_json::from_str(text).map_err(|error| error.to_string())?;
        if document.schema_version != "1.0" {
            return Err(format!(
                "unsupported graph schema {:?}",
                document.schema_version
            ));
        }
        Ok(document)
    }

    pub fn query(&self, document: Document, query: Query) -> Result<QueryResult, String> {
        let graph = document.into_graph();
        Ok(match query {
            Query::Summary => QueryResult::Summary(analysis::summary(&graph)),
            Query::PotentialRecursiveCycles => {
                QueryResult::PotentialRecursiveCycles(analysis::cycles(&graph))
            }
            Query::Reachable { start } => {
                QueryResult::Reachable(analysis::reachable(&graph, &start)?)
            }
            Query::ShortestPath { start, end } => {
                QueryResult::ShortestPath(analysis::shortest_path(&graph, &start, &end)?)
            }
        })
    }

    pub fn render_viewer(&self, document: &Document) -> Result<String, String> {
        viewer::render_html(document)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_llvm_ir_through_the_application_seam() {
        let document = Application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();

        assert_eq!(document.schema_version, "1.0");
        assert_eq!(document.metadata.inputs, ["tests/fixtures/simple.ll"]);
        assert_eq!(document.analysis.as_ref().unwrap().node_count, 3);
    }

    #[test]
    fn exports_and_loads_schema_1_0_through_the_application_seam() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();

        let json = application.export_json(&document).unwrap();
        assert!(json.ends_with('\n'));
        let loaded = application.load_json(&json).unwrap();
        assert_eq!(loaded.schema_version, "1.0");
        assert_eq!(loaded.nodes.len(), 3);
        assert_eq!(loaded.edges.len(), 3);
    }

    #[test]
    fn runs_the_semantic_query_suite_through_the_application_seam() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();

        assert_eq!(
            serde_json::to_value(
                application
                    .query(document.clone(), Query::PotentialRecursiveCycles)
                    .unwrap()
            )
            .unwrap(),
            serde_json::json!([["step"]])
        );
        assert_eq!(
            serde_json::to_value(
                application
                    .query(
                        document.clone(),
                        Query::Reachable {
                            start: "main".into(),
                        },
                    )
                    .unwrap()
            )
            .unwrap(),
            serde_json::json!(["puts", "step"])
        );
        assert_eq!(
            serde_json::to_value(
                application
                    .query(
                        document.clone(),
                        Query::ShortestPath {
                            start: "main".into(),
                            end: "puts".into(),
                        },
                    )
                    .unwrap()
            )
            .unwrap(),
            serde_json::json!(["main", "step", "puts"])
        );
        assert_eq!(
            serde_json::to_value(application.query(document, Query::Summary).unwrap()).unwrap()["node_count"],
            3
        );
    }

    #[test]
    fn renders_a_self_contained_viewer_through_the_application_seam() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();

        let html = application.render_viewer(&document).unwrap();
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("\"schema_version\":\"1.0\""));
        assert!(!html.contains("__GRAPH_DATA__"));
    }
}
