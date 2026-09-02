use crate::model::Document;
use crate::snapshot::{Explanation, ProgramSnapshot, ProjectedCallSite, PublishedSnapshot};
use serde::Serialize;

pub fn render_html(document: &Document) -> Result<String, String> {
    let data = serde_json::to_string(document)
        .map_err(|e| e.to_string())?
        .replace("</", "<\\/");
    Ok(include_str!("../assets/viewer.html").replace("__GRAPH_DATA__", &data))
}

#[derive(Serialize)]
struct SnapshotViewerCallSite<'a> {
    call_site: &'a ProjectedCallSite,
    target_set_incomplete: bool,
    explanation: Explanation,
}

#[derive(Serialize)]
struct SnapshotViewerData<'a> {
    program_snapshot: &'a ProgramSnapshot,
    call_sites: Vec<SnapshotViewerCallSite<'a>>,
}

pub fn render_snapshot_html(snapshot: &PublishedSnapshot) -> Result<String, String> {
    let call_sites = snapshot
        .call_graph_projection()
        .call_sites
        .iter()
        .map(|call_site| {
            Ok(SnapshotViewerCallSite {
                call_site,
                target_set_incomplete: call_site.resolution.target_set_incomplete(),
                explanation: snapshot.explain(&call_site.explanation_handle)?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let data = serde_json::to_string(&SnapshotViewerData {
        program_snapshot: snapshot.program_snapshot(),
        call_sites,
    })
    .map_err(|error| error.to_string())?
    .replace("</", "<\\/");
    Ok(include_str!("../assets/snapshot-viewer.html").replace("__SNAPSHOT_DATA__", &data))
}
