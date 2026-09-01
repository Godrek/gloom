use crate::model::Document;
use crate::snapshot::{CallRelationship, Explanation, ProgramSnapshot, PublishedSnapshot};
use serde::Serialize;

pub fn render_html(document: &Document) -> Result<String, String> {
    let data = serde_json::to_string(document)
        .map_err(|e| e.to_string())?
        .replace("</", "<\\/");
    Ok(include_str!("../assets/viewer.html").replace("__GRAPH_DATA__", &data))
}

#[derive(Serialize)]
struct SnapshotViewerRelationship<'a> {
    relationship: &'a CallRelationship,
    explanation: Explanation,
}

#[derive(Serialize)]
struct SnapshotViewerData<'a> {
    program_snapshot: &'a ProgramSnapshot,
    relationships: Vec<SnapshotViewerRelationship<'a>>,
}

pub fn render_snapshot_html(snapshot: &PublishedSnapshot) -> Result<String, String> {
    let relationships = snapshot
        .call_graph_projection()
        .relationships
        .iter()
        .map(|relationship| {
            Ok(SnapshotViewerRelationship {
                relationship,
                explanation: snapshot.explain(&relationship.explanation_handle)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let data = serde_json::to_string(&SnapshotViewerData {
        program_snapshot: snapshot.program_snapshot(),
        relationships,
    })
    .map_err(|error| error.to_string())?
    .replace("</", "<\\/");
    Ok(include_str!("../assets/snapshot-viewer.html").replace("__SNAPSHOT_DATA__", &data))
}
