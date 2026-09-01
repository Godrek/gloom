use crate::analysis;
use crate::llvm;
use crate::model::{
    AcquiredInput, CallSite, Document, EvidenceRecord, Graph, Knowledge, Manifestation, Node,
    ObservationContext, ProgramEntity, TargetClaim, acquired_input_id, call_site_entity_id,
    call_site_id, callable_entity_id, evidence_contributor_id, evidence_id, manifestation_id,
    observation_context_id, published_snapshot_id, target_claim_id,
};
use crate::viewer;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct Application;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Query {
    Summary,
    PotentialRecursiveCycles,
    Reachable { start: String },
    ShortestPath { start: String, end: String },
    CallsToNamedCallee { callee: String },
    Explain { handle: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NamedCallRelationship {
    pub caller_name: String,
    pub callee_name: String,
    pub explanation_handle: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Explanation {
    pub snapshot_id: String,
    pub claim: TargetClaim,
    pub evidence: EvidenceRecord,
    pub observation_context: ObservationContext,
    pub derivation: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct NamedCallView {
    pub legacy_caller_id: String,
    pub relationship: NamedCallRelationship,
    pub explanation: Explanation,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum QueryResult {
    Summary(crate::model::AnalysisSummary),
    PotentialRecursiveCycles(Vec<Vec<String>>),
    Reachable(Vec<String>),
    ShortestPath(Option<Vec<String>>),
    CallsToNamedCallee(Vec<NamedCallRelationship>),
    Explain(Box<Explanation>),
}

fn unique_index<'a, T>(
    records: &'a [T],
    id: impl Fn(&'a T) -> &'a str,
) -> Result<HashMap<&'a str, &'a T>, String> {
    let mut index = HashMap::with_capacity(records.len());
    for record in records {
        let record_id = id(record);
        if index.insert(record_id, record).is_some() {
            return Err(format!("duplicate document ID '{record_id}'"));
        }
    }
    Ok(index)
}

struct KnowledgeIndex<'a> {
    entities: HashMap<&'a str, &'a ProgramEntity>,
    claims: HashMap<&'a str, &'a TargetClaim>,
    manifestations: HashMap<&'a str, &'a Manifestation>,
    contexts: HashMap<&'a str, &'a ObservationContext>,
    call_sites: HashMap<&'a str, &'a CallSite>,
    inputs: HashMap<&'a str, &'a AcquiredInput>,
    evidence: HashMap<&'a str, &'a EvidenceRecord>,
}

impl<'a> KnowledgeIndex<'a> {
    fn new(knowledge: &'a Knowledge) -> Result<Self, String> {
        Ok(Self {
            entities: unique_index(&knowledge.entities, |record| &record.id)?,
            claims: unique_index(&knowledge.claims, |record| &record.id)?,
            manifestations: unique_index(&knowledge.manifestations, |record| &record.id)?,
            contexts: unique_index(&knowledge.observation_contexts, |record| &record.id)?,
            call_sites: unique_index(&knowledge.call_sites, |record| &record.id)?,
            inputs: unique_index(&knowledge.acquired_inputs, |record| &record.id)?,
            evidence: unique_index(&knowledge.evidence, |record| &record.id)?,
        })
    }
}

fn callable_entity_matches_node(
    entity: &ProgramEntity,
    node: &Node,
    snapshot_id: &str,
    inputs_by_id: &HashMap<&str, &AcquiredInput>,
    entity_language: &str,
) -> bool {
    let local_source_is_consistent = entity.local_input_id.as_deref().is_none_or(|input_id| {
        inputs_by_id
            .get(input_id)
            .is_some_and(|input| entity.source.as_deref() == Some(input.path.as_str()))
    });
    node.kind == "function"
        && node.id != "<indirect>"
        && entity.kind == "callable"
        && entity.name == node.label
        && entity.defined == node.defined
        && entity.language == entity_language
        && entity.language == node.language
        && entity.id == callable_entity_id(snapshot_id, &node.id, entity.local_input_id.as_deref())
        && local_source_is_consistent
}

fn callable_entity_belongs_to_input(entity: &ProgramEntity, input_id: &str) -> bool {
    entity
        .local_input_id
        .as_deref()
        .is_none_or(|local_input_id| local_input_id == input_id)
}

impl Application {
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
        self.validate_document(document)?;
        let mut text = serde_json::to_string_pretty(document).map_err(|error| error.to_string())?;
        text.push('\n');
        Ok(text)
    }

    pub fn load_json(&self, text: &str) -> Result<Document, String> {
        let document: Document = serde_json::from_str(text).map_err(|error| error.to_string())?;
        self.validate_document(&document)?;
        Ok(document)
    }

    fn validate_document(&self, document: &Document) -> Result<(), String> {
        if document.schema_version != "1.0" {
            return Err(format!(
                "unsupported graph schema {:?}",
                document.schema_version
            ));
        }
        if let Some(knowledge) = &document.knowledge {
            let indexes = KnowledgeIndex::new(knowledge)?;
            let entities_by_id = &indexes.entities;
            let claims_by_id = &indexes.claims;
            let manifestations_by_id = &indexes.manifestations;
            let contexts_by_id = &indexes.contexts;
            let call_sites_by_id = &indexes.call_sites;
            let inputs_by_id = &indexes.inputs;
            let evidence_by_id = &indexes.evidence;
            let legacy_nodes_by_id = unique_index::<Node>(&document.nodes, |record| &record.id)?;
            let acquisition_identities: Vec<_> = knowledge
                .acquired_inputs
                .iter()
                .map(|input| (input.path.as_str(), input.content_fingerprint.as_str()))
                .collect();
            if knowledge
                .acquired_inputs
                .iter()
                .any(|input| input.id != acquired_input_id(&input.path, &input.content_fingerprint))
                || knowledge.published_snapshot.id != published_snapshot_id(&acquisition_identities)
                || knowledge.published_snapshot.state != "published"
                || !document
                    .metadata
                    .inputs
                    .iter()
                    .map(String::as_str)
                    .eq(knowledge
                        .acquired_inputs
                        .iter()
                        .map(|input| input.path.as_str()))
            {
                return Err("acquisition provenance identity is internally inconsistent".into());
            }
            if !knowledge.contributor.is_complete()
                || knowledge.contributor_id.is_empty()
                || (!knowledge.acquired_inputs.is_empty()
                    && knowledge.acquisition_contexts.is_empty())
            {
                return Err(
                    "evidence-backed document lacks required provenance contract data".into(),
                );
            }
            if knowledge.contributor_id != evidence_contributor_id(&knowledge.contributor) {
                return Err(
                    "evidence contributor contract identity is internally inconsistent".into(),
                );
            }
            if !matches!(
                knowledge.contributor.call_resolution.as_str(),
                "complete" | "partial" | "absent"
            ) || knowledge
                .acquired_inputs
                .iter()
                .any(|input| input.kind != knowledge.contributor.acquired_input_kind)
            {
                return Err("evidence contributor semantics are internally inconsistent".into());
            }
            let indirect_placeholder_ids: HashSet<_> = document
                .edges
                .iter()
                .filter(|edge| edge.kind == "indirect-call")
                .map(|edge| edge.target.as_str())
                .collect();
            let mut function_nodes_by_label: HashMap<&str, Vec<&Node>> = HashMap::new();
            for node in document.nodes.iter().filter(|node| {
                node.kind == "function" && !indirect_placeholder_ids.contains(node.id.as_str())
            }) {
                function_nodes_by_label
                    .entry(node.label.as_str())
                    .or_default()
                    .push(node);
            }
            let mut callable_nodes_by_entity_id = HashMap::new();
            for entity in knowledge
                .entities
                .iter()
                .filter(|entity| entity.kind == "callable")
            {
                let node = function_nodes_by_label
                    .get(entity.name.as_str())
                    .and_then(|nodes| {
                        nodes.iter().copied().find(|node| {
                            callable_entity_matches_node(
                                entity,
                                node,
                                &knowledge.published_snapshot.id,
                                inputs_by_id,
                                &knowledge.contributor.entity_language,
                            )
                        })
                    });
                let Some(node) = node else {
                    return Err("callable entity identity is internally inconsistent".into());
                };
                callable_nodes_by_entity_id.insert(entity.id.as_str(), node);
            }
            for context in &knowledge.observation_contexts {
                if context.program_snapshot_id != knowledge.published_snapshot.id
                    || context.id
                        != observation_context_id(
                            &context.program_snapshot_id,
                            &context.target,
                            &context.build_configuration,
                            &context.toolchain,
                            &context.extraction_method,
                            &context.analysis_stage,
                            context.runtime_workload.as_deref(),
                        )
                    || context.extraction_method != knowledge.contributor.extraction_method
                    || context.analysis_stage != knowledge.contributor.analysis_stage
                {
                    return Err("observation context identity is internally inconsistent".into());
                }
            }
            let mut acquisition_context_pairs = HashSet::new();
            let mut covered_input_ids = HashSet::new();
            let mut covered_context_ids = HashSet::new();
            for association in &knowledge.acquisition_contexts {
                if !acquisition_context_pairs.insert((
                    association.input_id.as_str(),
                    association.observation_context_id.as_str(),
                )) || !inputs_by_id.contains_key(association.input_id.as_str())
                    || !contexts_by_id.contains_key(association.observation_context_id.as_str())
                {
                    return Err("acquisition context association is internally inconsistent".into());
                }
                covered_input_ids.insert(association.input_id.as_str());
                covered_context_ids.insert(association.observation_context_id.as_str());
            }
            if knowledge
                .acquired_inputs
                .iter()
                .any(|input| !covered_input_ids.contains(input.id.as_str()))
                || knowledge
                    .observation_contexts
                    .iter()
                    .any(|context| !covered_context_ids.contains(context.id.as_str()))
            {
                return Err("acquisition context association is internally inconsistent".into());
            }
            for manifestation in &knowledge.manifestations {
                if manifestation.input_ids.is_empty()
                    || manifestation.input_ids.iter().any(|input_id| {
                        !acquisition_context_pairs.contains(&(
                            input_id.as_str(),
                            manifestation.observation_context_id.as_str(),
                        ))
                    })
                {
                    return Err("acquisition context association is internally inconsistent".into());
                }
                if manifestation.id
                    != manifestation_id(
                        &manifestation.observation_context_id,
                        &manifestation.entity_id,
                    )
                    || entities_by_id
                        .get(manifestation.entity_id.as_str())
                        .is_none_or(|entity| {
                            entity.kind != "callable"
                                || entity.language != knowledge.contributor.entity_language
                                || !callable_nodes_by_entity_id.contains_key(entity.id.as_str())
                                || manifestation.input_ids.iter().any(|input_id| {
                                    !callable_entity_belongs_to_input(entity, input_id)
                                })
                        })
                    || !contexts_by_id.contains_key(manifestation.observation_context_id.as_str())
                {
                    return Err("manifestation identity is internally inconsistent".into());
                }
                if manifestation.kind != knowledge.contributor.manifestation_kind {
                    return Err("evidence contributor semantics are internally inconsistent".into());
                }
            }
            for site in &knowledge.call_sites {
                if !matches!(site.resolution.as_str(), "complete" | "partial" | "absent") {
                    return Err("call-site resolution is invalid".into());
                }
                if site.resolution != knowledge.contributor.call_resolution {
                    return Err("evidence contributor semantics are internally inconsistent".into());
                }
                if !entities_by_id
                    .get(site.entity_id.as_str())
                    .is_some_and(|entity| {
                        entity.kind == "call-site"
                            && entity.language == knowledge.contributor.entity_language
                            && entity.id == call_site_entity_id(&site.id)
                    })
                    || !inputs_by_id
                        .get(site.input_id.as_str())
                        .is_some_and(|input| {
                            site.id
                                == call_site_id(
                                    &knowledge.published_snapshot.id,
                                    &input.path,
                                    &input.content_fingerprint,
                                    site.line,
                                    site.ordinal,
                                )
                        })
                    || !entities_by_id
                        .get(site.caller_entity_id.as_str())
                        .is_some_and(|entity| {
                            entity.kind == "callable"
                                && callable_nodes_by_entity_id.contains_key(entity.id.as_str())
                                && callable_entity_belongs_to_input(entity, &site.input_id)
                        })
                    || !contexts_by_id.contains_key(site.observation_context_id.as_str())
                    || !manifestations_by_id
                        .get(site.caller_manifestation_id.as_str())
                        .is_some_and(|manifestation| {
                            manifestation.entity_id == site.caller_entity_id
                                && manifestation.observation_context_id
                                    == site.observation_context_id
                        })
                {
                    return Err("evidence-backed relationship is internally inconsistent".into());
                }
                if !acquisition_context_pairs
                    .contains(&(site.input_id.as_str(), site.observation_context_id.as_str()))
                {
                    return Err("acquisition context association is internally inconsistent".into());
                }
                if manifestations_by_id
                    .get(site.caller_manifestation_id.as_str())
                    .is_none_or(|manifestation| !manifestation.input_ids.contains(&site.input_id))
                {
                    return Err("evidence-backed relationship is internally inconsistent".into());
                }
            }
            for record in &knowledge.evidence {
                if record.kind != knowledge.contributor.evidence_kind {
                    return Err("evidence contributor semantics are internally inconsistent".into());
                }
                let consistent = call_sites_by_id
                    .get(record.call_site_id.as_str())
                    .copied()
                    .zip(inputs_by_id.get(record.input_id.as_str()).copied())
                    .is_some_and(|(site, input)| {
                        site.input_id == record.input_id
                            && site.observation_context_id == record.observation_context_id
                            && contexts_by_id.contains_key(record.observation_context_id.as_str())
                            && record.content_fingerprint == input.content_fingerprint
                            && record.id
                                == evidence_id(
                                    &record.observation_context_id,
                                    &input.path,
                                    &input.content_fingerprint,
                                    site.line,
                                    site.ordinal,
                                )
                    });
                if !consistent {
                    return Err("evidence provenance identity is internally inconsistent".into());
                }
            }
            for claim in &knowledge.claims {
                if claim.kind != knowledge.contributor.claim_kind
                    || claim.derivation != knowledge.contributor.derivation
                {
                    return Err("evidence contributor semantics are internally inconsistent".into());
                }
                let Some((site, input)) = call_sites_by_id
                    .get(claim.call_site_id.as_str())
                    .copied()
                    .and_then(|site| {
                        inputs_by_id
                            .get(site.input_id.as_str())
                            .copied()
                            .map(|input| (site, input))
                    })
                else {
                    return Err("target claim integrity is internally inconsistent".into());
                };
                if claim.observation_context_id != site.observation_context_id
                    || !contexts_by_id.contains_key(claim.observation_context_id.as_str())
                    || claim.id
                        != target_claim_id(
                            &claim.observation_context_id,
                            &input.path,
                            &input.content_fingerprint,
                            site.line,
                            site.ordinal,
                            &claim.kind,
                        )
                {
                    return Err("target claim identity is internally inconsistent".into());
                }
                let target_entity = entities_by_id.get(claim.target_entity_id.as_str()).copied();
                let target_node = callable_nodes_by_entity_id
                    .get(claim.target_entity_id.as_str())
                    .copied();
                let target_manifestation = manifestations_by_id
                    .get(claim.target_manifestation_id.as_str())
                    .copied();
                let mut evidence_ids = HashSet::new();
                let consistent = target_entity
                    .zip(target_node)
                    .is_some_and(|(entity, node)| {
                        entity.kind == "callable"
                            && entity.name == node.label
                            && entity.language == knowledge.contributor.entity_language
                            && callable_entity_belongs_to_input(entity, &site.input_id)
                    })
                    && target_manifestation.is_some_and(|manifestation| {
                        manifestation.entity_id == claim.target_entity_id
                            && manifestation.observation_context_id == claim.observation_context_id
                            && manifestation.input_ids.contains(&site.input_id)
                    })
                    && !claim.evidence_ids.is_empty()
                    && claim.evidence_ids.iter().all(|evidence_id| {
                        evidence_ids.insert(evidence_id.as_str())
                            && evidence_by_id
                                .get(evidence_id.as_str())
                                .is_some_and(|evidence| {
                                    evidence.call_site_id == claim.call_site_id
                                        && evidence.observation_context_id
                                            == claim.observation_context_id
                                        && evidence.input_id == site.input_id
                                        && evidence.content_fingerprint == input.content_fingerprint
                                        && target_node
                                            .is_some_and(|node| evidence.observed_callee == node.id)
                                })
                    });
                if !consistent {
                    return Err("target claim integrity is internally inconsistent".into());
                }
            }
            let mut canonical_direct_calls = HashMap::new();
            let mut projected_claim_ids = HashSet::new();
            let mut explanation_handles = HashSet::new();
            for relationship in &knowledge.call_graph.relationships {
                if !projected_claim_ids.insert(relationship.claim_id.as_str())
                    || !explanation_handles.insert(relationship.explanation_handle.as_str())
                {
                    return Err("duplicate projected relationship ownership".into());
                }
                let caller_entity = entities_by_id
                    .get(relationship.caller_entity_id.as_str())
                    .copied();
                let callee_entity = entities_by_id
                    .get(relationship.callee_entity_id.as_str())
                    .copied();
                let legacy_caller = legacy_nodes_by_id
                    .get(relationship.legacy_caller_id.as_str())
                    .copied();
                let legacy_callee = legacy_nodes_by_id
                    .get(relationship.legacy_callee_id.as_str())
                    .copied();
                let claim = claims_by_id.get(relationship.claim_id.as_str()).copied();
                let consistent = caller_entity
                    .zip(legacy_caller)
                    .is_some_and(|(entity, node)| {
                        callable_entity_matches_node(
                            entity,
                            node,
                            &knowledge.published_snapshot.id,
                            inputs_by_id,
                            &knowledge.contributor.entity_language,
                        )
                    })
                    && callee_entity
                        .zip(legacy_callee)
                        .is_some_and(|(entity, node)| {
                            callable_entity_matches_node(
                                entity,
                                node,
                                &knowledge.published_snapshot.id,
                                inputs_by_id,
                                &knowledge.contributor.entity_language,
                            )
                        })
                    && claim.is_some_and(|claim| {
                        relationship.explanation_handle == format!("explain:{}", claim.id)
                            && claim.target_entity_id == relationship.callee_entity_id
                            && call_sites_by_id
                                .get(claim.call_site_id.as_str())
                                .is_some_and(|site| {
                                    site.caller_entity_id == relationship.caller_entity_id
                                })
                    });
                if !consistent {
                    return Err("evidence-backed relationship is internally inconsistent".into());
                }
                *canonical_direct_calls
                    .entry((
                        relationship.legacy_caller_id.as_str(),
                        relationship.legacy_callee_id.as_str(),
                    ))
                    .or_insert(0_u64) += 1;
            }
            let mut legacy_direct_calls = HashMap::new();
            for edge in document
                .edges
                .iter()
                .filter(|edge| edge.kind == "direct-call")
            {
                let count = legacy_direct_calls
                    .entry((edge.source.as_str(), edge.target.as_str()))
                    .or_insert(0_u64);
                *count = count.checked_add(edge.call_count).ok_or_else(|| {
                    "malformed document: legacy direct-call count overflow".to_owned()
                })?;
            }
            if legacy_direct_calls != canonical_direct_calls {
                return Err(
                    "legacy direct-call edges do not match the evidence-backed projection".into(),
                );
            }
        }
        Ok(())
    }

    pub fn query(&self, document: Document, query: Query) -> Result<QueryResult, String> {
        if let Query::CallsToNamedCallee { callee } = &query {
            let result = self
                .named_call_views(&document)?
                .into_iter()
                .filter(|view| view.relationship.callee_name == *callee)
                .map(|view| view.relationship)
                .collect();
            return Ok(QueryResult::CallsToNamedCallee(result));
        }
        if let Query::Explain { handle } = &query {
            if document.knowledge.is_none() {
                return Err("explanations require evidence-backed snapshot data".to_owned());
            }
            if !handle.starts_with("explain:") {
                return Err(format!("invalid explanation handle '{handle}'"));
            }
            let explanation = self
                .named_call_views(&document)?
                .into_iter()
                .find(|view| view.relationship.explanation_handle == *handle)
                .map(|view| view.explanation)
                .ok_or_else(|| format!("unknown explanation handle '{handle}'"))?;
            return Ok(QueryResult::Explain(Box::new(explanation)));
        }
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
            Query::CallsToNamedCallee { .. } | Query::Explain { .. } => unreachable!(),
        })
    }

    pub fn render_viewer(&self, document: &Document) -> Result<String, String> {
        self.validate_document(document)?;
        let named_calls = if document.knowledge.is_some() {
            self.named_call_views(document)?
        } else {
            Vec::new()
        };
        viewer::render_html(document, &named_calls)
    }

    fn named_call_views(&self, document: &Document) -> Result<Vec<NamedCallView>, String> {
        let knowledge = document
            .knowledge
            .as_ref()
            .ok_or_else(|| "named call queries require evidence-backed snapshot data".to_owned())?;
        let indexes = KnowledgeIndex::new(knowledge)?;
        knowledge
            .call_graph
            .relationships
            .iter()
            .map(|projected| {
                let caller = indexes
                    .entities
                    .get(projected.caller_entity_id.as_str())
                    .ok_or_else(|| "relationship has no caller entity".to_owned())?;
                let callee = indexes
                    .entities
                    .get(projected.callee_entity_id.as_str())
                    .ok_or_else(|| "relationship has no callee entity".to_owned())?;
                let claim = indexes
                    .claims
                    .get(projected.claim_id.as_str())
                    .copied()
                    .ok_or_else(|| "relationship has no target claim".to_owned())?;
                let evidence_id = claim
                    .evidence_ids
                    .first()
                    .ok_or_else(|| format!("claim '{}' has no supporting evidence", claim.id))?;
                let record = indexes
                    .evidence
                    .get(evidence_id.as_str())
                    .copied()
                    .ok_or_else(|| format!("claim '{}' has no supporting evidence", claim.id))?;
                let context = indexes
                    .contexts
                    .get(claim.observation_context_id.as_str())
                    .copied()
                    .ok_or_else(|| format!("claim '{}' has no observation context", claim.id))?;
                Ok(NamedCallView {
                    legacy_caller_id: projected.legacy_caller_id.clone(),
                    relationship: NamedCallRelationship {
                        caller_name: caller.name.clone(),
                        callee_name: callee.name.clone(),
                        explanation_handle: projected.explanation_handle.clone(),
                    },
                    explanation: Explanation {
                        snapshot_id: knowledge.published_snapshot.id.clone(),
                        claim: claim.clone(),
                        evidence: record.clone(),
                        observation_context: context.clone(),
                        derivation: claim.derivation.clone(),
                    },
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serialize_unchecked(document: &Document) -> String {
        serde_json::to_string(document).unwrap()
    }

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
        let knowledge = document.knowledge.as_ref().unwrap();
        let direct_claim = knowledge
            .claims
            .iter()
            .find(|claim| {
                knowledge
                    .entities
                    .iter()
                    .any(|entity| entity.id == claim.target_entity_id && entity.name == "step")
            })
            .unwrap();
        let call_site = knowledge
            .call_sites
            .iter()
            .find(|site| site.id == direct_claim.call_site_id)
            .unwrap();
        assert_ne!(call_site.caller_entity_id, direct_claim.target_entity_id);
        assert_ne!(
            call_site.caller_manifestation_id,
            direct_claim.target_manifestation_id
        );
        assert_ne!(call_site.id, direct_claim.target_entity_id);
        assert!(
            knowledge
                .entities
                .iter()
                .any(|entity| entity.id == call_site.entity_id && entity.kind == "call-site")
        );
        assert_eq!(call_site.resolution, "complete");

        let json = application.export_json(&document).unwrap();
        assert!(json.ends_with('\n'));
        let loaded = application.load_json(&json).unwrap();
        assert_eq!(loaded.schema_version, "1.0");
        assert_eq!(loaded.nodes.len(), 3);
        assert_eq!(loaded.edges.len(), 3);
    }

    #[test]
    fn rejects_invalid_provenance_at_the_export_boundary() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        document.knowledge.as_mut().unwrap().evidence[0].id = "evidence:forged".into();

        assert_eq!(
            application.export_json(&document).unwrap_err(),
            "evidence provenance identity is internally inconsistent"
        );
    }

    #[test]
    fn rejects_forged_orphan_evidence_at_the_load_boundary() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let mut orphan = document.knowledge.as_ref().unwrap().evidence[0].clone();
        orphan.id = "evidence:forged-orphan".into();
        document.knowledge.as_mut().unwrap().evidence.push(orphan);

        let json = serialize_unchecked(&document);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "evidence provenance identity is internally inconsistent"
        );
    }

    #[test]
    fn rejects_forged_orphan_manifestations_at_export_and_load_boundaries() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let mut invalid = document;
        let mut manifestation = invalid.knowledge.as_ref().unwrap().manifestations[0].clone();
        manifestation.id = "manifestation:forged".into();
        invalid
            .knowledge
            .as_mut()
            .unwrap()
            .manifestations
            .push(manifestation);

        assert_eq!(
            application.export_json(&invalid).unwrap_err(),
            "manifestation identity is internally inconsistent"
        );
        let json = serialize_unchecked(&invalid);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "manifestation identity is internally inconsistent"
        );
    }

    #[test]
    fn rejects_unknown_call_site_resolution_at_the_document_boundary() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        document.knowledge.as_mut().unwrap().call_sites[0].resolution = "certain".into();

        assert_eq!(
            application.export_json(&document).unwrap_err(),
            "call-site resolution is invalid"
        );
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

    #[test]
    fn review_fixes_reject_invalid_provenance_at_the_viewer_boundary() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        document.knowledge.as_mut().unwrap().observation_contexts[0].toolchain =
            "tampered toolchain".into();

        assert_eq!(
            application.render_viewer(&document).unwrap_err(),
            "observation context identity is internally inconsistent"
        );
    }

    #[test]
    fn explains_a_named_callee_from_evidence_through_the_application_seam() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();

        let relationships = application
            .query(
                document.clone(),
                Query::CallsToNamedCallee {
                    callee: "step".into(),
                },
            )
            .unwrap();
        let relationships = serde_json::to_value(relationships).unwrap();
        assert_eq!(relationships[0]["caller_name"], "main");
        assert_eq!(relationships[0]["callee_name"], "step");
        let handle = relationships[0]["explanation_handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let exported = application.export_json(&document).unwrap();
        let viewer = application.render_viewer(&document).unwrap();
        assert!(exported.contains(&handle));
        assert!(viewer.contains(&handle));
        assert!(viewer.contains("Evidence-backed calls"));
        assert!(viewer.contains("observation_context"));
        let viewer_data = viewer
            .split_once("const VIEW=")
            .unwrap()
            .1
            .split_once(",DATA=VIEW.document")
            .unwrap()
            .0;
        let viewer_data: serde_json::Value = serde_json::from_str(viewer_data).unwrap();
        let viewer_explanation = viewer_data["named_calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|call| call["relationship"]["explanation_handle"] == handle)
            .unwrap()["explanation"]
            .clone();

        let explanation = application
            .query(document, Query::Explain { handle })
            .unwrap();
        let explanation = serde_json::to_value(explanation).unwrap();
        assert_eq!(viewer_explanation, explanation);
        assert_eq!(explanation["claim"]["kind"], "direct-target");
        assert_eq!(explanation["evidence"]["kind"], "llvm-direct-call");
        assert_eq!(explanation["derivation"], "direct LLVM callee operand");
        assert_eq!(
            explanation["observation_context"]["program_snapshot_id"],
            explanation["snapshot_id"]
        );
        for field in [
            "target",
            "build_configuration",
            "toolchain",
            "extraction_method",
            "analysis_stage",
        ] {
            assert!(
                !explanation["observation_context"][field]
                    .as_str()
                    .unwrap()
                    .is_empty()
            );
        }
    }

    #[test]
    fn rejects_evidence_backed_documents_with_divergent_legacy_calls() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let edge = document
            .edges
            .iter_mut()
            .find(|edge| edge.kind == "direct-call")
            .unwrap();
        edge.target = "contradictory-target".into();

        let json = serialize_unchecked(&document);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "legacy direct-call edges do not match the evidence-backed projection"
        );
    }

    #[test]
    fn rejects_legacy_direct_call_count_overflow() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let edge = document
            .edges
            .iter_mut()
            .find(|edge| edge.kind == "direct-call")
            .unwrap();
        edge.call_count = u64::MAX;
        let mut overflow = edge.clone();
        overflow.call_count = 1;
        document.edges.push(overflow);

        let json = serialize_unchecked(&document);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "malformed document: legacy direct-call count overflow"
        );
    }

    #[test]
    fn rejects_duplicate_projected_relationships() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let relationship = document
            .knowledge
            .as_ref()
            .unwrap()
            .call_graph
            .relationships[0]
            .clone();
        document
            .edges
            .iter_mut()
            .find(|edge| {
                edge.kind == "direct-call"
                    && edge.source == relationship.legacy_caller_id
                    && edge.target == relationship.legacy_callee_id
            })
            .unwrap()
            .call_count += 1;
        document
            .knowledge
            .as_mut()
            .unwrap()
            .call_graph
            .relationships
            .push(relationship);

        let json = serialize_unchecked(&document);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "duplicate projected relationship ownership"
        );
    }

    #[test]
    fn rejects_relationships_with_divergent_canonical_entities() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let knowledge = document.knowledge.as_mut().unwrap();
        let relationship = &mut knowledge.call_graph.relationships[0];
        relationship.callee_entity_id = knowledge
            .entities
            .iter()
            .find(|entity| {
                entity.name != relationship.legacy_callee_id && entity.kind == "callable"
            })
            .unwrap()
            .id
            .clone();

        let json = serialize_unchecked(&document);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "evidence-backed relationship is internally inconsistent"
        );
    }

    #[test]
    fn review_fixes_reject_non_callable_projected_endpoints() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let relationship = &document
            .knowledge
            .as_ref()
            .unwrap()
            .call_graph
            .relationships[0];
        let mut invalid_documents = Vec::new();
        for entity_id in [
            &relationship.caller_entity_id,
            &relationship.callee_entity_id,
        ] {
            let mut invalid = document.clone();
            invalid
                .knowledge
                .as_mut()
                .unwrap()
                .entities
                .iter_mut()
                .find(|entity| entity.id == *entity_id)
                .unwrap()
                .kind = "call-site".into();
            invalid_documents.push(invalid);
        }

        for invalid in invalid_documents {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                "manifestation identity is internally inconsistent"
            );
        }
    }

    #[test]
    fn rejects_legacy_aliases_with_the_same_display_label() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let relationship = &document
            .knowledge
            .as_ref()
            .unwrap()
            .call_graph
            .relationships[0];
        let original_caller = relationship.legacy_caller_id.clone();
        let alias_id = "legacy-caller-alias";
        let mut alias = document
            .nodes
            .iter()
            .find(|node| node.id == original_caller)
            .unwrap()
            .clone();
        alias.id = alias_id.into();
        document.nodes.push(alias);
        document
            .knowledge
            .as_mut()
            .unwrap()
            .call_graph
            .relationships[0]
            .legacy_caller_id = alias_id.into();
        document
            .edges
            .iter_mut()
            .find(|edge| edge.kind == "direct-call" && edge.source == original_caller)
            .unwrap()
            .source = alias_id.into();

        let json = serialize_unchecked(&document);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "evidence-backed relationship is internally inconsistent"
        );
    }

    #[test]
    fn rejects_relationships_with_divergent_claims_or_handles() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let mut wrong_claim = document.clone();
        wrong_claim
            .knowledge
            .as_mut()
            .unwrap()
            .call_graph
            .relationships[0]
            .claim_id = "claim:unrelated".into();
        let mut wrong_handle = document;
        wrong_handle
            .knowledge
            .as_mut()
            .unwrap()
            .call_graph
            .relationships[0]
            .explanation_handle = "explain:unrelated".into();

        for invalid in [wrong_claim, wrong_handle] {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                "evidence-backed relationship is internally inconsistent"
            );
        }
    }

    #[test]
    fn review_fixes_reject_forged_claim_identities_collection_wide() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let mut projected = document.clone();
        let knowledge = projected.knowledge.as_mut().unwrap();
        let original_id = knowledge.call_graph.relationships[0].claim_id.clone();
        let claim = knowledge
            .claims
            .iter_mut()
            .find(|claim| claim.id == original_id)
            .unwrap();
        claim.id = "claim:forged".into();
        knowledge.call_graph.relationships[0].claim_id = claim.id.clone();
        knowledge.call_graph.relationships[0].explanation_handle = format!("explain:{}", claim.id);

        let mut orphan = document;
        let mut claim = orphan.knowledge.as_ref().unwrap().claims[0].clone();
        claim.id = "claim:forged-orphan".into();
        orphan.knowledge.as_mut().unwrap().claims.push(claim);

        for invalid in [projected, orphan] {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                "target claim identity is internally inconsistent"
            );
        }
    }

    #[test]
    fn review_fixes_reject_claims_with_mixed_or_missing_evidence() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let mut mixed = document.clone();
        let knowledge = mixed.knowledge.as_mut().unwrap();
        let claim_id = knowledge.call_graph.relationships[0].claim_id.clone();
        let claim = knowledge
            .claims
            .iter_mut()
            .find(|claim| claim.id == claim_id)
            .unwrap();
        let unrelated_evidence = knowledge
            .evidence
            .iter()
            .find(|evidence| evidence.call_site_id != claim.call_site_id)
            .unwrap()
            .id
            .clone();
        claim.evidence_ids.push(unrelated_evidence);
        let mut missing = document;
        let knowledge = missing.knowledge.as_mut().unwrap();
        let claim_id = knowledge.call_graph.relationships[0].claim_id.clone();
        knowledge
            .claims
            .iter_mut()
            .find(|claim| claim.id == claim_id)
            .unwrap()
            .evidence_ids = vec!["evidence:missing".into()];

        for invalid in [mixed, missing] {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                "target claim integrity is internally inconsistent"
            );
        }
    }

    #[test]
    fn review_fixes_reject_claims_with_wrong_manifestations_or_contexts() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let mut wrong_manifestation = document.clone();
        let knowledge = wrong_manifestation.knowledge.as_mut().unwrap();
        let claim_id = knowledge.call_graph.relationships[0].claim_id.clone();
        let claim = knowledge
            .claims
            .iter_mut()
            .find(|claim| claim.id == claim_id)
            .unwrap();
        claim.target_manifestation_id = knowledge
            .manifestations
            .iter()
            .find(|manifestation| manifestation.entity_id != claim.target_entity_id)
            .unwrap()
            .id
            .clone();
        let mut missing_context = document.clone();
        missing_context
            .knowledge
            .as_mut()
            .unwrap()
            .observation_contexts
            .clear();
        let mut inconsistent_context = document;
        inconsistent_context
            .knowledge
            .as_mut()
            .unwrap()
            .observation_contexts[0]
            .program_snapshot_id = "snapshot:unrelated".into();

        for (invalid, expected_error) in [
            (
                wrong_manifestation,
                "target claim integrity is internally inconsistent",
            ),
            (
                missing_context,
                "acquisition context association is internally inconsistent",
            ),
            (
                inconsistent_context,
                "observation context identity is internally inconsistent",
            ),
        ] {
            let json = serialize_unchecked(&invalid);
            assert_eq!(application.load_json(&json).unwrap_err(), expected_error);
        }
    }

    #[test]
    fn rejects_call_sites_with_wrong_caller_manifestations() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let knowledge = document.knowledge.as_mut().unwrap();
        let relationship = &knowledge.call_graph.relationships[0];
        let claim = knowledge
            .claims
            .iter()
            .find(|claim| claim.id == relationship.claim_id)
            .unwrap();
        let site = knowledge
            .call_sites
            .iter_mut()
            .find(|site| site.id == claim.call_site_id)
            .unwrap();
        site.caller_manifestation_id = knowledge
            .manifestations
            .iter()
            .find(|manifestation| manifestation.entity_id != relationship.caller_entity_id)
            .unwrap()
            .id
            .clone();

        let json = serialize_unchecked(&document);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "evidence-backed relationship is internally inconsistent"
        );
    }

    #[test]
    fn rejects_call_sites_with_invalid_entity_identity() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let relationship = &document
            .knowledge
            .as_ref()
            .unwrap()
            .call_graph
            .relationships[0];
        let claim = document
            .knowledge
            .as_ref()
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.id == relationship.claim_id)
            .unwrap();
        let call_site_id = claim.call_site_id.clone();

        let mut wrong_kind = document.clone();
        let knowledge = wrong_kind.knowledge.as_mut().unwrap();
        let caller_entity_id = relationship.caller_entity_id.clone();
        knowledge
            .call_sites
            .iter_mut()
            .find(|site| site.id == call_site_id)
            .unwrap()
            .entity_id = caller_entity_id;

        let mut missing = document;
        missing
            .knowledge
            .as_mut()
            .unwrap()
            .call_sites
            .iter_mut()
            .find(|site| site.id == call_site_id)
            .unwrap()
            .entity_id = "entity:missing-call-site".into();

        for invalid in [wrong_kind, missing] {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                "evidence-backed relationship is internally inconsistent"
            );
        }
    }

    #[test]
    fn review_fixes_reject_call_sites_with_tampered_location_identity() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        document.knowledge.as_mut().unwrap().call_sites[0].ordinal += 1;

        let json = serialize_unchecked(&document);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "evidence-backed relationship is internally inconsistent"
        );
    }

    #[test]
    fn rejects_orphan_call_sites_with_invalid_entity_identity() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let knowledge = document.knowledge.as_ref().unwrap();
        let original = knowledge.call_sites[0].clone();
        let caller_entity_id = original.caller_entity_id.clone();
        let mut invalid_documents = Vec::new();

        let mut callable_entity = document.clone();
        let mut orphan = original.clone();
        orphan.id = "call-site:orphan-callable".into();
        orphan.entity_id = caller_entity_id;
        callable_entity
            .knowledge
            .as_mut()
            .unwrap()
            .call_sites
            .push(orphan);
        invalid_documents.push(callable_entity);

        let mut missing_entity = document.clone();
        let mut orphan = original.clone();
        orphan.id = "call-site:orphan-missing".into();
        orphan.entity_id = "entity:missing-call-site".into();
        missing_entity
            .knowledge
            .as_mut()
            .unwrap()
            .call_sites
            .push(orphan);
        invalid_documents.push(missing_entity);

        let mut mismatched_entity = document;
        let mut orphan = original;
        orphan.id = "call-site:orphan-mismatched".into();
        mismatched_entity
            .knowledge
            .as_mut()
            .unwrap()
            .call_sites
            .push(orphan);
        invalid_documents.push(mismatched_entity);

        for invalid in invalid_documents {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                "evidence-backed relationship is internally inconsistent"
            );
        }
    }

    #[test]
    fn review_fixes_reject_orphan_call_sites_with_broken_provenance_chains() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let knowledge = document.knowledge.as_ref().unwrap();
        let original = knowledge.call_sites[0].clone();
        let input = knowledge
            .acquired_inputs
            .iter()
            .find(|input| input.id == original.input_id)
            .unwrap();
        let ordinal = original.ordinal + knowledge.call_sites.len() + 1;
        let id = call_site_id(
            &knowledge.published_snapshot.id,
            &input.path,
            &input.content_fingerprint,
            original.line,
            ordinal,
        );
        let entity_id = call_site_entity_id(&id);
        let mut orphan = original.clone();
        orphan.id = id;
        orphan.entity_id = entity_id.clone();
        orphan.ordinal = ordinal;
        let mut entity = knowledge
            .entities
            .iter()
            .find(|entity| entity.id == original.entity_id)
            .unwrap()
            .clone();
        entity.id = entity_id;

        let mut invalid_documents = Vec::new();
        let mutations: [fn(&mut CallSite); 3] = [
            |site: &mut CallSite| site.caller_entity_id = "entity:missing-caller".into(),
            |site: &mut CallSite| {
                site.caller_manifestation_id = "manifestation:missing-caller".into()
            },
            |site: &mut CallSite| site.observation_context_id = "context:missing".into(),
        ];
        for mutate in mutations {
            let mut invalid = document.clone();
            let mut invalid_site = orphan.clone();
            mutate(&mut invalid_site);
            let knowledge = invalid.knowledge.as_mut().unwrap();
            knowledge.entities.push(entity.clone());
            knowledge.call_sites.push(invalid_site);
            invalid_documents.push(invalid);
        }

        for invalid in invalid_documents {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                "evidence-backed relationship is internally inconsistent"
            );
        }
    }

    #[test]
    fn review_fixes_persist_contexts_for_inputs_without_observations() {
        let application = Application;
        let mut graph = llvm::parse_llvm_ir(
            "define void @caller() {\n  call void @callee()\n  ret void\n}",
            Some("observed.ll"),
        )
        .unwrap();
        graph.merge(llvm::parse_llvm_ir("", Some("empty.ll")).unwrap());
        let document = Document::from_graph(&graph, analysis::summary(&graph));
        let knowledge = document.knowledge.as_ref().unwrap();
        let empty_input = knowledge
            .acquired_inputs
            .iter()
            .find(|input| input.path == "empty.ll")
            .unwrap();
        let empty_context_ids: Vec<_> = knowledge
            .acquisition_contexts
            .iter()
            .filter(|association| association.input_id == empty_input.id)
            .map(|association| association.observation_context_id.as_str())
            .collect();

        assert_eq!(empty_context_ids.len(), 1);
        assert!(
            knowledge.observation_contexts.iter().any(|context| {
                context.id == empty_context_ids[0] && context.target == "empty.ll"
            })
        );
        let exported = application.export_json(&document).unwrap();
        assert!(application.load_json(&exported).is_ok());
    }

    #[test]
    fn review_fixes_reject_cross_input_context_reassignment() {
        let application = Application;
        let mut graph = llvm::parse_llvm_ir(
            "define void @caller() {\n  call void @callee()\n  ret void\n}",
            Some("observed.ll"),
        )
        .unwrap();
        graph.merge(llvm::parse_llvm_ir("", Some("empty.ll")).unwrap());
        let mut document = Document::from_graph(&graph, analysis::summary(&graph));
        let knowledge = document.knowledge.as_mut().unwrap();
        let other_input = knowledge
            .acquired_inputs
            .iter()
            .find(|input| input.path == "empty.ll")
            .unwrap()
            .clone();
        let site_index = knowledge
            .call_sites
            .iter()
            .position(|site| site.input_id != other_input.id)
            .unwrap();
        let old_site_id = knowledge.call_sites[site_index].id.clone();
        let context_id = knowledge.call_sites[site_index]
            .observation_context_id
            .clone();
        let line = knowledge.call_sites[site_index].line;
        let ordinal = knowledge.call_sites[site_index].ordinal;
        let old_entity_id = knowledge.call_sites[site_index].entity_id.clone();
        let new_site_id = call_site_id(
            &knowledge.published_snapshot.id,
            &other_input.path,
            &other_input.content_fingerprint,
            line,
            ordinal,
        );
        let new_entity_id = call_site_entity_id(&new_site_id);
        let evidence = knowledge
            .evidence
            .iter_mut()
            .find(|evidence| evidence.call_site_id == old_site_id)
            .unwrap();
        evidence.id = evidence_id(
            &context_id,
            &other_input.path,
            &other_input.content_fingerprint,
            line,
            ordinal,
        );
        evidence.input_id = other_input.id.clone();
        evidence.content_fingerprint = other_input.content_fingerprint.clone();
        evidence.call_site_id = new_site_id.clone();
        let new_evidence_id = evidence.id.clone();
        let claim = knowledge
            .claims
            .iter_mut()
            .find(|claim| claim.call_site_id == old_site_id)
            .unwrap();
        let old_claim_id = claim.id.clone();
        claim.id = target_claim_id(
            &context_id,
            &other_input.path,
            &other_input.content_fingerprint,
            line,
            ordinal,
            &claim.kind,
        );
        claim.call_site_id = new_site_id.clone();
        claim.evidence_ids = vec![new_evidence_id];
        let new_claim_id = claim.id.clone();
        let relationship = knowledge
            .call_graph
            .relationships
            .iter_mut()
            .find(|relationship| relationship.claim_id == old_claim_id)
            .unwrap();
        relationship.claim_id = new_claim_id.clone();
        relationship.explanation_handle = format!("explain:{new_claim_id}");
        let entity = knowledge
            .entities
            .iter_mut()
            .find(|entity| entity.id == old_entity_id)
            .unwrap();
        entity.id = new_entity_id.clone();
        entity.name = format!("{}:{line}", other_input.path);
        entity.source = Some(other_input.path.clone());
        let site = &mut knowledge.call_sites[site_index];
        site.id = new_site_id;
        site.entity_id = new_entity_id;
        site.input_id = other_input.id;

        let json = serialize_unchecked(&document);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "acquisition context association is internally inconsistent"
        );
    }

    #[test]
    fn review_fixes_validate_unprojected_claim_integrity() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let knowledge = document.knowledge.as_mut().unwrap();
        let original = knowledge.call_sites[0].clone();
        let input = knowledge
            .acquired_inputs
            .iter()
            .find(|input| input.id == original.input_id)
            .unwrap();
        let ordinal = original.ordinal + knowledge.call_sites.len() + 100;
        let site_id = call_site_id(
            &knowledge.published_snapshot.id,
            &input.path,
            &input.content_fingerprint,
            original.line,
            ordinal,
        );
        let entity_id = call_site_entity_id(&site_id);
        let mut site = original.clone();
        site.id = site_id.clone();
        site.entity_id = entity_id.clone();
        site.ordinal = ordinal;
        let mut entity = knowledge
            .entities
            .iter()
            .find(|entity| entity.id == original.entity_id)
            .unwrap()
            .clone();
        entity.id = entity_id;
        let mut claim = knowledge.claims[0].clone();
        claim.id = target_claim_id(
            &claim.observation_context_id,
            &input.path,
            &input.content_fingerprint,
            site.line,
            site.ordinal,
            &claim.kind,
        );
        claim.call_site_id = site_id;
        claim.target_entity_id = "entity:missing-target".into();
        claim.target_manifestation_id = "manifestation:missing-target".into();
        claim.evidence_ids.clear();
        knowledge.entities.push(entity);
        knowledge.call_sites.push(site);
        knowledge.claims.push(claim);

        let json = serialize_unchecked(&document);
        assert_eq!(
            application.load_json(&json).unwrap_err(),
            "target claim integrity is internally inconsistent"
        );
    }

    #[test]
    fn review_fixes_validate_contributor_owned_semantics() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let mut invalid_documents = Vec::new();
        let mut resolution = document.clone();
        resolution.knowledge.as_mut().unwrap().call_sites[0].resolution = "absent".into();
        invalid_documents.push(resolution);
        let mut manifestation = document.clone();
        manifestation.knowledge.as_mut().unwrap().manifestations[0].kind = "other-kind".into();
        invalid_documents.push(manifestation);
        let mut evidence = document.clone();
        evidence.knowledge.as_mut().unwrap().evidence[0].kind = "other-kind".into();
        invalid_documents.push(evidence);
        let mut claim = document.clone();
        claim.knowledge.as_mut().unwrap().claims[0].derivation = "other derivation".into();
        invalid_documents.push(claim);

        for invalid in invalid_documents {
            assert_eq!(
                application.export_json(&invalid).unwrap_err(),
                "evidence contributor semantics are internally inconsistent"
            );
        }

        let mut changed_contract = document;
        let knowledge = changed_contract.knowledge.as_mut().unwrap();
        knowledge.contributor.call_resolution = "absent".into();
        for site in &mut knowledge.call_sites {
            site.resolution = "absent".into();
        }
        assert_eq!(
            application.export_json(&changed_contract).unwrap_err(),
            "evidence contributor contract identity is internally inconsistent"
        );
    }

    #[test]
    fn review_fixes_reject_unverifiable_pre_contract_knowledge() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let mut value = serde_json::to_value(document).unwrap();
        let knowledge = value["knowledge"].as_object_mut().unwrap();
        knowledge.remove("contributor_id");
        knowledge.remove("contributor");
        knowledge.remove("acquisition_contexts");

        assert_eq!(
            application.load_json(&value.to_string()).unwrap_err(),
            "evidence-backed document lacks required provenance contract data"
        );
    }

    #[test]
    fn review_fixes_keep_indirect_placeholders_out_of_canonical_entities() {
        let graph = llvm::parse_llvm_ir(
            "define void @caller() {\n  call void %callback()\n  ret void\n}",
            Some("indirect.ll"),
        )
        .unwrap();
        let document = Document::from_graph(&graph, analysis::summary(&graph));

        assert!(document.nodes.iter().any(|node| node.id == "<indirect>"));
        let knowledge = document.knowledge.unwrap();
        assert!(
            knowledge
                .entities
                .iter()
                .all(|entity| entity.name != "indirect call")
        );
        assert!(knowledge.manifestations.iter().all(|manifestation| {
            knowledge
                .entities
                .iter()
                .any(|entity| entity.id == manifestation.entity_id)
        }));
    }

    #[test]
    fn review_fixes_reject_forged_indirect_placeholder_callables() {
        let application = Application;
        let graph = llvm::parse_llvm_ir(
            "define void @caller() {\n  call void @callee(), call void %callback()\n  ret void\n}",
            Some("indirect-forgery.ll"),
        )
        .unwrap();
        let mut document = Document::from_graph(&graph, analysis::summary(&graph));
        let placeholder = document
            .nodes
            .iter()
            .find(|node| node.id == "<indirect>")
            .unwrap()
            .clone();
        let knowledge = document.knowledge.as_mut().unwrap();
        let relationship_index = knowledge
            .call_graph
            .relationships
            .iter()
            .position(|relationship| relationship.legacy_callee_id == "callee")
            .unwrap();
        let old_entity_id = knowledge.call_graph.relationships[relationship_index]
            .callee_entity_id
            .clone();
        let claim_id = knowledge.call_graph.relationships[relationship_index]
            .claim_id
            .clone();
        let new_entity_id =
            callable_entity_id(&knowledge.published_snapshot.id, &placeholder.id, None);
        let entity = knowledge
            .entities
            .iter_mut()
            .find(|entity| entity.id == old_entity_id)
            .unwrap();
        entity.id = new_entity_id.clone();
        entity.name = placeholder.label.clone();
        entity.kind = "callable".into();
        entity.defined = placeholder.defined;
        entity.language = placeholder.language.clone();
        entity.source = None;
        entity.local_input_id = None;
        let manifestation = knowledge
            .manifestations
            .iter_mut()
            .find(|manifestation| manifestation.entity_id == old_entity_id)
            .unwrap();
        manifestation.entity_id = new_entity_id.clone();
        manifestation.id = manifestation_id(&manifestation.observation_context_id, &new_entity_id);
        let new_manifestation_id = manifestation.id.clone();
        let evidence_ids = {
            let claim = knowledge
                .claims
                .iter_mut()
                .find(|claim| claim.id == claim_id)
                .unwrap();
            claim.target_entity_id = new_entity_id.clone();
            claim.target_manifestation_id = new_manifestation_id;
            claim.evidence_ids.clone()
        };
        for evidence_id in evidence_ids {
            knowledge
                .evidence
                .iter_mut()
                .find(|evidence| evidence.id == evidence_id)
                .unwrap()
                .observed_callee = placeholder.id.clone();
        }
        let relationship = &mut knowledge.call_graph.relationships[relationship_index];
        relationship.callee_entity_id = new_entity_id;
        relationship.legacy_callee_id = placeholder.id.clone();
        document
            .edges
            .iter_mut()
            .find(|edge| edge.kind == "direct-call" && edge.target == "callee")
            .unwrap()
            .target = placeholder.id;

        assert_eq!(
            application.export_json(&document).unwrap_err(),
            "callable entity identity is internally inconsistent"
        );
    }

    #[test]
    fn review_fixes_keep_local_callable_identities_acquisition_scoped() {
        let application = Application;
        let mut graph = llvm::parse_llvm_ir(
            "define internal void @helper() { ret void }\ndefine void @caller_a() { call void @helper() ret void }",
            Some("a.ll"),
        )
        .unwrap();
        graph.merge(
            llvm::parse_llvm_ir(
                "define internal void @helper() { ret void }\ndefine void @caller_b() { call void @helper() ret void }",
                Some("b.ll"),
            )
            .unwrap(),
        );
        let document = Document::from_graph(&graph, analysis::summary(&graph));
        application.export_json(&document).unwrap();
        let knowledge = document.knowledge.as_ref().unwrap();
        let helpers: Vec<_> = knowledge
            .entities
            .iter()
            .filter(|entity| entity.kind == "callable" && entity.name == "helper")
            .collect();
        let helper_ids: HashSet<_> = helpers.iter().map(|entity| entity.id.as_str()).collect();
        let local_input_ids: HashSet<_> = helpers
            .iter()
            .filter_map(|entity| entity.local_input_id.as_deref())
            .collect();
        let projected_helper_ids: HashSet<_> = knowledge
            .call_graph
            .relationships
            .iter()
            .filter(|relationship| relationship.legacy_callee_id == "helper")
            .map(|relationship| relationship.callee_entity_id.as_str())
            .collect();

        assert_eq!(helpers.len(), 2);
        assert_eq!(helper_ids.len(), 2);
        assert_eq!(local_input_ids.len(), 2);
        assert_eq!(projected_helper_ids, helper_ids);
        let QueryResult::CallsToNamedCallee(named_calls) = application
            .query(
                document,
                Query::CallsToNamedCallee {
                    callee: "helper".into(),
                },
            )
            .unwrap()
        else {
            panic!("expected named call results");
        };
        assert_eq!(named_calls.len(), 2);
    }

    #[test]
    fn review_fixes_reject_broken_evidence_acquisition_chains() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let relationship = &document
            .knowledge
            .as_ref()
            .unwrap()
            .call_graph
            .relationships[0];
        let claim = document
            .knowledge
            .as_ref()
            .unwrap()
            .claims
            .iter()
            .find(|claim| claim.id == relationship.claim_id)
            .unwrap();
        let evidence_id = claim.evidence_ids[0].clone();
        let call_site_id = claim.call_site_id.clone();

        let mut wrong_input = document.clone();
        let knowledge = wrong_input.knowledge.as_mut().unwrap();
        let mut other_input = knowledge.acquired_inputs[0].clone();
        other_input.id = "input:other".into();
        other_input.content_fingerprint = "content:other".into();
        knowledge.acquired_inputs.push(other_input);
        knowledge
            .evidence
            .iter_mut()
            .find(|evidence| evidence.id == evidence_id)
            .unwrap()
            .input_id = "input:other".into();

        let mut wrong_fingerprint = document.clone();
        wrong_fingerprint
            .knowledge
            .as_mut()
            .unwrap()
            .evidence
            .iter_mut()
            .find(|evidence| evidence.id == evidence_id)
            .unwrap()
            .content_fingerprint = "content:other".into();

        let mut missing_input = document;
        missing_input
            .knowledge
            .as_mut()
            .unwrap()
            .call_sites
            .iter_mut()
            .find(|site| site.id == call_site_id)
            .unwrap()
            .input_id = "input:missing".into();

        for (invalid, expected_error) in [
            (
                wrong_input,
                "acquisition provenance identity is internally inconsistent",
            ),
            (
                wrong_fingerprint,
                "evidence provenance identity is internally inconsistent",
            ),
            (
                missing_input,
                "evidence-backed relationship is internally inconsistent",
            ),
        ] {
            let json = serialize_unchecked(&invalid);
            assert_eq!(application.load_json(&json).unwrap_err(), expected_error);
        }
    }

    #[test]
    fn rejects_duplicate_ids_at_the_document_load_boundary() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let mut invalid_documents = Vec::new();

        let mut duplicate_entity = document.clone();
        let duplicate = duplicate_entity.knowledge.as_ref().unwrap().entities[0].clone();
        let duplicate_id = duplicate.id.clone();
        duplicate_entity
            .knowledge
            .as_mut()
            .unwrap()
            .entities
            .push(duplicate);
        invalid_documents.push((duplicate_entity, duplicate_id));

        let mut duplicate_node = document.clone();
        let duplicate = duplicate_node.nodes[0].clone();
        let duplicate_id = duplicate.id.clone();
        duplicate_node.nodes.push(duplicate);
        invalid_documents.push((duplicate_node, duplicate_id));

        let mut duplicate_claim = document.clone();
        let duplicate = duplicate_claim.knowledge.as_ref().unwrap().claims[0].clone();
        let duplicate_id = duplicate.id.clone();
        duplicate_claim
            .knowledge
            .as_mut()
            .unwrap()
            .claims
            .push(duplicate);
        invalid_documents.push((duplicate_claim, duplicate_id));

        let mut duplicate_manifestation = document.clone();
        let duplicate = duplicate_manifestation
            .knowledge
            .as_ref()
            .unwrap()
            .manifestations[0]
            .clone();
        let duplicate_id = duplicate.id.clone();
        duplicate_manifestation
            .knowledge
            .as_mut()
            .unwrap()
            .manifestations
            .push(duplicate);
        invalid_documents.push((duplicate_manifestation, duplicate_id));

        let mut duplicate_context = document.clone();
        let duplicate = duplicate_context
            .knowledge
            .as_ref()
            .unwrap()
            .observation_contexts[0]
            .clone();
        let duplicate_id = duplicate.id.clone();
        duplicate_context
            .knowledge
            .as_mut()
            .unwrap()
            .observation_contexts
            .push(duplicate);
        invalid_documents.push((duplicate_context, duplicate_id));

        let mut duplicate_call_site = document.clone();
        let duplicate = duplicate_call_site.knowledge.as_ref().unwrap().call_sites[0].clone();
        let duplicate_id = duplicate.id.clone();
        duplicate_call_site
            .knowledge
            .as_mut()
            .unwrap()
            .call_sites
            .push(duplicate);
        invalid_documents.push((duplicate_call_site, duplicate_id));

        let mut duplicate_input = document.clone();
        let duplicate = duplicate_input.knowledge.as_ref().unwrap().acquired_inputs[0].clone();
        let duplicate_id = duplicate.id.clone();
        duplicate_input
            .knowledge
            .as_mut()
            .unwrap()
            .acquired_inputs
            .push(duplicate);
        invalid_documents.push((duplicate_input, duplicate_id));

        let mut duplicate_evidence = document;
        let duplicate = duplicate_evidence.knowledge.as_ref().unwrap().evidence[0].clone();
        let duplicate_id = duplicate.id.clone();
        duplicate_evidence
            .knowledge
            .as_mut()
            .unwrap()
            .evidence
            .push(duplicate);
        invalid_documents.push((duplicate_evidence, duplicate_id));

        for (invalid, duplicate_id) in invalid_documents {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                format!("duplicate document ID '{duplicate_id}'")
            );
        }
    }

    #[test]
    fn review_fixes_named_queries_share_duplicate_id_validation() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let duplicate = document.knowledge.as_ref().unwrap().entities[0].clone();
        let duplicate_id = duplicate.id.clone();
        document
            .knowledge
            .as_mut()
            .unwrap()
            .entities
            .push(duplicate);

        let error = application
            .query(
                document,
                Query::CallsToNamedCallee {
                    callee: "step".into(),
                },
            )
            .unwrap_err();

        assert_eq!(error, format!("duplicate document ID '{duplicate_id}'"));
    }

    #[test]
    fn loads_legacy_documents_without_evidence_backed_knowledge() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        document.knowledge = None;

        let json = application.export_json(&document).unwrap();
        assert!(application.load_json(&json).is_ok());
    }

    #[test]
    fn rejects_acquisition_and_snapshot_identity_tampering() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();

        let mut wrong_input_id = document.clone();
        wrong_input_id.knowledge.as_mut().unwrap().acquired_inputs[0].id = "input:tampered".into();

        let mut wrong_snapshot_id = document;
        let input = &mut wrong_snapshot_id
            .knowledge
            .as_mut()
            .unwrap()
            .acquired_inputs[0];
        input.path = "tests/fixtures/same-content-different-path.ll".into();
        input.id = acquired_input_id(&input.path, &input.content_fingerprint);

        for invalid in [wrong_input_id, wrong_snapshot_id] {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                "acquisition provenance identity is internally inconsistent"
            );
        }
    }

    #[test]
    fn rejects_unpublished_or_metadata_divergent_snapshots() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();

        let mut unpublished = document.clone();
        unpublished
            .knowledge
            .as_mut()
            .unwrap()
            .published_snapshot
            .state = "draft".into();

        let mut divergent_metadata = document;
        divergent_metadata.metadata.inputs = vec!["different.ll".into()];

        for invalid in [unpublished, divergent_metadata] {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                "acquisition provenance identity is internally inconsistent"
            );
        }
    }

    #[test]
    fn rejects_tampered_observation_context_qualifiers() {
        let application = Application;
        let document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let mut invalid_documents = Vec::new();

        let mut toolchain = document.clone();
        toolchain.knowledge.as_mut().unwrap().observation_contexts[0].toolchain = "tampered".into();
        invalid_documents.push(toolchain);

        let mut workload = document;
        workload.knowledge.as_mut().unwrap().observation_contexts[0].runtime_workload =
            Some("tampered".into());
        invalid_documents.push(workload);

        for invalid in invalid_documents {
            let json = serialize_unchecked(&invalid);
            assert_eq!(
                application.load_json(&json).unwrap_err(),
                "observation context identity is internally inconsistent"
            );
        }
    }

    #[test]
    fn explanation_uses_the_first_claim_evidence_id_deterministically() {
        let application = Application;
        let mut document = application
            .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
            .unwrap();
        let knowledge = document.knowledge.as_mut().unwrap();
        let claim = &mut knowledge.claims[0];
        let original_evidence_id = claim.evidence_ids[0].clone();
        let mut preferred_evidence = knowledge.evidence[0].clone();
        preferred_evidence.id = "evidence:preferred".into();
        knowledge.evidence.push(preferred_evidence);
        claim.evidence_ids = vec!["evidence:preferred".into(), original_evidence_id];
        let handle = format!("explain:{}", claim.id);

        let explanation = application
            .query(document, Query::Explain { handle })
            .unwrap();
        let explanation = serde_json::to_value(explanation).unwrap();

        assert_eq!(explanation["evidence"]["id"], "evidence:preferred");
    }
}
