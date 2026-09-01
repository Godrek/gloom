use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};

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
    pub fn function(id: impl Into<String>, defined: bool, source: Option<String>) -> Self {
        let id = id.into();
        Self {
            label: id.clone(),
            id,
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
    pub acquisitions: Vec<AcquisitionIdentity>,
    pub acquisition_contexts: HashSet<ExtractedAcquisitionContext>,
    pub acquisition_scoped_entities: HashSet<AcquisitionScopedEntity>,
    pub observed_manifestations: HashSet<ObservedManifestation>,
    pub call_observations: Vec<CallObservation>,
    pub observation_target: String,
    pub build_configuration: String,
    pub toolchain: String,
    pub contributor: EvidenceContributorMetadata,
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AcquisitionIdentity {
    pub path: String,
    pub content_fingerprint: String,
}

impl AcquisitionIdentity {
    pub fn new(path: impl Into<String>, content_fingerprint: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            content_fingerprint: content_fingerprint.into(),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidenceContributorMetadata {
    pub extraction_method: String,
    pub analysis_stage: String,
    pub acquired_input_kind: String,
    pub manifestation_kind: String,
    pub evidence_kind: String,
    pub claim_kind: String,
    pub derivation: String,
    pub entity_language: String,
    pub call_resolution: String,
}

impl EvidenceContributorMetadata {
    pub(crate) fn is_complete(&self) -> bool {
        [
            &self.extraction_method,
            &self.analysis_stage,
            &self.acquired_input_kind,
            &self.manifestation_kind,
            &self.evidence_kind,
            &self.claim_kind,
            &self.derivation,
            &self.entity_language,
            &self.call_resolution,
        ]
        .into_iter()
        .all(|value| !value.is_empty())
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
pub struct ExtractedObservationContext {
    pub target: String,
    pub build_configuration: String,
    pub toolchain: String,
    pub extraction_method: String,
    pub analysis_stage: String,
    pub manifestation_kind: String,
    pub evidence_kind: String,
    pub claim_kind: String,
    pub derivation: String,
    pub entity_language: String,
    pub call_resolution: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ExtractedAcquisitionContext {
    pub acquisition: AcquisitionIdentity,
    pub context: ExtractedObservationContext,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AcquisitionScopedEntity {
    pub entity: String,
    pub acquisition: AcquisitionIdentity,
}

impl ExtractedObservationContext {
    pub fn new(
        target: impl Into<String>,
        build_configuration: impl Into<String>,
        toolchain: impl Into<String>,
        contributor: &EvidenceContributorMetadata,
    ) -> Self {
        Self {
            target: target.into(),
            build_configuration: build_configuration.into(),
            toolchain: toolchain.into(),
            extraction_method: contributor.extraction_method.clone(),
            analysis_stage: contributor.analysis_stage.clone(),
            manifestation_kind: contributor.manifestation_kind.clone(),
            evidence_kind: contributor.evidence_kind.clone(),
            claim_kind: contributor.claim_kind.clone(),
            derivation: contributor.derivation.clone(),
            entity_language: contributor.entity_language.clone(),
            call_resolution: contributor.call_resolution.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CallObservation {
    pub caller: String,
    pub callee: String,
    pub acquisition: AcquisitionIdentity,
    pub context: ExtractedObservationContext,
    pub line: usize,
    pub ordinal: usize,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ObservedManifestation {
    pub entity: String,
    pub acquisition: AcquisitionIdentity,
    pub context: ExtractedObservationContext,
}

impl Graph {
    fn context_matches_contributor(&self, context: &ExtractedObservationContext) -> bool {
        context.extraction_method == self.contributor.extraction_method
            && context.analysis_stage == self.contributor.analysis_stage
            && context.manifestation_kind == self.contributor.manifestation_kind
            && context.evidence_kind == self.contributor.evidence_kind
            && context.claim_kind == self.contributor.claim_kind
            && context.derivation == self.contributor.derivation
            && context.entity_language == self.contributor.entity_language
            && context.call_resolution == self.contributor.call_resolution
    }

    fn resolved_acquisition_contexts(&self) -> HashSet<ExtractedAcquisitionContext> {
        if !self.acquisition_contexts.is_empty() {
            return self.acquisition_contexts.clone();
        }
        let mut associations: HashSet<_> = self
            .call_observations
            .iter()
            .map(|observation| ExtractedAcquisitionContext {
                acquisition: observation.acquisition.clone(),
                context: observation.context.clone(),
            })
            .chain(self.observed_manifestations.iter().map(|observation| {
                ExtractedAcquisitionContext {
                    acquisition: observation.acquisition.clone(),
                    context: observation.context.clone(),
                }
            }))
            .collect();
        let fallback_context = ExtractedObservationContext::new(
            &self.observation_target,
            &self.build_configuration,
            &self.toolchain,
            &self.contributor,
        );
        for acquisition in &self.acquisitions {
            if !associations
                .iter()
                .any(|association| association.acquisition == *acquisition)
            {
                associations.insert(ExtractedAcquisitionContext {
                    acquisition: acquisition.clone(),
                    context: fallback_context.clone(),
                });
            }
        }
        associations
    }

    fn assert_observation_acquisitions(&self, boundary: &str) {
        let acquisitions: HashSet<_> = self.acquisitions.iter().collect();
        assert!(
            self.call_observations
                .iter()
                .all(|observation| acquisitions.contains(&observation.acquisition))
                && self
                    .observed_manifestations
                    .iter()
                    .all(|observation| acquisitions.contains(&observation.acquisition)),
            "cannot {boundary} graph with observation outside its acquired inputs"
        );
        assert!(
            self.acquisition_scoped_entities.iter().all(|entity| {
                acquisitions.contains(&entity.acquisition)
                    && self
                        .nodes
                        .get(&entity.entity)
                        .is_some_and(|node| node.kind == "function")
            }),
            "cannot {boundary} graph with invalid acquisition-scoped entity"
        );
        let associations = self.resolved_acquisition_contexts();
        assert!(
            associations
                .iter()
                .all(|association| acquisitions.contains(&association.acquisition))
                && self.acquisitions.iter().all(|acquisition| {
                    associations
                        .iter()
                        .any(|association| association.acquisition == *acquisition)
                })
                && self.call_observations.iter().all(|observation| {
                    associations.contains(&ExtractedAcquisitionContext {
                        acquisition: observation.acquisition.clone(),
                        context: observation.context.clone(),
                    })
                })
                && self.observed_manifestations.iter().all(|observation| {
                    associations.contains(&ExtractedAcquisitionContext {
                        acquisition: observation.acquisition.clone(),
                        context: observation.context.clone(),
                    })
                }),
            "cannot {boundary} graph with inconsistent acquisition context ownership"
        );
        assert!(
            !self.contributor.is_complete()
                || associations
                    .iter()
                    .all(|association| self.context_matches_contributor(&association.context)),
            "cannot {boundary} graph with observations outside its contributor contract"
        );
    }

    pub fn associate_acquisition(
        &mut self,
        acquisition: AcquisitionIdentity,
        context: ExtractedObservationContext,
    ) {
        self.acquisition_contexts
            .insert(ExtractedAcquisitionContext {
                acquisition,
                context,
            });
    }

    pub fn observe_manifestation(
        &mut self,
        entity: impl Into<String>,
        acquisition: AcquisitionIdentity,
        context: ExtractedObservationContext,
    ) {
        self.observed_manifestations.insert(ObservedManifestation {
            entity: entity.into(),
            acquisition,
            context,
        });
    }

    pub fn scope_entity_to_acquisition(
        &mut self,
        entity: impl Into<String>,
        acquisition: AcquisitionIdentity,
    ) {
        self.acquisition_scoped_entities
            .insert(AcquisitionScopedEntity {
                entity: entity.into(),
                acquisition,
            });
    }

    fn entity_input_scope(
        &self,
        entity: &str,
        acquisition: &AcquisitionIdentity,
    ) -> Option<String> {
        self.acquisition_scoped_entities
            .contains(&AcquisitionScopedEntity {
                entity: entity.to_owned(),
                acquisition: acquisition.clone(),
            })
            .then(|| acquired_input_id(&acquisition.path, &acquisition.content_fingerprint))
    }

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
        self.assert_observation_acquisitions("merge");
        other.assert_observation_acquisitions("merge");
        let mut merged_acquisition_contexts = self.resolved_acquisition_contexts();
        merged_acquisition_contexts.extend(other.resolved_acquisition_contexts());
        assert!(
            other.acquisitions.len() <= 1,
            "cannot merge acquisition-backed aggregate graph; merge single acquisitions instead"
        );
        let has_incoming_acquisitions = !other.acquisitions.is_empty();
        let added_acquisition = other
            .acquisitions
            .first()
            .is_some_and(|acquisition| !self.acquisitions.contains(acquisition));
        if added_acquisition {
            self.acquisitions.extend(other.acquisitions);
        }
        for node in other.nodes.into_values() {
            self.add_node(node);
        }
        if !has_incoming_acquisitions || added_acquisition {
            for edge in other.edges.into_values() {
                let key = (edge.source.clone(), edge.target.clone(), edge.kind.clone());
                self.edges
                    .entry(key)
                    .and_modify(|old| old.call_count += edge.call_count)
                    .or_insert(edge);
            }
        }
        self.observed_manifestations
            .extend(other.observed_manifestations);
        self.acquisition_scoped_entities
            .extend(other.acquisition_scoped_entities);
        self.acquisition_contexts = merged_acquisition_contexts;
        if added_acquisition {
            self.call_observations.extend(other.call_observations);
        } else {
            let mut observations: HashSet<_> = self.call_observations.iter().cloned().collect();
            self.call_observations.extend(
                other
                    .call_observations
                    .into_iter()
                    .filter(|observation| observations.insert(observation.clone())),
            );
        }
        if !has_incoming_acquisitions || added_acquisition {
            if self.observation_target.is_empty() {
                self.observation_target = other.observation_target;
            } else if !other.observation_target.is_empty() {
                self.observation_target.push('|');
                self.observation_target.push_str(&other.observation_target);
            }
        }
        if self.build_configuration.is_empty() {
            self.build_configuration = other.build_configuration;
        } else if !other.build_configuration.is_empty()
            && self.build_configuration != other.build_configuration
        {
            self.build_configuration.push('|');
            self.build_configuration
                .push_str(&other.build_configuration);
        }
        if self.toolchain.is_empty() {
            self.toolchain = other.toolchain;
        } else if !other.toolchain.is_empty() && self.toolchain != other.toolchain {
            self.toolchain.push('|');
            self.toolchain.push_str(&other.toolchain);
        }
        if self.contributor == EvidenceContributorMetadata::default() {
            self.contributor = other.contributor;
        } else if other.contributor != EvidenceContributorMetadata::default() {
            assert_eq!(
                self.contributor, other.contributor,
                "cannot merge graphs from different evidence contributor contracts"
            );
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<Knowledge>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Knowledge {
    pub published_snapshot: PublishedSnapshot,
    #[serde(default)]
    pub contributor_id: String,
    #[serde(default)]
    pub contributor: EvidenceContributorMetadata,
    pub acquired_inputs: Vec<AcquiredInput>,
    #[serde(default)]
    pub acquisition_contexts: Vec<AcquisitionContext>,
    pub observation_contexts: Vec<ObservationContext>,
    pub entities: Vec<ProgramEntity>,
    pub manifestations: Vec<Manifestation>,
    pub call_sites: Vec<CallSite>,
    pub evidence: Vec<EvidenceRecord>,
    pub claims: Vec<TargetClaim>,
    pub call_graph: CallGraphProjection,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PublishedSnapshot {
    pub id: String,
    pub state: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcquiredInput {
    pub id: String,
    pub path: String,
    pub content_fingerprint: String,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AcquisitionContext {
    pub input_id: String,
    pub observation_context_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObservationContext {
    pub id: String,
    pub program_snapshot_id: String,
    pub target: String,
    pub build_configuration: String,
    pub toolchain: String,
    pub extraction_method: String,
    pub analysis_stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_workload: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProgramEntity {
    pub id: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub defined: bool,
    #[serde(default)]
    pub language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_input_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifestation {
    pub id: String,
    pub entity_id: String,
    pub observation_context_id: String,
    #[serde(default)]
    pub input_ids: Vec<String>,
    pub kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallSite {
    pub id: String,
    pub entity_id: String,
    pub caller_entity_id: String,
    pub caller_manifestation_id: String,
    pub observation_context_id: String,
    pub input_id: String,
    pub line: usize,
    pub ordinal: usize,
    pub resolution: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: String,
    pub kind: String,
    pub observation_context_id: String,
    pub input_id: String,
    pub content_fingerprint: String,
    pub call_site_id: String,
    pub observed_callee: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TargetClaim {
    pub id: String,
    pub kind: String,
    pub call_site_id: String,
    pub target_entity_id: String,
    pub target_manifestation_id: String,
    pub observation_context_id: String,
    pub evidence_ids: Vec<String>,
    pub derivation: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CallGraphProjection {
    pub relationships: Vec<ProjectedCall>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectedCall {
    pub caller_entity_id: String,
    pub callee_entity_id: String,
    pub claim_id: String,
    pub explanation_handle: String,
    pub legacy_caller_id: String,
    pub legacy_callee_id: String,
}

impl Document {
    pub fn from_graph(graph: &Graph, analysis: AnalysisSummary) -> Self {
        let generated_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let knowledge = Knowledge::from_extractor_output(graph);
        let nodes = graph.nodes.values().cloned().collect();
        let mut projected_edges: BTreeMap<(String, String, String), Edge> = BTreeMap::new();
        for relationship in &knowledge.call_graph.relationships {
            let key = (
                relationship.legacy_caller_id.clone(),
                relationship.legacy_callee_id.clone(),
                "direct-call".to_owned(),
            );
            projected_edges
                .entry(key)
                .and_modify(|edge| edge.call_count += 1)
                .or_insert_with(|| Edge {
                    source: relationship.legacy_caller_id.clone(),
                    target: relationship.legacy_callee_id.clone(),
                    kind: "direct-call".into(),
                    call_count: 1,
                });
        }
        for edge in graph
            .edges
            .values()
            .filter(|edge| edge.kind != "direct-call")
        {
            projected_edges.insert(
                (edge.source.clone(), edge.target.clone(), edge.kind.clone()),
                edge.clone(),
            );
        }
        Self {
            schema_version: "1.0".into(),
            metadata: Metadata {
                generated_at_unix_ms,
                inputs: graph
                    .acquisitions
                    .iter()
                    .map(|acquisition| acquisition.path.clone())
                    .collect(),
            },
            nodes,
            edges: projected_edges.into_values().collect(),
            analysis: Some(analysis),
            knowledge: Some(knowledge),
        }
    }

    pub fn into_graph(self) -> Graph {
        let acquisitions = self
            .knowledge
            .as_ref()
            .map(|knowledge| {
                knowledge
                    .acquired_inputs
                    .iter()
                    .map(|input| AcquisitionIdentity::new(&input.path, &input.content_fingerprint))
                    .collect()
            })
            .unwrap_or_default();
        let mut graph = Graph {
            acquisitions,
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

fn identity_part(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'.' | b'_' | b'-' => {
                (byte as char).to_string()
            }
            _ => format!("~{byte:02x}"),
        })
        .collect()
}

fn qualified_identity(parts: &[&str]) -> String {
    let hash = parts.iter().fold(0xcbf29ce484222325_u64, |hash, part| {
        u64::try_from(part.len())
            .expect("identity component length exceeds u64")
            .to_le_bytes()
            .into_iter()
            .chain(part.bytes())
            .fold(hash, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            })
    });
    format!("fnv1a64:{hash:016x}")
}

pub(crate) fn evidence_contributor_id(contributor: &EvidenceContributorMetadata) -> String {
    format!(
        "contributor:{}",
        qualified_identity(&[
            &contributor.extraction_method,
            &contributor.analysis_stage,
            &contributor.acquired_input_kind,
            &contributor.manifestation_kind,
            &contributor.evidence_kind,
            &contributor.claim_kind,
            &contributor.derivation,
            &contributor.entity_language,
            &contributor.call_resolution,
        ])
    )
}

pub(crate) fn callable_entity_id(
    snapshot_id: &str,
    legacy_node_id: &str,
    local_input_id: Option<&str>,
) -> String {
    let mut id = format!("entity:{snapshot_id}:{}", identity_part(legacy_node_id));
    if let Some(input_id) = local_input_id {
        id.push_str(":local:");
        id.push_str(&identity_part(input_id));
    }
    id
}

pub(crate) fn call_site_entity_id(call_site_id: &str) -> String {
    format!("entity:{call_site_id}")
}

pub(crate) fn manifestation_id(context_id: &str, entity_id: &str) -> String {
    format!("manifestation:{context_id}:{}", identity_part(entity_id))
}

pub(crate) fn call_site_id(
    snapshot_id: &str,
    path: &str,
    content_fingerprint: &str,
    line: usize,
    ordinal: usize,
) -> String {
    format!(
        "call-site:{snapshot_id}:{}:{}:{line}:{ordinal}",
        identity_part(path),
        identity_part(content_fingerprint)
    )
}

pub(crate) fn evidence_id(
    context_id: &str,
    path: &str,
    content_fingerprint: &str,
    line: usize,
    ordinal: usize,
) -> String {
    format!(
        "evidence:{context_id}:{}:{}:{line}:{ordinal}",
        identity_part(path),
        identity_part(content_fingerprint)
    )
}

pub(crate) fn target_claim_id(
    context_id: &str,
    path: &str,
    content_fingerprint: &str,
    line: usize,
    ordinal: usize,
    claim_kind: &str,
) -> String {
    format!(
        "claim:{context_id}:{}:{}:{line}:{ordinal}:{}",
        identity_part(path),
        identity_part(content_fingerprint),
        identity_part(claim_kind)
    )
}

pub(crate) fn acquired_input_id(path: &str, fingerprint: &str) -> String {
    format!(
        "input:{}:{}",
        identity_part(path),
        identity_part(fingerprint)
    )
}

pub(crate) fn published_snapshot_id(acquisitions: &[(&str, &str)]) -> String {
    let parts: Vec<_> = acquisitions
        .iter()
        .flat_map(|(path, fingerprint)| [*path, *fingerprint])
        .collect();
    format!("snapshot:{}", qualified_identity(&parts))
}

pub(crate) fn observation_context_id(
    snapshot_id: &str,
    target: &str,
    build_configuration: &str,
    toolchain: &str,
    extraction_method: &str,
    analysis_stage: &str,
    runtime_workload: Option<&str>,
) -> String {
    let (runtime_presence, runtime_workload) = runtime_workload
        .map(|workload| ("runtime:present", workload))
        .unwrap_or(("runtime:absent", ""));
    let identity = qualified_identity(&[
        snapshot_id,
        target,
        build_configuration,
        toolchain,
        extraction_method,
        analysis_stage,
        runtime_presence,
        runtime_workload,
    ]);
    format!("context:{identity}")
}

impl Knowledge {
    fn from_extractor_output(graph: &Graph) -> Self {
        graph.assert_observation_acquisitions("export");
        let extracted_acquisition_contexts = graph.resolved_acquisition_contexts();
        let acquisitions: Vec<_> = graph
            .acquisitions
            .iter()
            .map(|acquisition| {
                (
                    acquisition.path.as_str(),
                    acquisition.content_fingerprint.as_str(),
                )
            })
            .collect();
        let snapshot_id = published_snapshot_id(&acquisitions);
        let acquired_inputs: Vec<_> = graph
            .acquisitions
            .iter()
            .map(|acquisition| AcquiredInput {
                id: acquired_input_id(&acquisition.path, &acquisition.content_fingerprint),
                path: acquisition.path.clone(),
                content_fingerprint: acquisition.content_fingerprint.clone(),
                kind: graph.contributor.acquired_input_kind.clone(),
            })
            .collect();
        let projection_placeholder_ids: BTreeSet<_> = graph
            .edges
            .values()
            .filter(|edge| edge.kind == "indirect-call")
            .map(|edge| edge.target.as_str())
            .collect();
        let mut canonical_nodes = BTreeMap::new();
        let mut include_observed_entity = |entity: &str, acquisition: &AcquisitionIdentity| {
            let node = graph
                .nodes
                .get(entity)
                .unwrap_or_else(|| panic!("observed manifestation has no entity '{entity}'"));
            if node.kind == "function" && !projection_placeholder_ids.contains(node.id.as_str()) {
                canonical_nodes.insert(
                    (
                        entity.to_owned(),
                        graph.entity_input_scope(entity, acquisition),
                    ),
                    node,
                );
            }
        };
        for observation in &graph.observed_manifestations {
            include_observed_entity(&observation.entity, &observation.acquisition);
        }
        for observation in &graph.call_observations {
            include_observed_entity(&observation.caller, &observation.acquisition);
            include_observed_entity(&observation.callee, &observation.acquisition);
        }
        let represented_node_ids: BTreeSet<_> = canonical_nodes
            .keys()
            .map(|(node_id, _)| node_id.clone())
            .collect();
        for node in graph.nodes.values().filter(|node| {
            node.kind == "function" && !projection_placeholder_ids.contains(node.id.as_str())
        }) {
            if !represented_node_ids.contains(&node.id) {
                canonical_nodes.insert((node.id.clone(), None), node);
            }
        }
        let input_paths: BTreeMap<_, _> = acquired_inputs
            .iter()
            .map(|input| (input.id.as_str(), input.path.as_str()))
            .collect();
        let mut entities: Vec<_> = canonical_nodes
            .iter()
            .map(|((_, local_input_id), node)| ProgramEntity {
                id: callable_entity_id(&snapshot_id, &node.id, local_input_id.as_deref()),
                name: node.label.clone(),
                kind: "callable".into(),
                defined: node.defined,
                language: node.language.clone(),
                source: local_input_id
                    .as_deref()
                    .and_then(|input_id| input_paths.get(input_id).copied())
                    .map(str::to_owned)
                    .or_else(|| node.source.clone()),
                local_input_id: local_input_id.clone(),
            })
            .collect();
        let entity_ids: BTreeMap<_, _> = canonical_nodes
            .keys()
            .cloned()
            .zip(entities.iter().map(|entity| entity.id.clone()))
            .collect();
        let mut observation_contexts = BTreeMap::new();
        let mut acquisition_contexts = BTreeSet::new();
        for association in &extracted_acquisition_contexts {
            let context = &association.context;
            let id = observation_context_id(
                &snapshot_id,
                &context.target,
                &context.build_configuration,
                &context.toolchain,
                &context.extraction_method,
                &context.analysis_stage,
                None,
            );
            acquisition_contexts.insert((
                acquired_input_id(
                    &association.acquisition.path,
                    &association.acquisition.content_fingerprint,
                ),
                id.clone(),
            ));
            observation_contexts.insert(
                id.clone(),
                ObservationContext {
                    id,
                    program_snapshot_id: snapshot_id.clone(),
                    target: context.target.clone(),
                    build_configuration: context.build_configuration.clone(),
                    toolchain: context.toolchain.clone(),
                    extraction_method: context.extraction_method.clone(),
                    analysis_stage: context.analysis_stage.clone(),
                    runtime_workload: None,
                },
            );
        }
        if observation_contexts.is_empty() {
            let context = ExtractedObservationContext::new(
                &graph.observation_target,
                &graph.build_configuration,
                &graph.toolchain,
                &graph.contributor,
            );
            let id = observation_context_id(
                &snapshot_id,
                &context.target,
                &context.build_configuration,
                &context.toolchain,
                &context.extraction_method,
                &context.analysis_stage,
                None,
            );
            observation_contexts.insert(
                id.clone(),
                ObservationContext {
                    id,
                    program_snapshot_id: snapshot_id.clone(),
                    target: context.target,
                    build_configuration: context.build_configuration,
                    toolchain: context.toolchain,
                    extraction_method: context.extraction_method,
                    analysis_stage: context.analysis_stage,
                    runtime_workload: None,
                },
            );
        }
        let mut manifestation_specs: BTreeMap<_, (String, BTreeSet<String>)> = BTreeMap::new();
        let mut add_manifestation =
            |entity: &str,
             acquisition: &AcquisitionIdentity,
             context: &ExtractedObservationContext| {
                let entity_key = (
                    entity.to_owned(),
                    graph.entity_input_scope(entity, acquisition),
                );
                let entity_id = entity_ids
                    .get(&entity_key)
                    .unwrap_or_else(|| panic!("observed manifestation has no entity '{entity}'"));
                let context_id = observation_context_id(
                    &snapshot_id,
                    &context.target,
                    &context.build_configuration,
                    &context.toolchain,
                    &context.extraction_method,
                    &context.analysis_stage,
                    None,
                );
                let input_id =
                    acquired_input_id(&acquisition.path, &acquisition.content_fingerprint);
                assert!(
                    acquisition_contexts.contains(&(input_id.clone(), context_id.clone())),
                    "observed manifestation has no acquisition context association"
                );
                let specification = manifestation_specs
                    .entry((context_id, (*entity_id).to_owned()))
                    .or_insert_with(|| (context.manifestation_kind.clone(), BTreeSet::new()));
                assert_eq!(
                    specification.0, context.manifestation_kind,
                    "conflicting manifestation kinds for one entity and context"
                );
                specification.1.insert(input_id);
            };
        for observation in &graph.observed_manifestations {
            add_manifestation(
                &observation.entity,
                &observation.acquisition,
                &observation.context,
            );
        }
        for observation in &graph.call_observations {
            add_manifestation(
                &observation.caller,
                &observation.acquisition,
                &observation.context,
            );
            add_manifestation(
                &observation.callee,
                &observation.acquisition,
                &observation.context,
            );
        }
        if manifestation_specs.is_empty() {
            let context = observation_contexts
                .values()
                .next()
                .expect("exported knowledge must have an observation context");
            let input_ids: BTreeSet<_> = acquisition_contexts
                .iter()
                .filter(|(_, context_id)| context_id == &context.id)
                .map(|(input_id, _)| input_id.clone())
                .collect();
            for entity_id in entity_ids.values() {
                manifestation_specs.insert(
                    (context.id.clone(), (*entity_id).to_owned()),
                    (
                        graph.contributor.manifestation_kind.clone(),
                        input_ids.clone(),
                    ),
                );
            }
        }
        let manifestations: Vec<_> = manifestation_specs
            .into_iter()
            .map(
                |((observation_context_id, entity_id), (kind, input_ids))| Manifestation {
                    id: manifestation_id(&observation_context_id, &entity_id),
                    entity_id,
                    observation_context_id,
                    input_ids: input_ids.into_iter().collect(),
                    kind,
                },
            )
            .collect();
        let manifestation_ids: BTreeMap<_, _> = manifestations
            .iter()
            .map(|item| {
                (
                    (
                        item.observation_context_id.as_str(),
                        item.entity_id.as_str(),
                    ),
                    item.id.as_str(),
                )
            })
            .collect();
        let mut call_sites = Vec::new();
        let mut evidence = Vec::new();
        let mut claims = Vec::new();
        let mut relationships = Vec::new();
        for observation in &graph.call_observations {
            let context = &observation.context;
            let context_id = observation_context_id(
                &snapshot_id,
                &context.target,
                &context.build_configuration,
                &context.toolchain,
                &context.extraction_method,
                &context.analysis_stage,
                None,
            );
            let call_site_id = call_site_id(
                &snapshot_id,
                &observation.acquisition.path,
                &observation.acquisition.content_fingerprint,
                observation.line,
                observation.ordinal,
            );
            let call_site_entity_id = call_site_entity_id(&call_site_id);
            let evidence_id = evidence_id(
                &context_id,
                &observation.acquisition.path,
                &observation.acquisition.content_fingerprint,
                observation.line,
                observation.ordinal,
            );
            let claim_id = target_claim_id(
                &context_id,
                &observation.acquisition.path,
                &observation.acquisition.content_fingerprint,
                observation.line,
                observation.ordinal,
                &context.claim_kind,
            );
            let input_id = acquired_input_id(
                &observation.acquisition.path,
                &observation.acquisition.content_fingerprint,
            );
            let caller_entity_id = entity_ids[&(
                observation.caller.clone(),
                graph.entity_input_scope(&observation.caller, &observation.acquisition),
            )]
                .to_owned();
            let callee_entity_id = entity_ids[&(
                observation.callee.clone(),
                graph.entity_input_scope(&observation.callee, &observation.acquisition),
            )]
                .to_owned();
            call_sites.push(CallSite {
                id: call_site_id.clone(),
                entity_id: call_site_entity_id.clone(),
                caller_entity_id: caller_entity_id.clone(),
                caller_manifestation_id: manifestation_ids
                    [&(context_id.as_str(), caller_entity_id.as_str())]
                    .to_owned(),
                observation_context_id: context_id.clone(),
                input_id: input_id.clone(),
                line: observation.line,
                ordinal: observation.ordinal,
                resolution: context.call_resolution.clone(),
            });
            entities.push(ProgramEntity {
                id: call_site_entity_id,
                name: format!("{}:{}", observation.acquisition.path, observation.line),
                kind: "call-site".into(),
                defined: true,
                language: context.entity_language.clone(),
                source: Some(observation.acquisition.path.clone()),
                local_input_id: None,
            });
            evidence.push(EvidenceRecord {
                id: evidence_id.clone(),
                kind: context.evidence_kind.clone(),
                observation_context_id: context_id.clone(),
                input_id,
                content_fingerprint: observation.acquisition.content_fingerprint.clone(),
                call_site_id: call_site_id.clone(),
                observed_callee: observation.callee.clone(),
            });
            claims.push(TargetClaim {
                id: claim_id.clone(),
                kind: context.claim_kind.clone(),
                call_site_id,
                target_entity_id: callee_entity_id.clone(),
                target_manifestation_id: manifestation_ids
                    [&(context_id.as_str(), callee_entity_id.as_str())]
                    .to_owned(),
                observation_context_id: context_id.clone(),
                evidence_ids: vec![evidence_id],
                derivation: context.derivation.clone(),
            });
            relationships.push(ProjectedCall {
                caller_entity_id,
                callee_entity_id,
                claim_id: claim_id.clone(),
                explanation_handle: format!("explain:{claim_id}"),
                legacy_caller_id: observation.caller.clone(),
                legacy_callee_id: observation.callee.clone(),
            });
        }
        Self {
            published_snapshot: PublishedSnapshot {
                id: snapshot_id.clone(),
                state: "published".into(),
            },
            contributor_id: evidence_contributor_id(&graph.contributor),
            contributor: graph.contributor.clone(),
            acquired_inputs,
            acquisition_contexts: acquisition_contexts
                .into_iter()
                .map(|(input_id, observation_context_id)| AcquisitionContext {
                    input_id,
                    observation_context_id,
                })
                .collect(),
            observation_contexts: observation_contexts.into_values().collect(),
            entities,
            manifestations,
            call_sites,
            evidence,
            claims,
            call_graph: CallGraphProjection { relationships },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis;

    fn acquisition_pair(path: &str, content_fingerprint: &str) -> Vec<AcquisitionIdentity> {
        vec![AcquisitionIdentity::new(path, content_fingerprint)]
    }

    #[test]
    fn definition_wins_and_document_round_trips() {
        let mut graph = Graph::default();
        graph.add_node(Node::function("f", false, None));
        graph.add_node(Node::function("f", true, Some("f.c".into())));
        assert!(graph.nodes["f"].defined);

        let document = Document::from_graph(&graph, analysis::summary(&graph));
        let encoded = serde_json::to_string(&document).unwrap();
        let decoded: Document = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.into_graph().nodes, graph.nodes);
    }

    #[test]
    fn published_snapshot_identity_changes_with_acquired_content() {
        let document_for = |fingerprint: &str| {
            let graph = Graph {
                acquisitions: acquisition_pair("fixture.ll", fingerprint),
                ..Graph::default()
            };
            Document::from_graph(&graph, analysis::summary(&graph))
                .knowledge
                .unwrap()
                .published_snapshot
                .id
        };

        assert_ne!(document_for("content:a"), document_for("content:b"));
    }

    #[test]
    fn published_snapshot_identity_changes_with_acquisition_path() {
        let document_for = |path: &str| {
            let graph = Graph {
                acquisitions: acquisition_pair(path, "content:same"),
                ..Graph::default()
            };
            Document::from_graph(&graph, analysis::summary(&graph))
                .knowledge
                .unwrap()
                .published_snapshot
                .id
        };

        assert_ne!(document_for("a.ll"), document_for("b.ll"));
    }

    #[test]
    fn review_fixes_use_fixed_width_identity_component_lengths() {
        let expected_hash = 1_u64
            .to_le_bytes()
            .into_iter()
            .chain(*b"a")
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });

        assert_eq!(
            qualified_identity(&["a"]),
            format!("fnv1a64:{expected_hash:016x}")
        );
    }

    #[test]
    fn exports_evidence_contributor_semantics_without_core_reinterpretation() {
        let mut graph = Graph {
            acquisitions: acquisition_pair("fixture.custom", "content:custom"),
            observation_target: "custom-target".into(),
            contributor: EvidenceContributorMetadata {
                extraction_method: "custom-contributor-v2".into(),
                analysis_stage: "custom-stage".into(),
                acquired_input_kind: "custom-input".into(),
                manifestation_kind: "custom-manifestation".into(),
                evidence_kind: "custom-evidence".into(),
                claim_kind: "custom-claim".into(),
                derivation: "custom derivation".into(),
                entity_language: "custom-language".into(),
                call_resolution: "partial".into(),
            },
            call_observations: vec![CallObservation {
                caller: "caller".into(),
                callee: "callee".into(),
                acquisition: AcquisitionIdentity::new("fixture.custom", "content:custom"),
                context: ExtractedObservationContext {
                    target: "custom-target".into(),
                    extraction_method: "custom-contributor-v2".into(),
                    analysis_stage: "custom-stage".into(),
                    manifestation_kind: "custom-manifestation".into(),
                    evidence_kind: "custom-evidence".into(),
                    claim_kind: "custom-claim".into(),
                    derivation: "custom derivation".into(),
                    entity_language: "custom-language".into(),
                    call_resolution: "partial".into(),
                    ..ExtractedObservationContext::default()
                },
                line: 1,
                ordinal: 1,
            }],
            ..Graph::default()
        };
        graph.add_node(Node {
            id: "caller".into(),
            label: "caller".into(),
            kind: "function".into(),
            defined: true,
            language: "custom-language".into(),
            source: Some("fixture.custom".into()),
        });
        graph.add_node(Node {
            id: "callee".into(),
            label: "callee".into(),
            kind: "function".into(),
            defined: false,
            language: "custom-language".into(),
            source: None,
        });

        let knowledge = Document::from_graph(&graph, analysis::summary(&graph))
            .knowledge
            .unwrap();

        assert_eq!(knowledge.acquired_inputs[0].kind, "custom-input");
        assert_eq!(
            knowledge.observation_contexts[0].extraction_method,
            "custom-contributor-v2"
        );
        assert_eq!(
            knowledge.observation_contexts[0].analysis_stage,
            "custom-stage"
        );
        assert_eq!(knowledge.manifestations[0].kind, "custom-manifestation");
        assert_eq!(knowledge.call_sites[0].resolution, "partial");
        assert_eq!(knowledge.evidence[0].kind, "custom-evidence");
        assert_eq!(knowledge.claims[0].kind, "custom-claim");
        assert_eq!(knowledge.claims[0].derivation, "custom derivation");
        assert_eq!(
            knowledge
                .entities
                .iter()
                .find(|entity| entity.kind == "call-site")
                .unwrap()
                .language,
            "custom-language"
        );
    }

    #[test]
    fn observation_identity_changes_with_qualified_context() {
        let context_for = |toolchain: &str| {
            let graph = Graph {
                acquisitions: acquisition_pair("fixture.ll", "content:a"),
                observation_target: "fixture".into(),
                build_configuration: "debug".into(),
                toolchain: toolchain.into(),
                ..Graph::default()
            };
            Document::from_graph(&graph, analysis::summary(&graph))
                .knowledge
                .unwrap()
                .observation_contexts
                .remove(0)
                .id
        };

        assert_ne!(context_for("clang-19"), context_for("clang-20"));
    }

    #[test]
    fn review_fixes_merge_changed_content_with_distinct_provenance() {
        let mut graph = Graph {
            acquisitions: acquisition_pair("fixture.ll", "content:first"),
            call_observations: vec![CallObservation {
                caller: "caller".into(),
                callee: "callee".into(),
                acquisition: AcquisitionIdentity::new("fixture.ll", "content:first"),
                context: ExtractedObservationContext::default(),
                line: 1,
                ordinal: 1,
            }],
            ..Graph::default()
        };
        graph.add_node(Node::function("caller", true, Some("fixture.ll".into())));
        graph.add_node(Node::function("callee", false, None));
        graph.merge(Graph {
            acquisitions: acquisition_pair("fixture.ll", "content:duplicate"),
            call_observations: vec![CallObservation {
                caller: "caller".into(),
                callee: "callee".into(),
                acquisition: AcquisitionIdentity::new("fixture.ll", "content:duplicate"),
                context: ExtractedObservationContext::default(),
                line: 1,
                ordinal: 1,
            }],
            ..Graph::default()
        });

        let knowledge = Document::from_graph(&graph, analysis::summary(&graph))
            .knowledge
            .unwrap();
        assert_eq!(
            graph.acquisitions,
            [
                AcquisitionIdentity::new("fixture.ll", "content:first"),
                AcquisitionIdentity::new("fixture.ll", "content:duplicate")
            ]
        );
        assert_eq!(knowledge.acquired_inputs.len(), 2);
        assert_ne!(
            knowledge.acquired_inputs[0].id,
            knowledge.acquired_inputs[1].id
        );
        assert_eq!(knowledge.evidence.len(), 2);
        assert_ne!(knowledge.call_sites[0].id, knowledge.call_sites[1].id);
        assert_ne!(knowledge.evidence[0].id, knowledge.evidence[1].id);
        assert_ne!(knowledge.claims[0].id, knowledge.claims[1].id);
        assert_ne!(
            knowledge.call_graph.relationships[0].explanation_handle,
            knowledge.call_graph.relationships[1].explanation_handle
        );
        assert_eq!(knowledge.evidence[0].content_fingerprint, "content:first");
        assert_eq!(
            knowledge.evidence[1].content_fingerprint,
            "content:duplicate"
        );
        assert_eq!(
            knowledge.evidence[0].input_id,
            knowledge.acquired_inputs[0].id
        );
        assert_eq!(
            knowledge.evidence[1].input_id,
            knowledge.acquired_inputs[1].id
        );
    }

    #[test]
    fn merged_observations_keep_separate_fully_qualified_contexts() {
        let graph_for = |path: &str, fingerprint: &str, target: &str, toolchain: &str| {
            let context = ExtractedObservationContext {
                target: target.into(),
                build_configuration: format!("build for {target}"),
                toolchain: toolchain.into(),
                extraction_method: "gloom-llvm-text-v1".into(),
                analysis_stage: "textual-ir".into(),
                manifestation_kind: "llvm-function".into(),
                evidence_kind: "llvm-direct-call".into(),
                claim_kind: "direct-target".into(),
                derivation: "direct LLVM callee operand".into(),
                entity_language: "llvm".into(),
                call_resolution: "complete".into(),
            };
            let mut graph = Graph {
                acquisitions: acquisition_pair(path, fingerprint),
                call_observations: vec![CallObservation {
                    caller: "caller".into(),
                    callee: "callee".into(),
                    acquisition: AcquisitionIdentity::new(path, fingerprint),
                    context,
                    line: 1,
                    ordinal: 1,
                }],
                ..Graph::default()
            };
            graph.add_node(Node::function("caller", true, Some(path.into())));
            graph.add_node(Node::function("callee", false, None));
            graph.add_edge("caller", "callee", "direct-call");
            graph
        };
        let mut graph = graph_for("a.ll", "content:a", "a.c", "clang-a");
        graph.merge(graph_for("b.ll", "content:b", "b.ll", "clang-b"));

        let knowledge = Document::from_graph(&graph, analysis::summary(&graph))
            .knowledge
            .unwrap();
        assert_eq!(knowledge.observation_contexts.len(), 2);
        for site in &knowledge.call_sites {
            let input = knowledge
                .acquired_inputs
                .iter()
                .find(|input| input.id == site.input_id)
                .unwrap();
            let context = knowledge
                .observation_contexts
                .iter()
                .find(|context| context.id == site.observation_context_id)
                .unwrap();
            match input.path.as_str() {
                "a.ll" => assert_eq!(
                    (context.target.as_str(), context.toolchain.as_str()),
                    ("a.c", "clang-a")
                ),
                "b.ll" => assert_eq!(
                    (context.target.as_str(), context.toolchain.as_str()),
                    ("b.ll", "clang-b")
                ),
                path => panic!("unexpected acquisition {path}"),
            }
        }
    }

    #[test]
    fn review_fixes_emit_only_context_supported_manifestations() {
        let graph_for = |path: &str, fingerprint: &str, caller: &str, callee: &str| {
            let acquisition = AcquisitionIdentity::new(path, fingerprint);
            let context = ExtractedObservationContext {
                target: path.into(),
                build_configuration: format!("build for {path}"),
                toolchain: format!("toolchain for {path}"),
                extraction_method: "gloom-llvm-text-v1".into(),
                analysis_stage: "textual-ir".into(),
                manifestation_kind: "llvm-function".into(),
                evidence_kind: "llvm-direct-call".into(),
                claim_kind: "direct-target".into(),
                derivation: "direct LLVM callee operand".into(),
                entity_language: "llvm".into(),
                call_resolution: "complete".into(),
            };
            let mut graph = Graph {
                acquisitions: vec![acquisition.clone()],
                call_observations: vec![CallObservation {
                    caller: caller.into(),
                    callee: callee.into(),
                    acquisition,
                    context,
                    line: 1,
                    ordinal: 1,
                }],
                ..Graph::default()
            };
            graph.add_node(Node::function(caller, true, Some(path.into())));
            graph.add_node(Node::function(callee, false, None));
            graph
        };
        let mut graph = graph_for("a.ll", "content:a", "a", "x");
        graph.merge(graph_for("b.ll", "content:b", "b", "y"));

        let knowledge = Document::from_graph(&graph, analysis::summary(&graph))
            .knowledge
            .unwrap();
        let contexts: BTreeMap<_, _> = knowledge
            .observation_contexts
            .iter()
            .map(|context| (context.id.as_str(), context.target.as_str()))
            .collect();
        let entities: BTreeMap<_, _> = knowledge
            .entities
            .iter()
            .map(|entity| (entity.id.as_str(), entity.name.as_str()))
            .collect();
        let actual: std::collections::BTreeSet<_> = knowledge
            .manifestations
            .iter()
            .map(|manifestation| {
                (
                    contexts[manifestation.observation_context_id.as_str()],
                    entities[manifestation.entity_id.as_str()],
                )
            })
            .collect();

        assert_eq!(
            actual,
            [("a.ll", "a"), ("a.ll", "x"), ("b.ll", "b"), ("b.ll", "y")]
                .into_iter()
                .collect()
        );
    }

    #[test]
    fn review_fixes_deduplicate_identical_acquisition_observations() {
        let observation = CallObservation {
            caller: "caller".into(),
            callee: "callee".into(),
            acquisition: AcquisitionIdentity::new("fixture.ll", "content:same"),
            context: ExtractedObservationContext::default(),
            line: 1,
            ordinal: 1,
        };
        let acquisition = || Graph {
            acquisitions: acquisition_pair("fixture.ll", "content:same"),
            call_observations: vec![observation.clone()],
            observation_target: "fixture.ll".into(),
            ..Graph::default()
        };
        let mut graph = acquisition();
        graph.add_node(Node::function("caller", true, Some("fixture.ll".into())));
        graph.add_node(Node::function("callee", false, None));
        let single_knowledge = Document::from_graph(&graph, analysis::summary(&graph))
            .knowledge
            .unwrap();
        graph.merge(acquisition());

        let knowledge = Document::from_graph(&graph, analysis::summary(&graph))
            .knowledge
            .unwrap();
        assert_eq!(
            graph.acquisitions,
            [AcquisitionIdentity::new("fixture.ll", "content:same")]
        );
        assert_eq!(graph.observation_target, "fixture.ll");
        assert_eq!(graph.call_observations, [observation]);
        assert_eq!(knowledge.call_sites.len(), 1);
        assert_eq!(knowledge.evidence.len(), 1);
        assert_eq!(knowledge.claims.len(), 1);
        assert_eq!(knowledge.call_graph.relationships.len(), 1);
        assert_eq!(
            knowledge.observation_contexts[0].id,
            single_knowledge.observation_contexts[0].id
        );
        assert_eq!(knowledge.evidence[0].id, single_knowledge.evidence[0].id);
        assert_eq!(knowledge.claims[0].id, single_knowledge.claims[0].id);
        assert_eq!(
            knowledge.call_graph.relationships[0].explanation_handle,
            single_knowledge.call_graph.relationships[0].explanation_handle
        );
    }

    #[test]
    fn edge_merging_tracks_distinct_acquisitions() {
        let acquisition = |fingerprint: &str| {
            let mut graph = Graph {
                acquisitions: acquisition_pair("fixture.ll", fingerprint),
                ..Graph::default()
            };
            graph.add_edge("caller", "<indirect>", "indirect-call");
            graph
        };
        let edge_count = |graph: &Graph| {
            graph
                .edges
                .get(&("caller".into(), "<indirect>".into(), "indirect-call".into()))
                .unwrap()
                .call_count
        };

        let mut identical = acquisition("content:same");
        identical.merge(acquisition("content:same"));
        assert_eq!(edge_count(&identical), 1);

        let mut distinct = acquisition("content:first");
        distinct.merge(acquisition("content:second"));
        assert_eq!(edge_count(&distinct), 2);

        let mut acquisition_free = Graph::default();
        acquisition_free.add_edge("caller", "<indirect>", "indirect-call");
        let mut other_acquisition_free = Graph::default();
        other_acquisition_free.add_edge("caller", "<indirect>", "indirect-call");
        acquisition_free.merge(other_acquisition_free);
        assert_eq!(edge_count(&acquisition_free), 2);
    }

    #[test]
    #[should_panic(
        expected = "cannot merge acquisition-backed aggregate graph; merge single acquisitions instead"
    )]
    fn merging_acquisition_backed_aggregates_fails_before_mixing_provenance() {
        Graph::default().merge(Graph {
            acquisitions: vec![
                AcquisitionIdentity::new("first.ll", "content:first"),
                AcquisitionIdentity::new("second.ll", "content:second"),
            ],
            observation_target: "first.ll|second.ll".into(),
            ..Graph::default()
        });
    }

    #[test]
    fn review_fixes_reject_merge_observations_from_undeclared_acquisitions() {
        let mut graph = Graph::default();
        graph.add_node(Node::function("existing", true, None));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            graph.merge(Graph {
                acquisitions: acquisition_pair("fixture.ll", "content:declared"),
                call_observations: vec![CallObservation {
                    caller: "caller".into(),
                    callee: "callee".into(),
                    acquisition: AcquisitionIdentity::new("other.ll", "content:other"),
                    context: ExtractedObservationContext::default(),
                    line: 1,
                    ordinal: 1,
                }],
                ..Graph::default()
            });
        }));

        assert!(result.is_err());
        assert_eq!(graph.nodes.len(), 1);
        assert!(graph.acquisitions.is_empty());
    }

    #[test]
    #[should_panic(expected = "cannot export graph with observation outside its acquired inputs")]
    fn review_fixes_reject_export_observations_from_undeclared_acquisitions() {
        let mut graph = Graph {
            acquisitions: acquisition_pair("fixture.ll", "content:declared"),
            call_observations: vec![CallObservation {
                caller: "caller".into(),
                callee: "callee".into(),
                acquisition: AcquisitionIdentity::new("other.ll", "content:other"),
                context: ExtractedObservationContext::default(),
                line: 1,
                ordinal: 1,
            }],
            ..Graph::default()
        };
        graph.add_node(Node::function("caller", true, None));
        graph.add_node(Node::function("callee", false, None));

        Document::from_graph(&graph, analysis::summary(&graph));
    }

    #[test]
    #[should_panic(
        expected = "cannot export graph with inconsistent acquisition context ownership"
    )]
    fn review_fixes_reject_cross_acquisition_context_tampering_at_export() {
        let mut graph = crate::llvm::parse_llvm_ir(
            "define void @caller() {\n  call void @callee()\n  ret void\n}",
            Some("observed.ll"),
        )
        .unwrap();
        graph.merge(crate::llvm::parse_llvm_ir("", Some("empty.ll")).unwrap());
        let other_acquisition = graph
            .acquisitions
            .iter()
            .find(|acquisition| acquisition.path == "empty.ll")
            .unwrap()
            .clone();
        graph.call_observations[0].acquisition = other_acquisition;

        Document::from_graph(&graph, analysis::summary(&graph));
    }

    #[test]
    fn legacy_edges_keep_nodes_with_distinct_display_labels() {
        let mut graph = Graph::default();
        graph.add_node(Node::function("caller", true, None));
        graph.add_node(Node {
            id: "<indirect>".into(),
            label: "indirect call".into(),
            kind: "function".into(),
            defined: false,
            language: "llvm".into(),
            source: None,
        });
        graph.add_edge("caller", "<indirect>", "indirect-call");

        let document = Document::from_graph(&graph, analysis::summary(&graph));

        assert!(
            document
                .nodes
                .iter()
                .any(|node| { node.id == "<indirect>" && node.label == "indirect call" })
        );
        assert!(document.edges.iter().any(|edge| {
            edge.source == "caller" && edge.target == "<indirect>" && edge.kind == "indirect-call"
        }));
    }

    #[test]
    fn duplicate_display_labels_still_produce_distinct_manifestations() {
        let mut graph = Graph {
            acquisitions: acquisition_pair("fixture.ll", "content:fixture"),
            ..Graph::default()
        };
        for id in ["first", "second"] {
            graph.add_node(Node {
                id: id.into(),
                label: "shared display label".into(),
                kind: "function".into(),
                defined: true,
                language: "llvm".into(),
                source: Some("fixture.ll".into()),
            });
        }

        let knowledge = Document::from_graph(&graph, analysis::summary(&graph))
            .knowledge
            .unwrap();
        let manifestation_ids: std::collections::BTreeSet<_> = knowledge
            .manifestations
            .iter()
            .map(|manifestation| manifestation.id.as_str())
            .collect();

        assert_eq!(knowledge.manifestations.len(), 2);
        assert_eq!(manifestation_ids.len(), 2);
    }
}
