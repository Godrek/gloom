use crate::contributor::{ContributorIdentity, EvidenceContribution, fingerprint_parts};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

pub const SNAPSHOT_SCHEMA_VERSION: &str = "2.0-pre";

macro_rules! identity_type {
    ($name:ident) => {
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

identity_type!(ProgramSnapshotId);
identity_type!(ObservationContextId);
identity_type!(AcquiredInputId);
identity_type!(ProgramEntityId);
identity_type!(ManifestationId);
identity_type!(EvidenceId);
identity_type!(TargetClaimId);

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProgramSnapshot {
    pub id: ProgramSnapshotId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ObservationContext {
    pub id: ObservationContextId,
    pub program_snapshot_id: ProgramSnapshotId,
    pub build_target: String,
    pub build_configuration: String,
    pub toolchain: String,
    pub extraction_method: String,
    pub extraction_version: String,
    pub analysis_stage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_workload: Option<String>,
}

impl ObservationContext {
    #[allow(clippy::too_many_arguments)]
    pub fn static_analysis(
        program_snapshot_id: impl Into<String>,
        build_target: impl Into<String>,
        build_configuration: impl Into<String>,
        toolchain: impl Into<String>,
        extraction_method: impl Into<String>,
        extraction_version: impl Into<String>,
        analysis_stage: impl Into<String>,
    ) -> Self {
        let program_snapshot_id = ProgramSnapshotId::new(program_snapshot_id);
        let mut context = Self {
            id: ObservationContextId::new(""),
            program_snapshot_id,
            build_target: build_target.into(),
            build_configuration: build_configuration.into(),
            toolchain: toolchain.into(),
            extraction_method: extraction_method.into(),
            extraction_version: extraction_version.into(),
            analysis_stage: analysis_stage.into(),
            runtime_workload: None,
        };
        context.id = context.qualified_id();
        context
    }

    fn validate(&self) -> Result<(), String> {
        for (field, value) in [
            ("id", self.id.as_str()),
            ("program snapshot", self.program_snapshot_id.as_str()),
            ("build target", self.build_target.as_str()),
            ("build configuration", self.build_configuration.as_str()),
            ("toolchain", self.toolchain.as_str()),
            ("extraction method", self.extraction_method.as_str()),
            ("extraction version", self.extraction_version.as_str()),
            ("analysis stage", self.analysis_stage.as_str()),
        ] {
            if value.trim().is_empty() {
                return Err(format!("observation context {field} cannot be empty"));
            }
        }
        if self
            .runtime_workload
            .as_deref()
            .is_some_and(|workload| workload.trim().is_empty())
        {
            return Err("observation context runtime workload cannot be empty".into());
        }
        if self.id != self.qualified_id() {
            return Err(format!(
                "observation context '{}' is not qualified by all of its fields",
                self.id
            ));
        }
        Ok(())
    }

    fn qualified_id(&self) -> ObservationContextId {
        let (workload_kind, workload) = self
            .runtime_workload
            .as_deref()
            .map_or(("no-runtime-workload", ""), |workload| {
                ("runtime-workload", workload)
            });
        let fingerprint = fingerprint_parts(&[
            self.program_snapshot_id.as_str(),
            &self.build_target,
            &self.build_configuration,
            &self.toolchain,
            &self.extraction_method,
            &self.extraction_version,
            &self.analysis_stage,
            workload_kind,
            workload,
        ]);
        ObservationContextId::new(format!(
            "context:{}:{fingerprint}",
            self.program_snapshot_id
        ))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AcquiredInput {
    pub id: AcquiredInputId,
    pub path: String,
    pub evidence_artifact: String,
    pub media_type: String,
    pub acquisition_method: String,
    pub content_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SourceLocation {
    pub input_id: AcquiredInputId,
    pub artifact: String,
    pub line: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProgramEntityKind {
    Callable,
    CallSite,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProgramEntity {
    pub id: ProgramEntityId,
    pub display_name: String,
    pub kind: ProgramEntityKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub caller_entity_id: Option<ProgramEntityId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_location: Option<SourceLocation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Manifestation {
    pub id: ManifestationId,
    pub entity_id: ProgramEntityId,
    pub observation_context_id: ObservationContextId,
    pub representation: String,
    pub defined: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: EvidenceId,
    pub acquired_input_id: AcquiredInputId,
    pub observation_context_id: ObservationContextId,
    pub evidence_type: String,
    pub subject_entity_id: ProgramEntityId,
    pub related_manifestation_ids: Vec<ManifestationId>,
    pub source_location: SourceLocation,
    pub description: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Resolution {
    Complete,
    Partial,
    Absent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TargetClaim {
    pub id: TargetClaimId,
    pub call_site_id: ProgramEntityId,
    pub target_manifestation_id: ManifestationId,
    pub observation_context_id: ObservationContextId,
    pub resolution: Resolution,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Derivation {
    pub rule: String,
    pub input_evidence_ids: Vec<EvidenceId>,
    pub output_claim_id: TargetClaimId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExplanationHandle(String);

impl ExplanationHandle {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallRelationship {
    pub caller_entity_id: ProgramEntityId,
    pub caller_display_name: String,
    pub callee_entity_id: ProgramEntityId,
    pub callee_display_name: String,
    pub call_site_id: ProgramEntityId,
    pub observation_context_id: ObservationContextId,
    pub resolution: Resolution,
    pub explanation_handle: ExplanationHandle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallGraphProjection {
    pub name: String,
    pub program_snapshot_id: ProgramSnapshotId,
    pub relationships: Vec<CallRelationship>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Explanation {
    pub handle: ExplanationHandle,
    pub target_claim: TargetClaim,
    pub evidence_records: Vec<EvidenceRecord>,
    pub derivation: Derivation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NamedQueryResult {
    pub query_name: String,
    pub program_snapshot_id: ProgramSnapshotId,
    pub relationships: Vec<CallRelationship>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PublishedSnapshot {
    schema_version: String,
    program_snapshot: ProgramSnapshot,
    acquired_inputs: Vec<AcquiredInput>,
    observation_contexts: Vec<ObservationContext>,
    program_entities: Vec<ProgramEntity>,
    manifestations: Vec<Manifestation>,
    evidence_records: Vec<EvidenceRecord>,
    target_claims: Vec<TargetClaim>,
    derivations: Vec<Derivation>,
    call_graph_projection: CallGraphProjection,
}

impl PublishedSnapshot {
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    pub fn program_snapshot(&self) -> &ProgramSnapshot {
        &self.program_snapshot
    }

    pub fn acquired_inputs(&self) -> &[AcquiredInput] {
        &self.acquired_inputs
    }

    pub fn observation_contexts(&self) -> &[ObservationContext] {
        &self.observation_contexts
    }

    pub fn program_entities(&self) -> &[ProgramEntity] {
        &self.program_entities
    }

    pub fn manifestations(&self) -> &[Manifestation] {
        &self.manifestations
    }

    pub fn evidence_records(&self) -> &[EvidenceRecord] {
        &self.evidence_records
    }

    pub fn target_claims(&self) -> &[TargetClaim] {
        &self.target_claims
    }

    pub fn call_graph_projection(&self) -> &CallGraphProjection {
        &self.call_graph_projection
    }

    pub(crate) fn query_callees(&self, caller_name: &str) -> Result<NamedQueryResult, String> {
        let caller_exists = self.program_entities.iter().any(|entity| {
            entity.kind == ProgramEntityKind::Callable && entity.display_name == caller_name
        });
        if !caller_exists {
            return Err(format!("unknown callable '{caller_name}'"));
        }
        Ok(NamedQueryResult {
            query_name: "callees".into(),
            program_snapshot_id: self.program_snapshot.id.clone(),
            relationships: self
                .call_graph_projection
                .relationships
                .iter()
                .filter(|relationship| relationship.caller_display_name == caller_name)
                .cloned()
                .collect(),
        })
    }

    pub(crate) fn explain(&self, handle: &ExplanationHandle) -> Result<Explanation, String> {
        let claim = self
            .target_claims
            .iter()
            .find(|claim| explanation_handle(&claim.id) == *handle)
            .ok_or_else(|| format!("unknown explanation handle '{}'", handle.as_str()))?;
        let derivation = self
            .derivations
            .iter()
            .find(|derivation| derivation.output_claim_id == claim.id)
            .ok_or_else(|| format!("claim '{}' has no derivation", claim.id))?;
        let evidence_by_id: BTreeMap<_, _> = self
            .evidence_records
            .iter()
            .map(|evidence| (evidence.id.as_str(), evidence))
            .collect();
        let evidence_records = claim
            .evidence_ids
            .iter()
            .map(|id| {
                evidence_by_id
                    .get(id.as_str())
                    .copied()
                    .cloned()
                    .ok_or_else(|| {
                        format!("claim '{}' references unknown evidence '{id}'", claim.id)
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Explanation {
            handle: handle.clone(),
            target_claim: claim.clone(),
            evidence_records,
            derivation: derivation.clone(),
        })
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.schema_version != SNAPSHOT_SCHEMA_VERSION {
            return Err(format!(
                "unsupported snapshot schema {:?}",
                self.schema_version
            ));
        }
        if self.program_snapshot.id.as_str().trim().is_empty() {
            return Err("program snapshot identity cannot be empty".into());
        }
        for context in &self.observation_contexts {
            context.validate()?;
            if context.program_snapshot_id != self.program_snapshot.id {
                return Err(format!(
                    "observation context '{}' belongs to another program snapshot",
                    context.id
                ));
            }
        }
        let inputs_by_id: BTreeMap<_, _> = self
            .acquired_inputs
            .iter()
            .map(|input| (input.id.as_str(), input))
            .collect();
        if inputs_by_id.len() != self.acquired_inputs.len() {
            return Err("published snapshot contains duplicate acquired-input identities".into());
        }
        let entities_by_id: BTreeMap<_, _> = self
            .program_entities
            .iter()
            .map(|entity| (entity.id.as_str(), entity))
            .collect();
        if entities_by_id.len() != self.program_entities.len() {
            return Err("published snapshot contains duplicate program-entity identities".into());
        }
        let manifestations_by_id: BTreeMap<_, _> = self
            .manifestations
            .iter()
            .map(|manifestation| (manifestation.id.as_str(), manifestation))
            .collect();
        if manifestations_by_id.len() != self.manifestations.len() {
            return Err("published snapshot contains duplicate manifestation identities".into());
        }
        let context_ids: BTreeSet<_> = self
            .observation_contexts
            .iter()
            .map(|context| context.id.as_str())
            .collect();
        for manifestation in &self.manifestations {
            if !entities_by_id.contains_key(manifestation.entity_id.as_str()) {
                return Err(format!(
                    "manifestation '{}' references unknown entity '{}'",
                    manifestation.id, manifestation.entity_id
                ));
            }
            if !context_ids.contains(manifestation.observation_context_id.as_str()) {
                return Err(format!(
                    "manifestation '{}' references unknown observation context '{}'",
                    manifestation.id, manifestation.observation_context_id
                ));
            }
        }
        let evidence_by_id: BTreeMap<_, _> = self
            .evidence_records
            .iter()
            .map(|evidence| (evidence.id.as_str(), evidence))
            .collect();
        if evidence_by_id.len() != self.evidence_records.len() {
            return Err("published snapshot contains duplicate evidence identities".into());
        }
        for evidence in &self.evidence_records {
            let acquired_input = inputs_by_id
                .get(evidence.acquired_input_id.as_str())
                .ok_or_else(|| {
                    format!(
                        "evidence '{}' references unknown acquired input '{}'",
                        evidence.id, evidence.acquired_input_id
                    )
                })?;
            if !context_ids.contains(evidence.observation_context_id.as_str()) {
                return Err(format!(
                    "evidence '{}' references unknown observation context '{}'",
                    evidence.id, evidence.observation_context_id
                ));
            }
            let subject = entities_by_id
                .get(evidence.subject_entity_id.as_str())
                .ok_or_else(|| {
                    format!(
                        "evidence '{}' references unknown subject entity '{}'",
                        evidence.id, evidence.subject_entity_id
                    )
                })?;
            if subject.kind != ProgramEntityKind::CallSite {
                return Err(format!(
                    "direct-call evidence '{}' does not identify a call site",
                    evidence.id
                ));
            }
            if evidence.source_location.input_id != evidence.acquired_input_id {
                return Err(format!(
                    "evidence '{}' has a source location in another acquired input",
                    evidence.id
                ));
            }
            if evidence.source_location.artifact != acquired_input.evidence_artifact {
                return Err(format!(
                    "evidence '{}' location does not identify its acquired evidence artifact",
                    evidence.id
                ));
            }
            for manifestation_id in &evidence.related_manifestation_ids {
                if !manifestations_by_id.contains_key(manifestation_id.as_str()) {
                    return Err(format!(
                        "evidence '{}' references unknown manifestation '{}'",
                        evidence.id, manifestation_id
                    ));
                }
            }
        }
        let derivations_by_claim: BTreeMap<_, _> = self
            .derivations
            .iter()
            .map(|derivation| (derivation.output_claim_id.as_str(), derivation))
            .collect();
        if derivations_by_claim.len() != self.derivations.len() {
            return Err("published snapshot contains duplicate claim derivations".into());
        }
        let claim_ids: BTreeSet<_> = self
            .target_claims
            .iter()
            .map(|claim| claim.id.as_str())
            .collect();
        if claim_ids.len() != self.target_claims.len() {
            return Err("published snapshot contains duplicate target-claim identities".into());
        }
        for claim in &self.target_claims {
            let call_site = entities_by_id
                .get(claim.call_site_id.as_str())
                .ok_or_else(|| {
                    format!(
                        "claim '{}' references unknown call site '{}'",
                        claim.id, claim.call_site_id
                    )
                })?;
            if call_site.kind != ProgramEntityKind::CallSite {
                return Err(format!(
                    "claim '{}' subject '{}' is not a call site",
                    claim.id, claim.call_site_id
                ));
            }
            let target = manifestations_by_id
                .get(claim.target_manifestation_id.as_str())
                .ok_or_else(|| {
                    format!(
                        "claim '{}' references unknown manifestation '{}'",
                        claim.id, claim.target_manifestation_id
                    )
                })?;
            if target.observation_context_id != claim.observation_context_id {
                return Err(format!(
                    "claim '{}' and its target manifestation use different observation contexts",
                    claim.id
                ));
            }
            if !context_ids.contains(claim.observation_context_id.as_str()) {
                return Err(format!(
                    "claim '{}' references unknown observation context '{}'",
                    claim.id, claim.observation_context_id
                ));
            }
            if claim.evidence_ids.is_empty() {
                return Err(format!("claim '{}' has no supporting evidence", claim.id));
            }
            for evidence_id in &claim.evidence_ids {
                let evidence = evidence_by_id.get(evidence_id.as_str()).ok_or_else(|| {
                    format!(
                        "claim '{}' references unknown evidence '{evidence_id}'",
                        claim.id
                    )
                })?;
                if evidence.observation_context_id != claim.observation_context_id
                    || evidence.subject_entity_id != claim.call_site_id
                {
                    return Err(format!(
                        "claim '{}' and its evidence have incompatible subjects or contexts",
                        claim.id
                    ));
                }
                if !evidence
                    .related_manifestation_ids
                    .contains(&claim.target_manifestation_id)
                {
                    return Err(format!(
                        "claim '{}' target is not supported by evidence '{}'",
                        claim.id, evidence.id
                    ));
                }
            }
            let derivation = derivations_by_claim
                .get(claim.id.as_str())
                .ok_or_else(|| format!("claim '{}' has no derivation", claim.id))?;
            if derivation.input_evidence_ids != claim.evidence_ids {
                return Err(format!(
                    "claim '{}' and its derivation reference different evidence",
                    claim.id
                ));
            }
        }
        if self.call_graph_projection.program_snapshot_id != self.program_snapshot.id {
            return Err("call-graph projection belongs to another program snapshot".into());
        }
        for relationship in &self.call_graph_projection.relationships {
            let explanation = self.explain(&relationship.explanation_handle)?;
            let claim = &explanation.target_claim;
            let call_site = entities_by_id
                .get(claim.call_site_id.as_str())
                .expect("validated target claim call site must exist");
            let caller = entities_by_id.get(relationship.caller_entity_id.as_str());
            let callee = entities_by_id.get(relationship.callee_entity_id.as_str());
            let target = manifestations_by_id
                .get(claim.target_manifestation_id.as_str())
                .expect("validated target manifestation must exist");
            let matches_claim = claim.call_site_id == relationship.call_site_id
                && claim.observation_context_id == relationship.observation_context_id
                && claim.resolution == relationship.resolution
                && call_site.caller_entity_id.as_ref() == Some(&relationship.caller_entity_id)
                && target.entity_id == relationship.callee_entity_id
                && caller.is_some_and(|entity| {
                    entity.kind == ProgramEntityKind::Callable
                        && entity.display_name == relationship.caller_display_name
                })
                && callee.is_some_and(|entity| {
                    entity.kind == ProgramEntityKind::Callable
                        && entity.display_name == relationship.callee_display_name
                });
            if !matches_claim {
                return Err(format!(
                    "call relationship at '{}' does not match its target claim",
                    relationship.call_site_id
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct CallableIdentity {
    entity_id: ProgramEntityId,
    manifestation_id: ManifestationId,
}

#[allow(clippy::too_many_arguments)]
fn ensure_callable(
    name: &str,
    representation: &str,
    snapshot_id: &ProgramSnapshotId,
    input_index: usize,
    context_id: &ObservationContextId,
    identities: &mut BTreeMap<String, CallableIdentity>,
    entities: &mut Vec<ProgramEntity>,
    manifestations: &mut Vec<Manifestation>,
) -> CallableIdentity {
    if let Some(identity) = identities.get(name) {
        return identity.clone();
    }
    let callable_index = identities.len();
    let identity = CallableIdentity {
        entity_id: ProgramEntityId::new(format!(
            "entity:{snapshot_id}:input:{input_index}:callable:{callable_index}"
        )),
        manifestation_id: ManifestationId::new(format!(
            "manifestation:{snapshot_id}:input:{input_index}:callable:{callable_index}"
        )),
    };
    entities.push(ProgramEntity {
        id: identity.entity_id.clone(),
        display_name: name.into(),
        kind: ProgramEntityKind::Callable,
        caller_entity_id: None,
        source_location: None,
    });
    manifestations.push(Manifestation {
        id: identity.manifestation_id.clone(),
        entity_id: identity.entity_id.clone(),
        observation_context_id: context_id.clone(),
        representation: representation.into(),
        defined: false,
    });
    identities.insert(name.into(), identity.clone());
    identity
}

fn explanation_handle(claim_id: &TargetClaimId) -> ExplanationHandle {
    ExplanationHandle(format!("explanation:{claim_id}"))
}

pub(crate) fn publish(
    contributions: Vec<EvidenceContribution>,
    contributor: ContributorIdentity,
    context: ObservationContext,
) -> Result<PublishedSnapshot, String> {
    contributor.validate()?;
    context.validate()?;
    if contributor.name != context.extraction_method
        || contributor.version != context.extraction_version
    {
        return Err(format!(
            "evidence contributor '{}@{}' does not match observation context '{}@{}'",
            contributor.name,
            contributor.version,
            context.extraction_method,
            context.extraction_version
        ));
    }
    if contributions.is_empty() {
        return Err("at least one acquired input is required".into());
    }
    let snapshot_id = context.program_snapshot_id.clone();
    let context_id = context.id.clone();
    let mut acquired_inputs = Vec::new();
    let mut program_entities = Vec::new();
    let mut manifestations = Vec::new();
    let mut evidence_records = Vec::new();
    let mut target_claims = Vec::new();
    let mut derivations = Vec::new();
    let mut relationships = Vec::new();

    for (input_index, contribution) in contributions.into_iter().enumerate() {
        contribution.validate()?;
        let input_id = AcquiredInputId::new(format!("input:{snapshot_id}:{input_index}"));
        let path_text = contribution.input.path.clone();
        let evidence_artifact = contribution.input.evidence_artifact.clone();
        acquired_inputs.push(AcquiredInput {
            id: input_id.clone(),
            path: path_text.clone(),
            evidence_artifact: evidence_artifact.clone(),
            media_type: contribution.input.media_type,
            acquisition_method: contribution.input.acquisition_method,
            content_fingerprint: contribution.input.content_fingerprint,
        });

        let mut identities = BTreeMap::new();
        for function in contribution.callables {
            let identity = ensure_callable(
                &function.display_name,
                &function.representation,
                &snapshot_id,
                input_index,
                &context_id,
                &mut identities,
                &mut program_entities,
                &mut manifestations,
            );
            if function.defined {
                manifestations
                    .iter_mut()
                    .find(|manifestation| manifestation.id == identity.manifestation_id)
                    .expect("new callable manifestation must exist")
                    .defined = true;
            }
        }

        for (direct_call_index, call) in contribution.direct_calls.into_iter().enumerate() {
            let caller = identities
                .get(&call.caller_display_name)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "direct-call evidence references uncontributed caller '{}'",
                        call.caller_display_name
                    )
                })?;
            let callee = ensure_callable(
                &call.callee_display_name,
                &call.target_representation,
                &snapshot_id,
                input_index,
                &context_id,
                &mut identities,
                &mut program_entities,
                &mut manifestations,
            );
            let call_site_id = ProgramEntityId::new(format!(
                "entity:{snapshot_id}:input:{input_index}:call-site:{direct_call_index}"
            ));
            let location = SourceLocation {
                input_id: input_id.clone(),
                artifact: evidence_artifact.clone(),
                line: call.line,
            };
            program_entities.push(ProgramEntity {
                id: call_site_id.clone(),
                display_name: format!(
                    "{} call at {}:{}",
                    call.caller_display_name, evidence_artifact, call.line
                ),
                kind: ProgramEntityKind::CallSite,
                caller_entity_id: Some(caller.entity_id.clone()),
                source_location: Some(location.clone()),
            });
            let evidence_id = EvidenceId::new(format!(
                "evidence:{snapshot_id}:input:{input_index}:direct-call:{direct_call_index}"
            ));
            evidence_records.push(EvidenceRecord {
                id: evidence_id.clone(),
                acquired_input_id: input_id.clone(),
                observation_context_id: context_id.clone(),
                evidence_type: call.evidence_type,
                subject_entity_id: call_site_id.clone(),
                related_manifestation_ids: vec![
                    caller.manifestation_id.clone(),
                    callee.manifestation_id.clone(),
                ],
                source_location: location,
                description: format!(
                    "acquired direct call instruction names '{}' as its target",
                    call.callee_display_name
                ),
            });
            let claim_id = TargetClaimId::new(format!(
                "claim:{snapshot_id}:input:{input_index}:direct-target:{direct_call_index}"
            ));
            let claim = TargetClaim {
                id: claim_id.clone(),
                call_site_id: call_site_id.clone(),
                target_manifestation_id: callee.manifestation_id.clone(),
                observation_context_id: context_id.clone(),
                resolution: Resolution::Complete,
                evidence_ids: vec![evidence_id.clone()],
            };
            derivations.push(Derivation {
                rule: "direct-call-target-from-acquired-instruction".into(),
                input_evidence_ids: vec![evidence_id],
                output_claim_id: claim_id.clone(),
            });
            relationships.push(CallRelationship {
                caller_entity_id: caller.entity_id,
                caller_display_name: call.caller_display_name,
                callee_entity_id: callee.entity_id,
                callee_display_name: call.callee_display_name,
                call_site_id,
                observation_context_id: context_id.clone(),
                resolution: Resolution::Complete,
                explanation_handle: explanation_handle(&claim_id),
            });
            target_claims.push(claim);
        }
    }

    let snapshot = PublishedSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION.into(),
        program_snapshot: ProgramSnapshot {
            id: snapshot_id.clone(),
        },
        acquired_inputs,
        observation_contexts: vec![context],
        program_entities,
        manifestations,
        evidence_records,
        target_claims,
        derivations,
        call_graph_projection: CallGraphProjection {
            name: "call-graph".into(),
            program_snapshot_id: snapshot_id,
            relationships,
        },
    };
    snapshot.validate()?;
    Ok(snapshot)
}
