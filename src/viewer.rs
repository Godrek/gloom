use crate::model::Document;
use crate::snapshot::{
    CallSiteResolution, CorrespondenceClaim, Derivation, EvidenceRecord, ProgramSnapshot,
    ProjectedCallSite, PublishedSnapshot, SourceLocation, TargetClaim,
};
use serde::Serialize;
use std::collections::BTreeMap;

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
    source_location: &'a SourceLocation,
    call_site_resolution: &'a CallSiteResolution,
}

#[derive(Serialize)]
struct SnapshotViewerData<'a> {
    program_snapshot: &'a ProgramSnapshot,
    call_sites: Vec<SnapshotViewerCallSite<'a>>,
    evidence_records: &'a [EvidenceRecord],
    target_claims: &'a [TargetClaim],
    correspondence_claims: &'a [CorrespondenceClaim],
    derivations: &'a [Derivation],
}

pub fn render_snapshot_html(snapshot: &PublishedSnapshot) -> Result<String, String> {
    let source_locations = snapshot
        .program_entities()
        .iter()
        .filter_map(|entity| {
            entity
                .source_location
                .as_ref()
                .map(|location| (entity.id.as_str(), location))
        })
        .collect::<BTreeMap<_, _>>();
    let resolutions = snapshot
        .call_site_resolutions()
        .iter()
        .map(|resolution| (resolution.call_site_id.as_str(), resolution))
        .collect::<BTreeMap<_, _>>();
    let call_sites = snapshot
        .call_graph_projection()
        .call_sites
        .iter()
        .map(|call_site| {
            Ok(SnapshotViewerCallSite {
                call_site,
                target_set_incomplete: call_site.resolution.target_set_incomplete(),
                source_location: source_locations
                    .get(call_site.call_site_id.as_str())
                    .copied()
                    .ok_or_else(|| {
                        format!(
                            "projected call site '{}' has no source location",
                            call_site.call_site_id
                        )
                    })?,
                call_site_resolution: resolutions
                    .get(call_site.call_site_id.as_str())
                    .copied()
                    .ok_or_else(|| {
                        format!(
                            "projected call site '{}' has no resolution",
                            call_site.call_site_id
                        )
                    })?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let data = serde_json::to_string(&SnapshotViewerData {
        program_snapshot: snapshot.program_snapshot(),
        call_sites,
        evidence_records: snapshot.evidence_records(),
        target_claims: snapshot.target_claims(),
        correspondence_claims: snapshot.correspondence_claims(),
        derivations: snapshot.derivations(),
    })
    .map_err(|error| error.to_string())?
    .replace("</", "<\\/");
    Ok(include_str!("../assets/snapshot-viewer.html").replace("__SNAPSHOT_DATA__", &data))
}
