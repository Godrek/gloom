use crate::contributor::{
    ContributedCallSite, ContributedCallSiteAttachment, ContributedEvidence, ContributorIdentity,
    EvidenceContribution, fingerprint_parts,
};
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
identity_type!(CorrespondenceClaimId);

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
        Self::new(
            program_snapshot_id,
            build_target,
            build_configuration,
            toolchain,
            extraction_method,
            extraction_version,
            analysis_stage,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn runtime_analysis(
        program_snapshot_id: impl Into<String>,
        build_target: impl Into<String>,
        build_configuration: impl Into<String>,
        toolchain: impl Into<String>,
        extraction_method: impl Into<String>,
        extraction_version: impl Into<String>,
        analysis_stage: impl Into<String>,
        runtime_workload: impl Into<String>,
    ) -> Self {
        Self::new(
            program_snapshot_id,
            build_target,
            build_configuration,
            toolchain,
            extraction_method,
            extraction_version,
            analysis_stage,
            Some(runtime_workload.into()),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new(
        program_snapshot_id: impl Into<String>,
        build_target: impl Into<String>,
        build_configuration: impl Into<String>,
        toolchain: impl Into<String>,
        extraction_method: impl Into<String>,
        extraction_version: impl Into<String>,
        analysis_stage: impl Into<String>,
        runtime_workload: Option<String>,
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
            runtime_workload,
        };
        context.id = context.qualified_id();
        context
    }

    pub(crate) fn validate(&self) -> Result<(), String> {
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
    pub acquired_input_id: AcquiredInputId,
    pub contributor_callable_id: String,
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
    pub scope: EvidenceScope,
    pub support: EvidenceSupport,
    pub subject_entity_id: ProgramEntityId,
    pub related_manifestation_ids: Vec<ManifestationId>,
    pub source_location: SourceLocation,
    pub description: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceScope {
    Static,
    Runtime,
}

impl EvidenceScope {
    pub fn matches(self, context: &ObservationContext) -> bool {
        match self {
            Self::Static => context.runtime_workload.is_none(),
            Self::Runtime => context.runtime_workload.is_some(),
        }
    }
}

/// What an evidence record supports.
///
/// The subject entity of a record follows from its support: resolution and
/// target evidence describe a call site, so their subject is that call site,
/// while contributor-identity evidence describes one callable manifestation,
/// so its subject is the callable entity that manifestation represents and its
/// related manifestation is that manifestation.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceSupport {
    CallSiteResolution,
    TargetClaim,
    ContributorIdentity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Resolution {
    Complete,
    Partial,
    Absent,
}

impl Resolution {
    pub fn target_set_incomplete(self) -> bool {
        !matches!(self, Self::Complete)
    }

    pub fn accepts_target_count(self, target_count: usize) -> bool {
        match self {
            Self::Absent => target_count == 0,
            Self::Partial | Self::Complete => target_count > 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallSiteResolution {
    pub call_site_id: ProgramEntityId,
    pub observation_context_id: ObservationContextId,
    pub resolution: Resolution,
    pub evidence_ids: Vec<EvidenceId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct TargetClaim {
    pub id: TargetClaimId,
    pub call_site_id: ProgramEntityId,
    pub target_manifestation_id: ManifestationId,
    pub observation_context_id: ObservationContextId,
    pub evidence_ids: Vec<EvidenceId>,
}

/// The only target-claim derivation rule the core currently produces: a
/// target claim is derived directly from the contributed evidence that names
/// the target, without any core-side inference.
pub const CONTRIBUTED_EVIDENCE_TARGET_RULE: &str = "call-target-from-contributed-evidence";

/// The only correspondence rule the core currently derives. A contributor
/// asserts one contributor callable identity for a callable in each of its
/// observation contexts; when that identity appears in more than one context
/// of the same acquired input, the manifestations correspond. Display names
/// never participate, and the claim cites only the contributor-identity
/// evidence for each of its manifestations, never resolution or target
/// evidence, so the derivation can be recomputed. A manifestation without
/// identity evidence takes no part in the claim, and the claim is not derived
/// at all when fewer than two manifestations remain.
pub const CONTRIBUTOR_IDENTITY_CORRESPONDENCE_RULE: &str =
    "correspondence-from-contributor-callable-identity";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CorrespondenceClaim {
    pub id: CorrespondenceClaimId,
    pub rule: String,
    pub acquired_input_id: AcquiredInputId,
    pub contributor_callable_id: String,
    pub manifestation_ids: Vec<ManifestationId>,
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
    pub target_claim_id: TargetClaimId,
    pub caller_entity_id: ProgramEntityId,
    pub caller_display_name: String,
    pub callee_entity_id: ProgramEntityId,
    pub callee_display_name: String,
    pub call_site_id: ProgramEntityId,
    pub target_observation_context_id: ObservationContextId,
    pub resolution_observation_context_id: ObservationContextId,
    pub resolution: Resolution,
    pub correspondence_claim_ids: Vec<CorrespondenceClaimId>,
    pub explanation_handle: ExplanationHandle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectedCallTarget {
    pub target_claim_id: TargetClaimId,
    pub callee_entity_id: ProgramEntityId,
    pub callee_display_name: String,
    pub target_observation_context_id: ObservationContextId,
    pub correspondence_claim_ids: Vec<CorrespondenceClaimId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProjectedCallSite {
    pub caller_entity_id: ProgramEntityId,
    pub caller_display_name: String,
    pub call_site_id: ProgramEntityId,
    pub call_site_display_name: String,
    pub resolution_observation_context_id: ObservationContextId,
    pub resolution: Resolution,
    pub targets: Vec<ProjectedCallTarget>,
    pub explanation_handle: ExplanationHandle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallGraphProjection {
    pub name: String,
    pub program_snapshot_id: ProgramSnapshotId,
    pub relationships: Vec<CallRelationship>,
    pub call_sites: Vec<ProjectedCallSite>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Explanation {
    pub handle: ExplanationHandle,
    pub call_site_resolution: CallSiteResolution,
    pub target_claims: Vec<TargetClaim>,
    pub correspondence_claims: Vec<CorrespondenceClaim>,
    pub evidence_records: Vec<EvidenceRecord>,
    pub derivations: Vec<Derivation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NamedQueryResult {
    pub query_name: String,
    pub program_snapshot_id: ProgramSnapshotId,
    pub caller_entity_id: ProgramEntityId,
    pub caller_observation_context_id: ObservationContextId,
    pub correspondence_claims: Vec<CorrespondenceClaim>,
    pub relationships: Vec<CallRelationship>,
    pub call_sites: Vec<ProjectedCallSite>,
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
    call_site_resolutions: Vec<CallSiteResolution>,
    target_claims: Vec<TargetClaim>,
    correspondence_claims: Vec<CorrespondenceClaim>,
    derivations: Vec<Derivation>,
    call_graph_projection: CallGraphProjection,
}

/// The contributor-identity evidence a snapshot publishes for one contributor
/// callable identity within one acquired input.
#[derive(Default)]
struct IdentityEvidenceGroup<'a> {
    manifestation_ids: BTreeSet<&'a str>,
    evidence_ids: BTreeSet<&'a str>,
    observation_context_ids: BTreeSet<&'a str>,
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

    pub fn call_site_resolutions(&self) -> &[CallSiteResolution] {
        &self.call_site_resolutions
    }

    pub fn target_claims(&self) -> &[TargetClaim] {
        &self.target_claims
    }

    pub fn correspondence_claims(&self) -> &[CorrespondenceClaim] {
        &self.correspondence_claims
    }

    pub fn call_graph_projection(&self) -> &CallGraphProjection {
        &self.call_graph_projection
    }

    pub(crate) fn query_callees(
        &self,
        caller_name: &str,
        caller_entity_id: Option<&ProgramEntityId>,
    ) -> Result<NamedQueryResult, String> {
        let candidates = self
            .program_entities
            .iter()
            .filter(|entity| {
                entity.kind == ProgramEntityKind::Callable && entity.display_name == caller_name
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err(format!("unknown callable '{caller_name}'"));
        }
        let caller = if let Some(caller_entity_id) = caller_entity_id {
            candidates
                .iter()
                .copied()
                .find(|candidate| candidate.id == *caller_entity_id)
                .ok_or_else(|| {
                    format!(
                        "callable entity '{caller_entity_id}' does not identify callable '{caller_name}'"
                    )
                })?
        } else if candidates.len() == 1 {
            candidates[0]
        } else {
            let candidate_ids = candidates
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "callable '{caller_name}' is ambiguous; select a caller entity ID from: {candidate_ids}"
            ));
        };
        let caller_observation_context_id = self
            .manifestations
            .iter()
            .find(|manifestation| manifestation.entity_id == caller.id)
            .expect("validated callable must have a manifestation")
            .observation_context_id
            .clone();
        let relationships = self
            .call_graph_projection
            .relationships
            .iter()
            .filter(|relationship| relationship.caller_entity_id == caller.id)
            .cloned()
            .collect::<Vec<_>>();
        let call_sites = self
            .call_graph_projection
            .call_sites
            .iter()
            .filter(|call_site| call_site.caller_entity_id == caller.id)
            .cloned()
            .collect::<Vec<_>>();
        let correspondence_ids = call_sites
            .iter()
            .flat_map(|site| &site.targets)
            .flat_map(|target| &target.correspondence_claim_ids)
            .map(CorrespondenceClaimId::as_str)
            .collect::<BTreeSet<_>>();
        Ok(NamedQueryResult {
            query_name: "callees".into(),
            program_snapshot_id: self.program_snapshot.id.clone(),
            caller_entity_id: caller.id.clone(),
            caller_observation_context_id,
            correspondence_claims: self
                .correspondence_claims
                .iter()
                .filter(|claim| correspondence_ids.contains(claim.id.as_str()))
                .cloned()
                .collect(),
            relationships,
            call_sites,
        })
    }

    pub(crate) fn explain(&self, handle: &ExplanationHandle) -> Result<Explanation, String> {
        let call_site_resolution = self
            .call_site_resolutions
            .iter()
            .find(|resolution| call_site_explanation_handle(&resolution.call_site_id) == *handle)
            .ok_or_else(|| format!("unknown explanation handle '{}'", handle.as_str()))?;
        let target_claims = self
            .target_claims
            .iter()
            .filter(|claim| claim.call_site_id == call_site_resolution.call_site_id)
            .cloned()
            .collect::<Vec<_>>();
        let claim_ids = target_claims
            .iter()
            .map(|claim| claim.id.as_str())
            .collect::<BTreeSet<_>>();
        let target_manifestation_ids = target_claims
            .iter()
            .map(|claim| claim.target_manifestation_id.as_str())
            .collect::<BTreeSet<_>>();
        let correspondence_claims = self
            .correspondence_claims
            .iter()
            .filter(|claim| {
                claim
                    .manifestation_ids
                    .iter()
                    .any(|id| target_manifestation_ids.contains(id.as_str()))
            })
            .cloned()
            .collect::<Vec<_>>();
        let derivations = self
            .derivations
            .iter()
            .filter(|derivation| claim_ids.contains(derivation.output_claim_id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let evidence_ids = call_site_resolution
            .evidence_ids
            .iter()
            .chain(target_claims.iter().flat_map(|claim| &claim.evidence_ids))
            .chain(
                correspondence_claims
                    .iter()
                    .flat_map(|claim| &claim.evidence_ids),
            )
            .map(EvidenceId::as_str)
            .collect::<BTreeSet<_>>();
        let evidence_records = self
            .evidence_records
            .iter()
            .filter(|evidence| evidence_ids.contains(evidence.id.as_str()))
            .cloned()
            .collect();
        Ok(Explanation {
            handle: handle.clone(),
            call_site_resolution: call_site_resolution.clone(),
            target_claims,
            correspondence_claims,
            evidence_records,
            derivations,
        })
    }

    /// Groups this snapshot's contributor-identity evidence by the acquired
    /// input and contributor callable identity it asserts, rejecting a
    /// manifestation asserted more than once.
    ///
    /// Validation recomputes correspondence claims from these groups, so a
    /// hand-edited claim can neither cite identity evidence for another
    /// callable, nor drop a manifestation whose identity evidence remains, nor
    /// disappear while its group still spans observation contexts.
    fn identity_evidence_groups(
        &self,
    ) -> Result<BTreeMap<(&str, &str), IdentityEvidenceGroup<'_>>, String> {
        let manifestations_by_id: BTreeMap<_, _> = self
            .manifestations
            .iter()
            .map(|manifestation| (manifestation.id.as_str(), manifestation))
            .collect();
        let mut groups: BTreeMap<(&str, &str), IdentityEvidenceGroup<'_>> = BTreeMap::new();
        let mut asserted_manifestations = BTreeSet::new();
        for evidence in self
            .evidence_records
            .iter()
            .filter(|evidence| evidence.support == EvidenceSupport::ContributorIdentity)
        {
            let manifestation_id = evidence
                .related_manifestation_ids
                .first()
                .expect("validated contributor-identity evidence relates one manifestation");
            if !asserted_manifestations.insert(manifestation_id.as_str()) {
                return Err(format!(
                    "manifestation '{manifestation_id}' has more than one contributor-identity evidence record"
                ));
            }
            let manifestation = manifestations_by_id
                .get(manifestation_id.as_str())
                .expect("validated contributor-identity evidence manifestation must exist");
            let group = groups
                .entry((
                    manifestation.acquired_input_id.as_str(),
                    manifestation.contributor_callable_id.as_str(),
                ))
                .or_default();
            group.manifestation_ids.insert(manifestation_id.as_str());
            group.evidence_ids.insert(evidence.id.as_str());
            group
                .observation_context_ids
                .insert(manifestation.observation_context_id.as_str());
        }
        Ok(groups)
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
        // An acquired input is identified by its program snapshot and its
        // position in this snapshot's acquisition order, and nothing else. An
        // identity of any other shape is rejected outright, so a hand-edited
        // export cannot introduce an input the core never published and
        // re-attribute evidence to it.
        let mut input_indexes = BTreeMap::new();
        for (index, input) in self.acquired_inputs.iter().enumerate() {
            for (field, value) in [
                ("path", input.path.as_str()),
                ("evidence artifact", input.evidence_artifact.as_str()),
                ("media type", input.media_type.as_str()),
                ("acquisition method", input.acquisition_method.as_str()),
                ("content fingerprint", input.content_fingerprint.as_str()),
            ] {
                if value.trim().is_empty() {
                    return Err(format!("acquired input {field} cannot be empty"));
                }
            }
            let qualified = acquired_input_id(&self.program_snapshot.id, index);
            if input.id != qualified {
                return Err(format!(
                    "acquired input '{}' is not identified as '{qualified}', the input this program snapshot acquired at position {index}",
                    input.id
                ));
            }
            input_indexes.insert(input.id.as_str(), index);
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
        let contexts_by_id: BTreeMap<_, _> = self
            .observation_contexts
            .iter()
            .map(|context| (context.id.as_str(), context))
            .collect();
        if context_ids.len() != self.observation_contexts.len() {
            return Err(
                "published snapshot contains duplicate observation-context identities".into(),
            );
        }
        let mut entity_contexts = BTreeMap::new();
        let mut contributor_manifestations = BTreeSet::new();
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
            if manifestation.contributor_callable_id.trim().is_empty() {
                return Err(format!(
                    "manifestation '{}' has an empty contributor callable identity",
                    manifestation.id
                ));
            }
            if !inputs_by_id.contains_key(manifestation.acquired_input_id.as_str()) {
                return Err(format!(
                    "manifestation '{}' references unknown acquired input '{}'",
                    manifestation.id, manifestation.acquired_input_id
                ));
            }
            if !identity_belongs_to_acquired_input(
                manifestation.id.as_str(),
                "manifestation",
                &self.program_snapshot.id,
                &input_indexes,
                &manifestation.acquired_input_id,
            ) {
                return Err(format!(
                    "manifestation '{}' was not published from acquired input '{}'",
                    manifestation.id, manifestation.acquired_input_id
                ));
            }
            if !contributor_manifestations.insert((
                manifestation.acquired_input_id.as_str(),
                manifestation.observation_context_id.as_str(),
                manifestation.contributor_callable_id.as_str(),
            )) {
                return Err(format!(
                    "contributor callable identity '{}' has duplicate manifestations in observation context '{}'",
                    manifestation.contributor_callable_id, manifestation.observation_context_id
                ));
            }
            match entity_contexts.insert(
                manifestation.entity_id.as_str(),
                manifestation.observation_context_id.as_str(),
            ) {
                Some(existing) if existing != manifestation.observation_context_id.as_str() => {
                    return Err(format!(
                        "program entity '{}' is merged across observation contexts without correspondence evidence",
                        manifestation.entity_id
                    ));
                }
                Some(_) => {
                    return Err(format!(
                        "program entity '{}' merges distinct contributor callable identities in observation context '{}'",
                        manifestation.entity_id, manifestation.observation_context_id
                    ));
                }
                None => {}
            }
        }
        for callable in self
            .program_entities
            .iter()
            .filter(|entity| entity.kind == ProgramEntityKind::Callable)
        {
            if !entity_contexts.contains_key(callable.id.as_str()) {
                return Err(format!(
                    "callable entity '{}' has no context-specific manifestation",
                    callable.id
                ));
            }
        }
        for entity in &self.program_entities {
            match (entity.kind, entity.source_location.as_ref()) {
                (ProgramEntityKind::Callable, None) => {}
                (ProgramEntityKind::Callable, Some(_)) => {
                    return Err(format!(
                        "callable entity '{}' carries a source location its evidence does not preserve",
                        entity.id
                    ));
                }
                (ProgramEntityKind::CallSite, None) => {
                    return Err(format!("call site '{}' has no source location", entity.id));
                }
                (ProgramEntityKind::CallSite, Some(location)) => {
                    let acquired_input =
                        inputs_by_id
                            .get(location.input_id.as_str())
                            .ok_or_else(|| {
                                format!(
                                    "call site '{}' is located in unknown acquired input '{}'",
                                    entity.id, location.input_id
                                )
                            })?;
                    if location.artifact != acquired_input.evidence_artifact {
                        return Err(format!(
                            "call site '{}' location does not identify its acquired evidence artifact",
                            entity.id
                        ));
                    }
                    if location.line == 0 {
                        return Err(format!(
                            "call site '{}' has no location within its evidence artifact",
                            entity.id
                        ));
                    }
                    if !identity_belongs_to_acquired_input(
                        entity.id.as_str(),
                        "entity",
                        &self.program_snapshot.id,
                        &input_indexes,
                        &location.input_id,
                    ) {
                        return Err(format!(
                            "call site '{}' was not published from acquired input '{}'",
                            entity.id, location.input_id
                        ));
                    }
                }
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
            if evidence.evidence_type.trim().is_empty() {
                return Err(format!(
                    "evidence '{}' has an empty evidence type",
                    evidence.id
                ));
            }
            let evidence_context = contexts_by_id
                .get(evidence.observation_context_id.as_str())
                .expect("validated evidence context must exist");
            if !evidence.scope.matches(evidence_context) {
                return Err(format!(
                    "evidence '{}' has a scope incompatible with observation context '{}'",
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
            if evidence.source_location.line == 0 {
                return Err(format!(
                    "evidence '{}' has no location within its evidence artifact",
                    evidence.id
                ));
            }
            // Evidence records the provenance its contributor observed: the
            // acquired input it read and the line within that input's evidence
            // artifact. That input need not be the one that published the call
            // site, so runtime evidence acquired from a trace keeps the trace's
            // input and line instead of inheriting the static call site's.
            if !identity_belongs_to_acquired_input(
                evidence.id.as_str(),
                "evidence",
                &self.program_snapshot.id,
                &input_indexes,
                &evidence.acquired_input_id,
            ) {
                return Err(format!(
                    "evidence '{}' was not published from acquired input '{}'",
                    evidence.id, evidence.acquired_input_id
                ));
            }
            for manifestation_id in &evidence.related_manifestation_ids {
                let manifestation = manifestations_by_id
                    .get(manifestation_id.as_str())
                    .ok_or_else(|| {
                        format!(
                            "evidence '{}' references unknown manifestation '{}'",
                            evidence.id, manifestation_id
                        )
                    })?;
                if manifestation.observation_context_id != evidence.observation_context_id {
                    return Err(format!(
                        "evidence '{}' and its related manifestations use different observation contexts",
                        evidence.id
                    ));
                }
                if manifestation.acquired_input_id != evidence.acquired_input_id {
                    return Err(format!(
                        "evidence '{}' and manifestation '{}' were read from different acquired inputs",
                        evidence.id, manifestation.id
                    ));
                }
            }
            match evidence.support {
                EvidenceSupport::CallSiteResolution | EvidenceSupport::TargetClaim => {
                    if subject.kind != ProgramEntityKind::CallSite {
                        return Err(format!(
                            "evidence record '{}' does not identify a call site",
                            evidence.id
                        ));
                    }
                    // A call site's target-set resolution is asserted only by
                    // the contribution that published the site, so resolution
                    // evidence is a statement about that site, read in the same
                    // acquired input, and must agree with where the site is. A
                    // target claim may be acquired later from another input and
                    // keeps its own provenance; it is bound instead to the
                    // manifestation it names, which is always read from the
                    // same acquired input as the evidence itself.
                    if evidence.support == EvidenceSupport::CallSiteResolution
                        && subject.source_location.as_ref() != Some(&evidence.source_location)
                    {
                        return Err(format!(
                            "evidence '{}' and call site '{}' disagree about the call-site location",
                            evidence.id, subject.id
                        ));
                    }
                }
                EvidenceSupport::ContributorIdentity => {
                    if subject.kind != ProgramEntityKind::Callable {
                        return Err(format!(
                            "contributor-identity evidence '{}' does not identify a callable",
                            evidence.id
                        ));
                    }
                    let identified = match evidence.related_manifestation_ids.as_slice() {
                        [manifestation_id] => manifestations_by_id
                            .get(manifestation_id.as_str())
                            .expect("validated related manifestation must exist"),
                        _ => {
                            return Err(format!(
                                "contributor-identity evidence '{}' must relate exactly one manifestation",
                                evidence.id
                            ));
                        }
                    };
                    if identified.entity_id != evidence.subject_entity_id
                        || identified.acquired_input_id != evidence.acquired_input_id
                    {
                        return Err(format!(
                            "contributor-identity evidence '{}' does not describe a manifestation of its subject callable",
                            evidence.id
                        ));
                    }
                }
            }
        }
        let identity_groups = self.identity_evidence_groups()?;
        let correspondence_claims_by_id: BTreeMap<_, _> = self
            .correspondence_claims
            .iter()
            .map(|claim| (claim.id.as_str(), claim))
            .collect();
        if correspondence_claims_by_id.len() != self.correspondence_claims.len() {
            return Err(
                "published snapshot contains duplicate correspondence-claim identities".into(),
            );
        }
        let mut correspondence_references = BTreeSet::new();
        let mut correspondence_ids_by_manifestation: BTreeMap<_, Vec<_>> = BTreeMap::new();
        for claim in &self.correspondence_claims {
            if claim.rule != CONTRIBUTOR_IDENTITY_CORRESPONDENCE_RULE {
                return Err(format!(
                    "correspondence claim '{}' uses unknown derivation rule '{}'",
                    claim.id, claim.rule
                ));
            }
            if claim.contributor_callable_id.trim().is_empty() {
                return Err(format!(
                    "correspondence claim '{}' has an empty contributor callable identity",
                    claim.id
                ));
            }
            if !inputs_by_id.contains_key(claim.acquired_input_id.as_str()) {
                return Err(format!(
                    "correspondence claim '{}' references unknown acquired input '{}'",
                    claim.id, claim.acquired_input_id
                ));
            }
            if !correspondence_references.insert((
                claim.acquired_input_id.as_str(),
                claim.contributor_callable_id.as_str(),
            )) {
                return Err(format!(
                    "contributor callable identity '{}' has duplicate correspondence claims for acquired input '{}'",
                    claim.contributor_callable_id, claim.acquired_input_id
                ));
            }
            let manifestation_ids = claim
                .manifestation_ids
                .iter()
                .map(ManifestationId::as_str)
                .collect::<BTreeSet<_>>();
            if manifestation_ids.len() != claim.manifestation_ids.len()
                || manifestation_ids.len() < 2
            {
                return Err(format!(
                    "correspondence claim '{}' must identify at least two distinct manifestations",
                    claim.id
                ));
            }
            let mut correspondence_contexts = BTreeSet::new();
            for manifestation_id in &claim.manifestation_ids {
                let manifestation = manifestations_by_id
                    .get(manifestation_id.as_str())
                    .ok_or_else(|| {
                        format!(
                            "correspondence claim '{}' references unknown manifestation '{}'",
                            claim.id, manifestation_id
                        )
                    })?;
                let entity = entities_by_id
                    .get(manifestation.entity_id.as_str())
                    .expect("validated manifestation entity must exist");
                if entity.kind != ProgramEntityKind::Callable {
                    return Err(format!(
                        "correspondence claim '{}' references a non-callable manifestation '{}'",
                        claim.id, manifestation_id
                    ));
                }
                if manifestation.acquired_input_id != claim.acquired_input_id
                    || manifestation.contributor_callable_id != claim.contributor_callable_id
                {
                    return Err(format!(
                        "correspondence claim '{}' does not preserve its contributor callable identity",
                        claim.id
                    ));
                }
                correspondence_contexts.insert(manifestation.observation_context_id.as_str());
                correspondence_ids_by_manifestation
                    .entry(manifestation_id.as_str())
                    .or_default()
                    .push(claim.id.as_str());
            }
            if correspondence_contexts.len() < 2 {
                return Err(format!(
                    "correspondence claim '{}' does not span observation contexts",
                    claim.id
                ));
            }
            let evidence_ids = claim
                .evidence_ids
                .iter()
                .map(EvidenceId::as_str)
                .collect::<BTreeSet<_>>();
            if evidence_ids.len() != claim.evidence_ids.len() || evidence_ids.is_empty() {
                return Err(format!(
                    "correspondence claim '{}' must reference distinct supporting evidence",
                    claim.id
                ));
            }
            let mut evidenced_manifestations = BTreeSet::new();
            for evidence_id in &claim.evidence_ids {
                let evidence = evidence_by_id.get(evidence_id.as_str()).ok_or_else(|| {
                    format!(
                        "correspondence claim '{}' references unknown evidence '{}'",
                        claim.id, evidence_id
                    )
                })?;
                if evidence.support != EvidenceSupport::ContributorIdentity {
                    return Err(format!(
                        "correspondence claim '{}' cites evidence '{}' with {:?} support instead of contributor-identity support",
                        claim.id, evidence.id, evidence.support
                    ));
                }
                if evidence.acquired_input_id != claim.acquired_input_id {
                    return Err(format!(
                        "correspondence claim '{}' and its evidence use different acquired inputs",
                        claim.id
                    ));
                }
                evidenced_manifestations.extend(
                    evidence
                        .related_manifestation_ids
                        .iter()
                        .filter(|id| manifestation_ids.contains(id.as_str()))
                        .map(ManifestationId::as_str),
                );
            }
            if evidenced_manifestations != manifestation_ids {
                return Err(format!(
                    "correspondence claim '{}' lacks evidence for every manifestation",
                    claim.id
                ));
            }
        }
        for claim in &self.correspondence_claims {
            let group = identity_groups
                .get(&(
                    claim.acquired_input_id.as_str(),
                    claim.contributor_callable_id.as_str(),
                ))
                .filter(|group| group.observation_context_ids.len() > 1)
                .ok_or_else(|| {
                    format!(
                        "correspondence claim '{}' is not derived from contributor-identity evidence spanning observation contexts",
                        claim.id
                    )
                })?;
            if claim
                .manifestation_ids
                .iter()
                .map(ManifestationId::as_str)
                .collect::<BTreeSet<_>>()
                != group.manifestation_ids
            {
                return Err(format!(
                    "correspondence claim '{}' does not identify every manifestation its contributor-identity evidence asserts",
                    claim.id
                ));
            }
            if claim
                .evidence_ids
                .iter()
                .map(EvidenceId::as_str)
                .collect::<BTreeSet<_>>()
                != group.evidence_ids
            {
                return Err(format!(
                    "correspondence claim '{}' does not cite exactly the contributor-identity evidence for its manifestations",
                    claim.id
                ));
            }
        }
        for ((acquired_input_id, contributor_callable_id), group) in &identity_groups {
            if group.observation_context_ids.len() < 2 {
                continue;
            }
            if !self.correspondence_claims.iter().any(|claim| {
                claim.acquired_input_id.as_str() == *acquired_input_id
                    && claim.contributor_callable_id == *contributor_callable_id
            }) {
                return Err(format!(
                    "contributor callable identity '{contributor_callable_id}' has contributor-identity evidence in {} observation contexts of acquired input '{acquired_input_id}' but no correspondence claim",
                    group.observation_context_ids.len()
                ));
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
        for derivation in &self.derivations {
            if derivation.rule != CONTRIBUTED_EVIDENCE_TARGET_RULE {
                return Err(format!(
                    "derivation for claim '{}' uses unknown rule '{}'",
                    derivation.output_claim_id, derivation.rule
                ));
            }
        }
        let resolutions_by_site: BTreeMap<_, _> = self
            .call_site_resolutions
            .iter()
            .map(|resolution| (resolution.call_site_id.as_str(), resolution))
            .collect();
        if resolutions_by_site.len() != self.call_site_resolutions.len() {
            return Err("published snapshot contains duplicate call-site resolutions".into());
        }
        for call_site in self
            .program_entities
            .iter()
            .filter(|entity| entity.kind == ProgramEntityKind::CallSite)
        {
            if !resolutions_by_site.contains_key(call_site.id.as_str()) {
                return Err(format!(
                    "call site '{}' has no target-set resolution",
                    call_site.id
                ));
            }
        }
        for resolution in &self.call_site_resolutions {
            let call_site = entities_by_id
                .get(resolution.call_site_id.as_str())
                .ok_or_else(|| {
                    format!(
                        "resolution references unknown call site '{}'",
                        resolution.call_site_id
                    )
                })?;
            if call_site.kind != ProgramEntityKind::CallSite {
                return Err(format!(
                    "resolution subject '{}' is not a call site",
                    resolution.call_site_id
                ));
            }
            if !context_ids.contains(resolution.observation_context_id.as_str()) {
                return Err(format!(
                    "resolution for '{}' references unknown observation context '{}'",
                    resolution.call_site_id, resolution.observation_context_id
                ));
            }
            if resolution.evidence_ids.is_empty() {
                return Err(format!(
                    "resolution for '{}' has no supporting evidence",
                    resolution.call_site_id
                ));
            }
            let caller_entity_id = call_site.caller_entity_id.as_ref().ok_or_else(|| {
                format!(
                    "resolution subject '{}' has no caller entity",
                    resolution.call_site_id
                )
            })?;
            let caller_manifestation = self
                .manifestations
                .iter()
                .find(|manifestation| {
                    manifestation.entity_id == *caller_entity_id
                        && manifestation.observation_context_id == resolution.observation_context_id
                })
                .ok_or_else(|| {
                    format!(
                        "resolution for '{}' has no caller manifestation in observation context '{}'",
                        resolution.call_site_id, resolution.observation_context_id
                    )
                })?;
            for evidence_id in &resolution.evidence_ids {
                let evidence = evidence_by_id.get(evidence_id.as_str()).ok_or_else(|| {
                    format!(
                        "resolution for '{}' references unknown evidence '{evidence_id}'",
                        resolution.call_site_id
                    )
                })?;
                if evidence.subject_entity_id != resolution.call_site_id
                    || evidence.observation_context_id != resolution.observation_context_id
                    || evidence.support != EvidenceSupport::CallSiteResolution
                    || !evidence
                        .related_manifestation_ids
                        .contains(&caller_manifestation.id)
                {
                    return Err(format!(
                        "resolution for '{}' and its evidence have incompatible subjects, contexts, callers, or support semantics",
                        resolution.call_site_id
                    ));
                }
            }
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
                    || evidence.support != EvidenceSupport::TargetClaim
                {
                    return Err(format!(
                        "claim '{}' and its evidence have incompatible subjects, contexts, or support semantics",
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
        if derivations_by_claim.len() != claim_ids.len() {
            return Err(
                "published snapshot contains derivations for claims it does not publish".into(),
            );
        }
        for resolution in &self.call_site_resolutions {
            let target_count = self
                .target_claims
                .iter()
                .filter(|claim| {
                    claim.call_site_id == resolution.call_site_id
                        && claim.observation_context_id == resolution.observation_context_id
                })
                .count();
            if !resolution.resolution.accepts_target_count(target_count) {
                return Err(format!(
                    "resolution for '{}' is incompatible with {target_count} target claims in its observation context",
                    resolution.call_site_id
                ));
            }
        }
        if self.call_graph_projection.program_snapshot_id != self.program_snapshot.id {
            return Err("call-graph projection belongs to another program snapshot".into());
        }
        let claims_by_id: BTreeMap<_, _> = self
            .target_claims
            .iter()
            .map(|claim| (claim.id.as_str(), claim))
            .collect();
        let relationships_by_claim: BTreeMap<_, _> = self
            .call_graph_projection
            .relationships
            .iter()
            .map(|relationship| (relationship.target_claim_id.as_str(), relationship))
            .collect();
        let relationship_claim_ids = relationships_by_claim
            .keys()
            .copied()
            .collect::<BTreeSet<_>>();
        if relationships_by_claim.len() != self.call_graph_projection.relationships.len()
            || relationship_claim_ids != claim_ids
        {
            return Err(
                "call-graph projection must contain exactly one relationship for every target claim"
                    .into(),
            );
        }
        for relationship in &self.call_graph_projection.relationships {
            let claim = claims_by_id
                .get(relationship.target_claim_id.as_str())
                .expect("validated relationship target claim must exist");
            let call_site = entities_by_id
                .get(claim.call_site_id.as_str())
                .expect("validated target claim call site must exist");
            let caller = entities_by_id.get(relationship.caller_entity_id.as_str());
            let callee = entities_by_id.get(relationship.callee_entity_id.as_str());
            let target = manifestations_by_id
                .get(claim.target_manifestation_id.as_str())
                .expect("validated target manifestation must exist");
            let resolution = resolutions_by_site
                .get(claim.call_site_id.as_str())
                .expect("validated target claim call site must have a resolution");
            let expected_correspondence_ids = correspondence_ids_by_manifestation
                .get(target.id.as_str())
                .into_iter()
                .flat_map(|ids| ids.iter().copied())
                .collect::<BTreeSet<_>>();
            let relationship_correspondence_ids = relationship
                .correspondence_claim_ids
                .iter()
                .map(CorrespondenceClaimId::as_str)
                .collect::<BTreeSet<_>>();
            let matches_claim = claim.call_site_id == relationship.call_site_id
                && claim.observation_context_id == relationship.target_observation_context_id
                && resolution.observation_context_id
                    == relationship.resolution_observation_context_id
                && resolution.resolution == relationship.resolution
                && relationship_correspondence_ids.len()
                    == relationship.correspondence_claim_ids.len()
                && relationship_correspondence_ids == expected_correspondence_ids
                && relationship.explanation_handle
                    == call_site_explanation_handle(&claim.call_site_id)
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
        let projected_site_ids: BTreeSet<_> = self
            .call_graph_projection
            .call_sites
            .iter()
            .map(|site| site.call_site_id.as_str())
            .collect();
        if projected_site_ids.len() != self.call_graph_projection.call_sites.len()
            || projected_site_ids.len() != resolutions_by_site.len()
        {
            return Err("call-graph projection must contain every call site exactly once".into());
        }
        for projected in &self.call_graph_projection.call_sites {
            let resolution = resolutions_by_site
                .get(projected.call_site_id.as_str())
                .ok_or_else(|| {
                    format!(
                        "projected call site '{}' has no resolution",
                        projected.call_site_id
                    )
                })?;
            let entity = entities_by_id
                .get(projected.call_site_id.as_str())
                .expect("validated projected call site must exist");
            let caller = entities_by_id.get(projected.caller_entity_id.as_str());
            if projected.resolution != resolution.resolution
                || projected.resolution_observation_context_id != resolution.observation_context_id
                || projected.explanation_handle
                    != call_site_explanation_handle(&projected.call_site_id)
                || entity.display_name != projected.call_site_display_name
                || entity.caller_entity_id.as_ref() != Some(&projected.caller_entity_id)
                || !caller.is_some_and(|caller| {
                    caller.kind == ProgramEntityKind::Callable
                        && caller.display_name == projected.caller_display_name
                })
            {
                return Err(format!(
                    "projected call site '{}' does not match canonical snapshot knowledge",
                    projected.call_site_id
                ));
            }
            let canonical_target_ids = self
                .target_claims
                .iter()
                .filter(|claim| claim.call_site_id == projected.call_site_id)
                .map(|claim| claim.id.as_str())
                .collect::<BTreeSet<_>>();
            let projected_target_ids = projected
                .targets
                .iter()
                .map(|target| target.target_claim_id.as_str())
                .collect::<BTreeSet<_>>();
            if projected_target_ids.len() != projected.targets.len()
                || projected_target_ids != canonical_target_ids
            {
                return Err(format!(
                    "projected call site '{}' does not preserve every target claim",
                    projected.call_site_id
                ));
            }
            for target in &projected.targets {
                let claim = claims_by_id
                    .get(target.target_claim_id.as_str())
                    .ok_or_else(|| {
                        format!(
                            "projected call site '{}' references unknown target claim '{}'",
                            projected.call_site_id, target.target_claim_id
                        )
                    })?;
                let manifestation = manifestations_by_id
                    .get(claim.target_manifestation_id.as_str())
                    .expect("validated target claim manifestation must exist");
                let callee = entities_by_id.get(target.callee_entity_id.as_str());
                let expected_correspondence_ids = correspondence_ids_by_manifestation
                    .get(manifestation.id.as_str())
                    .into_iter()
                    .flat_map(|ids| ids.iter().copied())
                    .collect::<BTreeSet<_>>();
                let projected_correspondence_ids = target
                    .correspondence_claim_ids
                    .iter()
                    .map(CorrespondenceClaimId::as_str)
                    .collect::<BTreeSet<_>>();
                if claim.call_site_id != projected.call_site_id
                    || claim.observation_context_id != target.target_observation_context_id
                    || manifestation.entity_id != target.callee_entity_id
                    || projected_correspondence_ids.len() != target.correspondence_claim_ids.len()
                    || projected_correspondence_ids != expected_correspondence_ids
                    || !callee.is_some_and(|callee| {
                        callee.kind == ProgramEntityKind::Callable
                            && callee.display_name == target.callee_display_name
                    })
                {
                    return Err(format!(
                        "projected target '{}' does not match its claim",
                        target.target_claim_id
                    ));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct CallableIdentity {
    entity_id: ProgramEntityId,
    manifestation_id: ManifestationId,
    display_name: String,
    representation: String,
}

#[derive(Clone)]
struct CallableReference {
    entity_id: ProgramEntityId,
    manifestation_id: ManifestationId,
    display_name: String,
}

/// A call site one acquired input published, indexed by the contributor
/// call-site identity that was asserted for it. A later acquired input reaches
/// an existing call site only through this index: nothing is joined by display
/// name, source line, or similar bodies.
struct PublishedCallSite {
    acquired_input_id: AcquiredInputId,
    caller_callable_id: String,
    caller: CallableReference,
    call_site_id: ProgramEntityId,
    resolution_index: usize,
    projection_index: usize,
}

/// Either a call site an acquired input contributes for the first time, or
/// evidence it attaches to a call site an earlier acquired input published.
/// Both carry target claims, which are published identically once the call
/// site they belong to is known.
enum ContributedSiteEvidence {
    NewCallSite(ContributedCallSite),
    Attachment(ContributedCallSiteAttachment),
}

/// The call site that target claims from one piece of contributed site
/// evidence attach to, whether it was just created or resolved by identity.
struct AttachedCallSite {
    call_site_id: ProgramEntityId,
    caller: CallableReference,
    resolution: Resolution,
    resolution_observation_context_id: ObservationContextId,
    id_prefix: String,
    projection_index: usize,
}

struct CorrespondenceSeed {
    input_index: usize,
    acquired_input_id: AcquiredInputId,
    contributor_callable_id: String,
    manifestation_ids: Vec<ManifestationId>,
}

type CallableIdentities = BTreeMap<ObservationContextId, BTreeMap<String, CallableIdentity>>;

#[allow(clippy::too_many_arguments)]
fn ensure_callable(
    contributor_callable_id: &str,
    name: &str,
    representation: &str,
    snapshot_id: &ProgramSnapshotId,
    input_index: usize,
    acquired_input_id: &AcquiredInputId,
    context_id: &ObservationContextId,
    defined: bool,
    identities: &mut CallableIdentities,
    entities: &mut Vec<ProgramEntity>,
    manifestations: &mut Vec<Manifestation>,
) -> Result<CallableReference, String> {
    let callable_index = identities.values().map(BTreeMap::len).sum::<usize>();
    let context_identities = identities.entry(context_id.clone()).or_default();
    if let Some(identity) = context_identities.get(contributor_callable_id) {
        if identity.display_name != name || identity.representation != representation {
            return Err(format!(
                "contributed callable identity '{contributor_callable_id}' has conflicting labels or representations in observation context '{context_id}'"
            ));
        }
        if defined {
            manifestations
                .iter_mut()
                .find(|manifestation| manifestation.id == identity.manifestation_id)
                .expect("callable manifestation must exist")
                .defined = true;
        }
        return Ok(CallableReference {
            entity_id: identity.entity_id.clone(),
            manifestation_id: identity.manifestation_id.clone(),
            display_name: identity.display_name.clone(),
        });
    }

    let entity_id = ProgramEntityId::new(format!(
        "entity:{snapshot_id}:input:{input_index}:callable:{callable_index}"
    ));
    let manifestation_id = ManifestationId::new(format!(
        "manifestation:{snapshot_id}:input:{input_index}:callable:{callable_index}"
    ));
    entities.push(ProgramEntity {
        id: entity_id.clone(),
        display_name: name.into(),
        kind: ProgramEntityKind::Callable,
        caller_entity_id: None,
        source_location: None,
    });
    manifestations.push(Manifestation {
        id: manifestation_id.clone(),
        entity_id: entity_id.clone(),
        acquired_input_id: acquired_input_id.clone(),
        contributor_callable_id: contributor_callable_id.into(),
        observation_context_id: context_id.clone(),
        representation: representation.into(),
        defined,
    });
    context_identities.insert(
        contributor_callable_id.into(),
        CallableIdentity {
            entity_id: entity_id.clone(),
            manifestation_id: manifestation_id.clone(),
            display_name: name.into(),
            representation: representation.into(),
        },
    );
    Ok(CallableReference {
        entity_id,
        manifestation_id,
        display_name: name.into(),
    })
}

/// Publishes one contributed evidence record. Both the resolution evidence a
/// call site is published with and the target evidence attached to it are built
/// here, so every path records provenance, context, and support the same way.
#[allow(clippy::too_many_arguments)]
fn contributed_evidence_record(
    id: EvidenceId,
    contributed: ContributedEvidence,
    observation_context_id: &ObservationContextId,
    subject_entity_id: &ProgramEntityId,
    related_manifestation_ids: Vec<ManifestationId>,
    acquired_input_id: &AcquiredInputId,
    evidence_artifact: &str,
    description: String,
) -> EvidenceRecord {
    EvidenceRecord {
        id,
        acquired_input_id: acquired_input_id.clone(),
        observation_context_id: observation_context_id.clone(),
        evidence_type: contributed.evidence_type,
        scope: contributed.scope,
        support: contributed.support,
        subject_entity_id: subject_entity_id.clone(),
        related_manifestation_ids,
        // Evidence keeps the provenance its contributor acquired it from, which
        // for a later acquired input is not where the call site was published.
        source_location: SourceLocation {
            input_id: acquired_input_id.clone(),
            artifact: evidence_artifact.into(),
            line: contributed.location.line,
        },
        description,
    }
}

/// The only identity an acquired input can have: its program snapshot and its
/// position in that snapshot's acquisition order.
fn acquired_input_id(snapshot_id: &ProgramSnapshotId, input_index: usize) -> AcquiredInputId {
    AcquiredInputId::new(format!("input:{snapshot_id}:{input_index}"))
}

/// Core-generated identities are qualified by the program snapshot and the
/// acquired input they were published from: `{kind}:{snapshot}:input:{index}:…`.
/// Requiring that exact prefix, rather than matching a suffix of the input
/// identity, is what stops a hand-edited export from pointing a record at a
/// forged or foreign acquired input while keeping the identity the rest of the
/// snapshot refers to it by.
fn identity_belongs_to_acquired_input(
    id: &str,
    kind: &str,
    snapshot_id: &ProgramSnapshotId,
    input_indexes: &BTreeMap<&str, usize>,
    acquired_input_id: &AcquiredInputId,
) -> bool {
    input_indexes
        .get(acquired_input_id.as_str())
        .is_some_and(|index| id.starts_with(&format!("{kind}:{snapshot_id}:input:{index}:")))
}

fn call_site_explanation_handle(call_site_id: &ProgramEntityId) -> ExplanationHandle {
    ExplanationHandle(format!("explanation:{call_site_id}"))
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
    let mut observation_contexts = vec![context.clone()];
    let mut contexts_by_id = BTreeMap::from([(context.id.clone(), context.clone())]);
    for contribution in &contributions {
        contribution.validate(&contributor, &context)?;
        for contributed_context in &contribution.observation_contexts {
            if let Some(existing) = contexts_by_id.get(&contributed_context.id) {
                if existing != contributed_context {
                    return Err(format!(
                        "observation-context identity '{}' has conflicting qualifications",
                        contributed_context.id
                    ));
                }
            } else {
                contexts_by_id.insert(contributed_context.id.clone(), contributed_context.clone());
                observation_contexts.push(contributed_context.clone());
            }
        }
    }
    let mut acquired_inputs = Vec::new();
    let mut program_entities = Vec::new();
    let mut manifestations = Vec::new();
    let mut evidence_records = Vec::new();
    let mut call_site_resolutions = Vec::new();
    let mut target_claims = Vec::new();
    let mut correspondence_seeds = Vec::new();
    let mut derivations = Vec::new();
    let mut relationships = Vec::new();
    let mut projected_call_sites = Vec::new();
    // Keyed by the qualifier a contributor call-site identity is unique within:
    // its observation context and its acquired input, the latter named by the
    // content fingerprint the contributor declared for it.
    let mut published_call_sites: BTreeMap<
        (ObservationContextId, String, String),
        Vec<PublishedCallSite>,
    > = BTreeMap::new();

    for (input_index, contribution) in contributions.into_iter().enumerate() {
        let input_id = acquired_input_id(&snapshot_id, input_index);
        let path_text = contribution.input.path.clone();
        let evidence_artifact = contribution.input.evidence_artifact.clone();
        let content_fingerprint = contribution.input.content_fingerprint.clone();
        acquired_inputs.push(AcquiredInput {
            id: input_id.clone(),
            path: path_text.clone(),
            evidence_artifact: evidence_artifact.clone(),
            media_type: contribution.input.media_type,
            acquisition_method: contribution.input.acquisition_method,
            content_fingerprint: contribution.input.content_fingerprint,
        });

        let mut identities = BTreeMap::new();
        for (callable_index, function) in contribution.callables.into_iter().enumerate() {
            let callable = ensure_callable(
                &function.contributor_callable_id,
                &function.display_name,
                &function.representation,
                &snapshot_id,
                input_index,
                &input_id,
                &function.observation_context_id,
                function.defined,
                &mut identities,
                &mut program_entities,
                &mut manifestations,
            )?;
            evidence_records.push(contributed_evidence_record(
                EvidenceId::new(format!(
                    "evidence:{snapshot_id}:input:{input_index}:callable-identity:{callable_index}"
                )),
                function.identity_evidence,
                &function.observation_context_id,
                &callable.entity_id,
                vec![callable.manifestation_id],
                &input_id,
                &evidence_artifact,
                format!(
                    "contributed evidence asserts contributor callable identity '{}' for this manifestation",
                    function.contributor_callable_id
                ),
            ));
        }

        for (contributed_index, contributed) in contribution
            .call_sites
            .into_iter()
            .map(ContributedSiteEvidence::NewCallSite)
            .chain(
                contribution
                    .call_site_attachments
                    .into_iter()
                    .map(ContributedSiteEvidence::Attachment),
            )
            .enumerate()
        {
            let (site, contributed_targets) = match contributed {
                ContributedSiteEvidence::NewCallSite(call) => {
                    let caller = identities
                        .get(&call.observation_context_id)
                        .and_then(|context_identities| {
                            context_identities.get(&call.caller_callable_id)
                        })
                        .map(|identity| CallableReference {
                            entity_id: identity.entity_id.clone(),
                            manifestation_id: identity.manifestation_id.clone(),
                            display_name: identity.display_name.clone(),
                        })
                        .ok_or_else(|| {
                            format!(
                                "call-site evidence references uncontributed caller identity '{}' in observation context '{}'",
                                call.caller_callable_id, call.observation_context_id
                            )
                        })?;
                    let call_site_index = contributed_index;
                    let call_site_id = ProgramEntityId::new(format!(
                        "entity:{snapshot_id}:input:{input_index}:call-site:{call_site_index}"
                    ));
                    let location = SourceLocation {
                        input_id: input_id.clone(),
                        artifact: evidence_artifact.clone(),
                        line: call.line,
                    };
                    let call_site_display_name = format!(
                        "{} call at {}:{}",
                        caller.display_name, evidence_artifact, call.line
                    );
                    program_entities.push(ProgramEntity {
                        id: call_site_id.clone(),
                        display_name: call_site_display_name.clone(),
                        kind: ProgramEntityKind::CallSite,
                        caller_entity_id: Some(caller.entity_id.clone()),
                        source_location: Some(location),
                    });
                    let evidence_id = EvidenceId::new(format!(
                        "evidence:{snapshot_id}:input:{input_index}:call-site:{call_site_index}"
                    ));
                    evidence_records.push(contributed_evidence_record(
                        evidence_id.clone(),
                        call.evidence,
                        &call.observation_context_id,
                        &call_site_id,
                        vec![caller.manifestation_id.clone()],
                        &input_id,
                        &evidence_artifact,
                        format!(
                            "contributed call site has {:?} target-set resolution",
                            call.resolution
                        ),
                    ));
                    call_site_resolutions.push(CallSiteResolution {
                        call_site_id: call_site_id.clone(),
                        observation_context_id: call.observation_context_id.clone(),
                        resolution: call.resolution,
                        evidence_ids: vec![evidence_id],
                    });
                    projected_call_sites.push(ProjectedCallSite {
                        caller_entity_id: caller.entity_id.clone(),
                        caller_display_name: caller.display_name.clone(),
                        call_site_id: call_site_id.clone(),
                        call_site_display_name,
                        resolution_observation_context_id: call.observation_context_id.clone(),
                        resolution: call.resolution,
                        targets: Vec::new(),
                        explanation_handle: call_site_explanation_handle(&call_site_id),
                    });
                    let projection_index = projected_call_sites.len() - 1;
                    published_call_sites
                        .entry((
                            call.observation_context_id.clone(),
                            content_fingerprint.clone(),
                            call.contributor_call_site_id.as_str().to_owned(),
                        ))
                        .or_default()
                        .push(PublishedCallSite {
                            acquired_input_id: input_id.clone(),
                            caller_callable_id: call.caller_callable_id,
                            caller: caller.clone(),
                            call_site_id: call_site_id.clone(),
                            resolution_index: call_site_resolutions.len() - 1,
                            projection_index,
                        });
                    (
                        AttachedCallSite {
                            call_site_id,
                            caller,
                            resolution: call.resolution,
                            resolution_observation_context_id: call.observation_context_id,
                            id_prefix: format!("input:{input_index}:call-site:{call_site_index}"),
                            projection_index,
                        },
                        call.target_claims,
                    )
                }
                ContributedSiteEvidence::Attachment(attachment) => {
                    let reference = attachment.call_site;
                    let candidates = published_call_sites
                        .get(&(
                            reference.observation_context_id.clone(),
                            reference.acquired_input_fingerprint.clone(),
                            reference.contributor_call_site_id.as_str().to_owned(),
                        ))
                        .map_or(&[][..], Vec::as_slice);
                    let published = match candidates {
                        [] => {
                            return Err(format!(
                                "call-site attachment references unknown contributor call-site identity '{}' in acquired input '{}' and observation context '{}'",
                                reference.contributor_call_site_id,
                                reference.acquired_input_fingerprint,
                                reference.observation_context_id
                            ));
                        }
                        [published] => published,
                        ambiguous => {
                            let inputs = ambiguous
                                .iter()
                                .map(|published| published.acquired_input_id.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            return Err(format!(
                                "call-site attachment references ambiguous contributor call-site identity '{}' in observation context '{}': acquired inputs {inputs} share content fingerprint '{}' and each published it",
                                reference.contributor_call_site_id,
                                reference.observation_context_id,
                                reference.acquired_input_fingerprint
                            ));
                        }
                    };
                    if published.caller_callable_id != reference.caller_callable_id {
                        return Err(format!(
                            "call-site attachment names caller identity '{}' but contributor call-site identity '{}' was contributed by caller '{}'",
                            reference.caller_callable_id,
                            reference.contributor_call_site_id,
                            published.caller_callable_id
                        ));
                    }
                    let call_site_id = published.call_site_id.clone();
                    let caller = published.caller.clone();
                    let resolution_index = published.resolution_index;
                    let projection_index = published.projection_index;
                    let resolution_observation_context_id = call_site_resolutions[resolution_index]
                        .observation_context_id
                        .clone();
                    (
                        AttachedCallSite {
                            call_site_id,
                            caller,
                            resolution: call_site_resolutions[resolution_index].resolution,
                            resolution_observation_context_id,
                            id_prefix: format!(
                                "input:{input_index}:attachment:{contributed_index}"
                            ),
                            projection_index,
                        },
                        attachment.target_claims,
                    )
                }
            };
            let AttachedCallSite {
                call_site_id,
                caller,
                resolution,
                resolution_observation_context_id,
                id_prefix,
                projection_index,
            } = site;
            let mut projected_targets = Vec::new();
            for (target_index, target) in contributed_targets.into_iter().enumerate() {
                let callee = ensure_callable(
                    &target.target_callable_id,
                    &target.callee_display_name,
                    &target.target_representation,
                    &snapshot_id,
                    input_index,
                    &input_id,
                    &target.observation_context_id,
                    false,
                    &mut identities,
                    &mut program_entities,
                    &mut manifestations,
                )?;
                let mut target_evidence_ids = Vec::new();
                for (evidence_index, evidence) in target.evidence.into_iter().enumerate() {
                    let target_evidence_id = EvidenceId::new(format!(
                        "evidence:{snapshot_id}:{id_prefix}:target:{target_index}:{evidence_index}"
                    ));
                    evidence_records.push(contributed_evidence_record(
                        target_evidence_id.clone(),
                        evidence,
                        &target.observation_context_id,
                        &call_site_id,
                        vec![callee.manifestation_id.clone()],
                        &input_id,
                        &evidence_artifact,
                        format!(
                            "contributed evidence identifies '{}' as a possible target",
                            target.callee_display_name
                        ),
                    ));
                    target_evidence_ids.push(target_evidence_id);
                }
                let claim_id = TargetClaimId::new(format!(
                    "claim:{snapshot_id}:{id_prefix}:target:{target_index}"
                ));
                let claim = TargetClaim {
                    id: claim_id.clone(),
                    call_site_id: call_site_id.clone(),
                    target_manifestation_id: callee.manifestation_id,
                    observation_context_id: target.observation_context_id.clone(),
                    evidence_ids: target_evidence_ids.clone(),
                };
                derivations.push(Derivation {
                    rule: CONTRIBUTED_EVIDENCE_TARGET_RULE.into(),
                    input_evidence_ids: target_evidence_ids,
                    output_claim_id: claim_id.clone(),
                });
                relationships.push(CallRelationship {
                    target_claim_id: claim_id.clone(),
                    caller_entity_id: caller.entity_id.clone(),
                    caller_display_name: caller.display_name.clone(),
                    callee_entity_id: callee.entity_id.clone(),
                    callee_display_name: callee.display_name.clone(),
                    call_site_id: call_site_id.clone(),
                    target_observation_context_id: target.observation_context_id.clone(),
                    resolution_observation_context_id: resolution_observation_context_id.clone(),
                    resolution,
                    correspondence_claim_ids: Vec::new(),
                    explanation_handle: call_site_explanation_handle(&call_site_id),
                });
                projected_targets.push(ProjectedCallTarget {
                    target_claim_id: claim_id,
                    callee_entity_id: callee.entity_id,
                    callee_display_name: callee.display_name,
                    target_observation_context_id: target.observation_context_id,
                    correspondence_claim_ids: Vec::new(),
                });
                target_claims.push(claim);
            }
            projected_call_sites[projection_index]
                .targets
                .extend(projected_targets);
        }

        let mut manifestations_by_contributor_id: BTreeMap<String, Vec<ManifestationId>> =
            BTreeMap::new();
        for context_identities in identities.values() {
            for (contributor_callable_id, identity) in context_identities {
                manifestations_by_contributor_id
                    .entry(contributor_callable_id.clone())
                    .or_default()
                    .push(identity.manifestation_id.clone());
            }
        }
        correspondence_seeds.extend(
            manifestations_by_contributor_id
                .into_iter()
                .filter(|(_, manifestation_ids)| manifestation_ids.len() > 1)
                .map(
                    |(contributor_callable_id, manifestation_ids)| CorrespondenceSeed {
                        input_index,
                        acquired_input_id: input_id.clone(),
                        contributor_callable_id,
                        manifestation_ids,
                    },
                ),
        );
    }

    let mut identity_evidence_by_manifestation: BTreeMap<
        (&AcquiredInputId, &ManifestationId),
        Vec<&EvidenceId>,
    > = BTreeMap::new();
    for evidence in &evidence_records {
        if evidence.support != EvidenceSupport::ContributorIdentity {
            continue;
        }
        for manifestation_id in &evidence.related_manifestation_ids {
            identity_evidence_by_manifestation
                .entry((&evidence.acquired_input_id, manifestation_id))
                .or_default()
                .push(&evidence.id);
        }
    }
    let mut correspondence_claims = Vec::new();
    for seed in correspondence_seeds {
        let mut evidence_ids = BTreeSet::new();
        let manifestation_ids = seed
            .manifestation_ids
            .into_iter()
            .filter(|manifestation_id| {
                let Some(related) = identity_evidence_by_manifestation
                    .get(&(&seed.acquired_input_id, manifestation_id))
                else {
                    return false;
                };
                evidence_ids.extend(related.iter().map(|id| (*id).clone()));
                true
            })
            .collect::<Vec<_>>();
        if manifestation_ids.len() < 2 {
            continue;
        }
        let correspondence_index = correspondence_claims.len();
        correspondence_claims.push(CorrespondenceClaim {
            id: CorrespondenceClaimId::new(format!(
                "correspondence:{snapshot_id}:input:{}:callable:{correspondence_index}",
                seed.input_index
            )),
            rule: CONTRIBUTOR_IDENTITY_CORRESPONDENCE_RULE.into(),
            acquired_input_id: seed.acquired_input_id,
            contributor_callable_id: seed.contributor_callable_id,
            manifestation_ids,
            evidence_ids: evidence_ids.into_iter().collect(),
        });
    }
    let mut correspondence_ids_by_manifestation: BTreeMap<
        ManifestationId,
        Vec<CorrespondenceClaimId>,
    > = BTreeMap::new();
    for claim in &correspondence_claims {
        for manifestation_id in &claim.manifestation_ids {
            correspondence_ids_by_manifestation
                .entry(manifestation_id.clone())
                .or_default()
                .push(claim.id.clone());
        }
    }
    let target_manifestations_by_claim = target_claims
        .iter()
        .map(|claim| (claim.id.clone(), claim.target_manifestation_id.clone()))
        .collect::<BTreeMap<_, _>>();
    for relationship in &mut relationships {
        relationship.correspondence_claim_ids = target_manifestations_by_claim
            .get(&relationship.target_claim_id)
            .and_then(|id| correspondence_ids_by_manifestation.get(id))
            .cloned()
            .unwrap_or_default();
    }
    for target in projected_call_sites
        .iter_mut()
        .flat_map(|site| &mut site.targets)
    {
        target.correspondence_claim_ids = target_manifestations_by_claim
            .get(&target.target_claim_id)
            .and_then(|id| correspondence_ids_by_manifestation.get(id))
            .cloned()
            .unwrap_or_default();
    }

    let snapshot = PublishedSnapshot {
        schema_version: SNAPSHOT_SCHEMA_VERSION.into(),
        program_snapshot: ProgramSnapshot {
            id: snapshot_id.clone(),
        },
        acquired_inputs,
        observation_contexts,
        program_entities,
        manifestations,
        evidence_records,
        call_site_resolutions,
        target_claims,
        correspondence_claims,
        derivations,
        call_graph_projection: CallGraphProjection {
            name: "call-graph".into(),
            program_snapshot_id: snapshot_id,
            relationships,
            call_sites: projected_call_sites,
        },
    };
    snapshot.validate()?;
    Ok(snapshot)
}
