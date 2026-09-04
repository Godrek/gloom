use crate::contributor::{
    ContributedCallSite, ContributedCallSiteAttachment, ContributedEvidence, ContributorIdentity,
    DECLARED_CALLABLE_REPRESENTATIONS, EvidenceContribution, STATIC_DIRECT_CALL_EVIDENCE_TYPE,
    fingerprint_parts,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

pub const SNAPSHOT_SCHEMA_VERSION: &str = "2.0-pre.1";

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

/// The boundary within which one contributor callable identity means one
/// callable.
///
/// Linkage decides sameness, not spelling. A callable that is not visible
/// beyond the acquired input it was read from — an LLVM `internal` or
/// `private` function — is identified within that input, so two such callables
/// in different inputs are different callables however they are spelled. A
/// callable the link can see is identified in the namespace the link joins it
/// by, so one identity may manifest in several acquired inputs.
///
/// The core never parses a contributor callable identity, so a declared scope
/// is what lets it check that an input-scoped identity never spans acquired
/// inputs.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CallableIdentityScope {
    AcquiredInput,
    LinkageNamespace,
}

/// The opaque identity an evidence contributor asserts for one callable,
/// together with the scope in which that assertion means one callable.
///
/// Keeping the two values together makes an identity impossible to pass through
/// contributor, evidence-source, and publication code with a different scope
/// by mistake. The core never parses `id`; only the contributor's explicit
/// scope controls whether the identity may join acquired inputs.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "UncheckedContributorCallableIdentity")]
pub struct ContributorCallableIdentity {
    id: String,
    scope: CallableIdentityScope,
}

#[derive(Deserialize)]
struct UncheckedContributorCallableIdentity {
    id: String,
    scope: CallableIdentityScope,
}

impl TryFrom<UncheckedContributorCallableIdentity> for ContributorCallableIdentity {
    type Error = String;

    fn try_from(identity: UncheckedContributorCallableIdentity) -> Result<Self, Self::Error> {
        Self::new(identity.id, identity.scope)
    }
}

impl ContributorCallableIdentity {
    pub fn new(id: impl Into<String>, scope: CallableIdentityScope) -> Result<Self, String> {
        let id = id.into();
        if id.is_empty() || id.trim() != id {
            return Err(format!(
                "contributor callable identity {id:?} must be non-empty and free of surrounding whitespace"
            ));
        }
        Ok(Self { id, scope })
    }

    pub fn as_str(&self) -> &str {
        &self.id
    }

    pub fn scope(&self) -> CallableIdentityScope {
        self.scope
    }
}

impl fmt::Display for ContributorCallableIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.id.fmt(formatter)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Manifestation {
    pub id: ManifestationId,
    pub entity_id: ProgramEntityId,
    pub acquired_input_id: AcquiredInputId,
    pub contributor_callable_identity: ContributorCallableIdentity,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completeness_basis: Option<CompletenessBasis>,
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

/// A contributor's explicit declaration that it observed a closed target set at
/// one call site: `boundary` names the scope it observed, and `guarantee` states
/// why no other target can exist within that boundary. Completeness is declared
/// rather than inferred, so it stays independent of how the evidence was
/// obtained.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CompletenessBasis {
    pub boundary: String,
    pub guarantee: String,
}

impl CompletenessBasis {
    pub(crate) fn validate(&self, subject: &str) -> Result<(), String> {
        if self.boundary.trim().is_empty() || self.guarantee.trim().is_empty() {
            return Err(format!(
                "{subject} carries a completeness basis without both a boundary and a guarantee"
            ));
        }
        Ok(())
    }
}

/// The one completeness rule, applied by both the contributor seam and snapshot
/// loading so they cannot drift: Complete resolution rests on a declared
/// completeness basis, and only Complete resolution may.
pub(crate) fn check_completeness_declaration(
    resolution: Resolution,
    declared_basis: bool,
    subject: &str,
) -> Result<(), String> {
    match (resolution, declared_basis) {
        (Resolution::Complete, false) => Err(missing_completeness_basis_error(subject)),
        (Resolution::Partial | Resolution::Absent, true) => {
            Err(contradictory_completeness_basis_error(subject))
        }
        _ => Ok(()),
    }
}

fn missing_completeness_basis_error(subject: &str) -> String {
    format!(
        "{subject} declares Complete resolution without a completeness basis; Complete \
         resolution requires at least one call-site-resolution evidence record carrying a \
         completeness basis that names the boundary the contributor observed and the guarantee \
         that no other target exists within it"
    )
}

fn contradictory_completeness_basis_error(subject: &str) -> String {
    format!(
        "{subject} carries a completeness basis without declaring Complete resolution; a \
         completeness basis asserts a closed target set and contradicts Partial or Absent \
         resolution"
    )
}

pub(crate) fn misplaced_completeness_basis_error(subject: &str) -> String {
    format!(
        "{subject} carries a completeness basis on evidence that does not support a call-site \
         resolution; only call-site-resolution evidence can close a target set"
    )
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
    pub contributor_callable_identity: ContributorCallableIdentity,
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

/// Where an evidence contributor read the declaration a callable manifestation
/// was asserted from.
///
/// A callable entity carries no source location, because its evidence does not
/// preserve one for the entity itself; the contributor-identity evidence that
/// declared one of its manifestations does, and that is what a searcher needs
/// to tell two callables spelled the same way apart.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableDeclaration {
    pub evidence_id: EvidenceId,
    pub source_location: SourceLocation,
}

/// One manifestation of a matched callable, with the acquired input it was read
/// from and the declaration it was read at.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchedCallableManifestation {
    pub manifestation_id: ManifestationId,
    pub contributor_callable_identity: ContributorCallableIdentity,
    pub acquired_input_id: AcquiredInputId,
    pub acquired_input_path: String,
    pub observation_context_id: ObservationContextId,
    pub representation: String,
    pub defined: bool,
    /// Absent for a manifestation no contribution declared, one a target claim
    /// introduced by naming it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub declaration: Option<CallableDeclaration>,
}

/// One callable a search matched.
///
/// The label is what matched; the identity is `entity_id`, and the
/// manifestations are what tell this callable from another the search matched
/// under the same label.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SearchedCallable {
    pub entity_id: ProgramEntityId,
    pub display_name: String,
    pub manifestations: Vec<SearchedCallableManifestation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallableSearchResult {
    pub query_name: String,
    pub program_snapshot_id: ProgramSnapshotId,
    pub label: String,
    pub callables: Vec<SearchedCallable>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallRelationshipsResult {
    pub query_name: String,
    pub program_snapshot_id: ProgramSnapshotId,
    pub selected_callable_entity_id: ProgramEntityId,
    pub selected_callable_observation_context_id: ObservationContextId,
    pub correspondence_claims: Vec<CorrespondenceClaim>,
    pub relationships: Vec<CallRelationship>,
    pub call_sites: Vec<ProjectedCallSite>,
}

/// The bounded shortest call path returned by the `call-path` named query.
/// Every item is an existing projected relationship and therefore carries its
/// target claim, call-site identity, resolution, and explanation handle.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CallPathResult {
    pub query_name: String,
    pub program_snapshot_id: ProgramSnapshotId,
    pub start_entity_id: ProgramEntityId,
    pub end_entity_id: ProgramEntityId,
    pub max_relationships: usize,
    pub correspondence_claims: Vec<CorrespondenceClaim>,
    pub path: Option<Vec<CallRelationship>>,
}

/// Results from the shared snapshot named-query seam.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NamedQueryResult {
    CallableSearch(CallableSearchResult),
    CallRelationships(CallRelationshipsResult),
    CallPath(CallPathResult),
}

impl NamedQueryResult {
    pub fn callable_search(&self) -> Option<&CallableSearchResult> {
        match self {
            Self::CallableSearch(result) => Some(result),
            Self::CallRelationships(_) | Self::CallPath(_) => None,
        }
    }

    pub fn call_relationships(&self) -> Option<&CallRelationshipsResult> {
        match self {
            Self::CallRelationships(result) => Some(result),
            Self::CallableSearch(_) | Self::CallPath(_) => None,
        }
    }

    pub fn call_path(&self) -> Option<&CallPathResult> {
        match self {
            Self::CallPath(result) => Some(result),
            Self::CallableSearch(_) | Self::CallRelationships(_) => None,
        }
    }
}

/// An immutable program snapshot made available for queries.
///
/// A published snapshot is coherent by construction: the only ways to obtain
/// one are publishing evidence contributions and deserializing an export, and
/// both run `validate` before the value exists. The fields are private and
/// there is no public constructor, so no safe code outside this crate can hold
/// a snapshot whose invariants were never checked, and a republished export
/// cannot smuggle in knowledge the core never derived.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "wire::PublishedSnapshot")]
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

/// The wire form an export is read back in, before anything has checked that
/// it is coherent.
///
/// Deserializing a [`PublishedSnapshot`] goes through this private module so
/// that reading an export and accepting it stay one step: a document that
/// `validate` rejects never becomes a published snapshot, and the failure
/// carries the message the loader reports. The stand-in carries the name of
/// the type it stands in for, so a document of the wrong shape is still
/// reported against `PublishedSnapshot`.
mod wire {
    use super::{
        AcquiredInput, CallGraphProjection, CallSiteResolution, CorrespondenceClaim, Derivation,
        EvidenceRecord, Manifestation, ObservationContext, ProgramEntity, ProgramSnapshot,
        TargetClaim,
    };
    use serde::Deserialize;

    #[derive(Deserialize)]
    pub(super) struct PublishedSnapshot {
        pub(super) schema_version: String,
        pub(super) program_snapshot: ProgramSnapshot,
        pub(super) acquired_inputs: Vec<AcquiredInput>,
        pub(super) observation_contexts: Vec<ObservationContext>,
        pub(super) program_entities: Vec<ProgramEntity>,
        pub(super) manifestations: Vec<Manifestation>,
        pub(super) evidence_records: Vec<EvidenceRecord>,
        pub(super) call_site_resolutions: Vec<CallSiteResolution>,
        pub(super) target_claims: Vec<TargetClaim>,
        pub(super) correspondence_claims: Vec<CorrespondenceClaim>,
        pub(super) derivations: Vec<Derivation>,
        pub(super) call_graph_projection: CallGraphProjection,
    }
}

impl TryFrom<wire::PublishedSnapshot> for PublishedSnapshot {
    type Error = String;

    fn try_from(document: wire::PublishedSnapshot) -> Result<Self, Self::Error> {
        let snapshot = Self {
            schema_version: document.schema_version,
            program_snapshot: document.program_snapshot,
            acquired_inputs: document.acquired_inputs,
            observation_contexts: document.observation_contexts,
            program_entities: document.program_entities,
            manifestations: document.manifestations,
            evidence_records: document.evidence_records,
            call_site_resolutions: document.call_site_resolutions,
            target_claims: document.target_claims,
            correspondence_claims: document.correspondence_claims,
            derivations: document.derivations,
            call_graph_projection: document.call_graph_projection,
        };
        snapshot.validate()?;
        Ok(snapshot)
    }
}

/// Reads a published snapshot back from an exported document.
///
/// Loading and deserializing enforce exactly the same invariants; loading only
/// finishes reading the document first. It takes the wire form, insists the
/// text held nothing but that one document, and converts afterwards, so a
/// malformed export is reported as the malformed text it is instead of being
/// preempted by the first invariant its content happens to break.
pub(crate) fn load_json(text: &str) -> Result<PublishedSnapshot, String> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let document = wire::PublishedSnapshot::deserialize(&mut deserializer)
        .map_err(|error| error.to_string())?;
    deserializer.end().map_err(|error| error.to_string())?;
    PublishedSnapshot::try_from(document)
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

    /// Finds the callable entities whose display name contains `label`.
    ///
    /// A display name is a label, not an identity, so a search matches labels
    /// and answers with identities: every match carries its entity identity and
    /// one entry per manifestation naming the acquired input it was read from
    /// and the declaration it was read at. That is what lets a user choose
    /// between two translation-unit-local callables spelled the same way. A
    /// search that matches nothing is an empty result, not an error: absence in
    /// an open-world projection means only that nothing matched.
    pub(crate) fn search_callables(&self, label: &str) -> CallableSearchResult {
        CallableSearchResult {
            query_name: "callable-search".into(),
            program_snapshot_id: self.program_snapshot.id.clone(),
            label: label.into(),
            callables: self
                .program_entities
                .iter()
                .filter(|entity| {
                    entity.kind == ProgramEntityKind::Callable
                        && entity.display_name.contains(label)
                })
                .map(|entity| self.searched_callable(entity))
                .collect(),
        }
    }

    fn searched_callable(&self, entity: &ProgramEntity) -> SearchedCallable {
        SearchedCallable {
            entity_id: entity.id.clone(),
            display_name: entity.display_name.clone(),
            manifestations: self
                .manifestations
                .iter()
                .filter(|manifestation| manifestation.entity_id == entity.id)
                .map(|manifestation| {
                    let acquired_input = self
                        .acquired_inputs
                        .iter()
                        .find(|input| input.id == manifestation.acquired_input_id)
                        .expect("validated manifestation must name an acquired input");
                    SearchedCallableManifestation {
                        manifestation_id: manifestation.id.clone(),
                        contributor_callable_identity: manifestation
                            .contributor_callable_identity
                            .clone(),
                        acquired_input_id: manifestation.acquired_input_id.clone(),
                        acquired_input_path: acquired_input.path.clone(),
                        observation_context_id: manifestation.observation_context_id.clone(),
                        representation: manifestation.representation.clone(),
                        defined: manifestation.defined,
                        declaration: self.callable_declaration(&manifestation.id),
                    }
                })
                .collect(),
        }
    }

    fn callable_declaration(
        &self,
        manifestation_id: &ManifestationId,
    ) -> Option<CallableDeclaration> {
        self.evidence_records
            .iter()
            .find(|evidence| {
                evidence.support == EvidenceSupport::ContributorIdentity
                    && evidence
                        .related_manifestation_ids
                        .contains(manifestation_id)
            })
            .map(|evidence| CallableDeclaration {
                evidence_id: evidence.id.clone(),
                source_location: evidence.source_location.clone(),
            })
    }

    /// How one ambiguous candidate is offered to a user: its identity, the
    /// acquired input it was read from, and where it was declared. Two
    /// callables spelled the same way are told apart here rather than by
    /// asking the user to guess from bare identities.
    fn callable_selection_hint(&self, entity: &ProgramEntity) -> String {
        let described = self
            .searched_callable(entity)
            .manifestations
            .into_iter()
            .map(|manifestation| match manifestation.declaration {
                // The evidence artifact is the acquired input's own path for a
                // textual input and a generated artifact for a compiled one, so
                // it is named only when it says something the path does not.
                Some(declaration)
                    if declaration.source_location.artifact
                        == manifestation.acquired_input_path =>
                {
                    format!(
                        "declared at {}:{}",
                        manifestation.acquired_input_path, declaration.source_location.line
                    )
                }
                Some(declaration) => format!(
                    "acquired from {}, declared at {}:{}",
                    manifestation.acquired_input_path,
                    declaration.source_location.artifact,
                    declaration.source_location.line
                ),
                None => format!("acquired from {}", manifestation.acquired_input_path),
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!("{} ({described})", entity.id)
    }

    fn select_callable(
        &self,
        display_name: &str,
        entity_id: Option<&ProgramEntityId>,
        role: &str,
    ) -> Result<&ProgramEntity, String> {
        let candidates = self
            .program_entities
            .iter()
            .filter(|entity| {
                entity.kind == ProgramEntityKind::Callable && entity.display_name == display_name
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Err(format!("unknown callable '{display_name}'"));
        }
        if let Some(entity_id) = entity_id {
            candidates
                .iter()
                .copied()
                .find(|candidate| candidate.id == *entity_id)
                .ok_or_else(|| {
                    format!(
                        "callable entity '{entity_id}' does not identify callable '{display_name}'"
                    )
                })
        } else if candidates.len() == 1 {
            Ok(candidates[0])
        } else {
            let candidate_ids = candidates
                .iter()
                .map(|candidate| self.callable_selection_hint(candidate))
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "callable '{display_name}' is ambiguous; select a {role} entity ID from: {candidate_ids}"
            ))
        }
    }

    fn callable_observation_context_id(&self, callable: &ProgramEntity) -> ObservationContextId {
        self.manifestations
            .iter()
            .find(|manifestation| manifestation.entity_id == callable.id)
            .expect("validated callable must have a manifestation")
            .observation_context_id
            .clone()
    }

    fn correspondence_claims_for_relationships(
        &self,
        relationships: &[CallRelationship],
    ) -> Vec<CorrespondenceClaim> {
        let correspondence_ids = relationships
            .iter()
            .flat_map(|relationship| &relationship.correspondence_claim_ids)
            .map(CorrespondenceClaimId::as_str)
            .collect::<BTreeSet<_>>();
        self.correspondence_claims
            .iter()
            .filter(|claim| correspondence_ids.contains(claim.id.as_str()))
            .cloned()
            .collect()
    }

    pub(crate) fn query_callees(
        &self,
        caller_name: &str,
        caller_entity_id: Option<&ProgramEntityId>,
    ) -> Result<CallRelationshipsResult, String> {
        let caller = self.select_callable(caller_name, caller_entity_id, "caller")?;
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
        Ok(CallRelationshipsResult {
            query_name: "callees".into(),
            program_snapshot_id: self.program_snapshot.id.clone(),
            selected_callable_entity_id: caller.id.clone(),
            selected_callable_observation_context_id: self.callable_observation_context_id(caller),
            correspondence_claims: self.correspondence_claims_for_relationships(&relationships),
            relationships,
            call_sites,
        })
    }

    pub(crate) fn query_callers(
        &self,
        callee_name: &str,
        callee_entity_id: Option<&ProgramEntityId>,
    ) -> Result<CallRelationshipsResult, String> {
        let callee = self.select_callable(callee_name, callee_entity_id, "callee")?;
        let relationships = self
            .call_graph_projection
            .relationships
            .iter()
            .filter(|relationship| relationship.callee_entity_id == callee.id)
            .cloned()
            .collect::<Vec<_>>();
        let call_site_ids = relationships
            .iter()
            .map(|relationship| relationship.call_site_id.as_str())
            .collect::<BTreeSet<_>>();
        let call_sites = self
            .call_graph_projection
            .call_sites
            .iter()
            .filter(|call_site| call_site_ids.contains(call_site.call_site_id.as_str()))
            .cloned()
            .collect();
        Ok(CallRelationshipsResult {
            query_name: "callers".into(),
            program_snapshot_id: self.program_snapshot.id.clone(),
            selected_callable_entity_id: callee.id.clone(),
            selected_callable_observation_context_id: self.callable_observation_context_id(callee),
            correspondence_claims: self.correspondence_claims_for_relationships(&relationships),
            relationships,
            call_sites,
        })
    }

    pub(crate) fn query_call_path(
        &self,
        start_name: &str,
        start_entity_id: Option<&ProgramEntityId>,
        end_name: &str,
        end_entity_id: Option<&ProgramEntityId>,
        max_relationships: usize,
    ) -> Result<CallPathResult, String> {
        const MAX_SUPPORTED_RELATIONSHIPS: usize = 1_000;
        if !(1..=MAX_SUPPORTED_RELATIONSHIPS).contains(&max_relationships) {
            return Err(format!(
                "call-path max relationships must be between 1 and {MAX_SUPPORTED_RELATIONSHIPS}"
            ));
        }
        let start = self.select_callable(start_name, start_entity_id, "start")?;
        let end = self.select_callable(end_name, end_entity_id, "end")?;
        let mut seen = BTreeSet::from([start.id.clone()]);
        let mut queue = VecDeque::from([(start.id.clone(), Vec::new())]);
        let mut found = None;
        while let Some((entity_id, path)) = queue.pop_front() {
            if entity_id == end.id {
                found = Some(path);
                break;
            }
            if path.len() == max_relationships {
                continue;
            }
            for relationship in self
                .call_graph_projection
                .relationships
                .iter()
                .filter(|relationship| relationship.caller_entity_id == entity_id)
            {
                if seen.insert(relationship.callee_entity_id.clone()) {
                    let mut next = path.clone();
                    next.push(relationship.clone());
                    queue.push_back((relationship.callee_entity_id.clone(), next));
                }
            }
        }
        let correspondence_claims = found
            .as_deref()
            .map(|path| self.correspondence_claims_for_relationships(path))
            .unwrap_or_default();
        Ok(CallPathResult {
            query_name: "call-path".into(),
            program_snapshot_id: self.program_snapshot.id.clone(),
            start_entity_id: start.id.clone(),
            end_entity_id: end.id.clone(),
            max_relationships,
            correspondence_claims,
            path: found,
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
    ) -> Result<BTreeMap<(&str, &ContributorCallableIdentity), IdentityEvidenceGroup<'_>>, String>
    {
        let manifestations_by_id: BTreeMap<_, _> = self
            .manifestations
            .iter()
            .map(|manifestation| (manifestation.id.as_str(), manifestation))
            .collect();
        let mut groups: BTreeMap<(&str, &ContributorCallableIdentity), IdentityEvidenceGroup<'_>> =
            BTreeMap::new();
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
                    &manifestation.contributor_callable_identity,
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

    fn validate(&self) -> Result<(), String> {
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
        let mut entity_identities = BTreeMap::new();
        let mut contributor_manifestations = BTreeSet::new();
        // The value type owns each ID's scope; these indexes enforce the
        // cross-manifestation invariants that one raw contributor ID cannot
        // switch scopes and one input-scoped identity cannot span inputs.
        let mut scopes_by_id: BTreeMap<&str, CallableIdentityScope> = BTreeMap::new();
        let mut acquired_input_by_local_identity: BTreeMap<&ContributorCallableIdentity, &str> =
            BTreeMap::new();
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
            // One contributor callable identity means one callable only
            // within the scope its contributor declared for it. An identity
            // scoped to its acquired input names a callable nothing outside
            // that input can be, so it may not manifest in another: two
            // identically named translation-unit-local callables are different
            // callables, and neither the core nor a hand-edited export may
            // join them.
            let identity = &manifestation.contributor_callable_identity;
            match scopes_by_id.entry(identity.as_str()) {
                std::collections::btree_map::Entry::Vacant(slot) => {
                    slot.insert(identity.scope());
                }
                std::collections::btree_map::Entry::Occupied(slot) => {
                    if *slot.get() != identity.scope() {
                        return Err(format!(
                            "contributor callable identity '{}' declares {:?} scope on one manifestation and {:?} on another",
                            identity,
                            slot.get(),
                            identity.scope()
                        ));
                    }
                }
            }
            if identity.scope() == CallableIdentityScope::AcquiredInput {
                if let Some(first_input) = acquired_input_by_local_identity
                    .insert(identity, manifestation.acquired_input_id.as_str())
                {
                    if first_input != manifestation.acquired_input_id.as_str() {
                        return Err(format!(
                            "contributor callable identity '{}' is scoped to acquired input '{first_input}' but also manifests in acquired input '{}'; a callable that is not visible beyond one acquired input is a different callable in another",
                            identity, manifestation.acquired_input_id
                        ));
                    }
                }
            }
            if !contributor_manifestations.insert((
                manifestation.acquired_input_id.as_str(),
                manifestation.observation_context_id.as_str(),
                identity,
            )) {
                return Err(format!(
                    "contributor callable identity '{}' has duplicate manifestations in observation context '{}'",
                    identity, manifestation.observation_context_id
                ));
            }
            match entity_identities.insert(
                manifestation.entity_id.as_str(),
                (manifestation.observation_context_id.as_str(), identity),
            ) {
                Some((existing_context, _))
                    if existing_context != manifestation.observation_context_id.as_str() =>
                {
                    return Err(format!(
                        "program entity '{}' is merged across observation contexts without correspondence evidence",
                        manifestation.entity_id
                    ));
                }
                Some((_, existing_identity)) if existing_identity != identity => {
                    return Err(format!(
                        "program entity '{}' merges distinct contributor callable identities in observation context '{}'",
                        manifestation.entity_id, manifestation.observation_context_id
                    ));
                }
                Some(_) => {}
                None => {}
            }
        }
        for callable in self
            .program_entities
            .iter()
            .filter(|entity| entity.kind == ProgramEntityKind::Callable)
        {
            if !entity_identities.contains_key(callable.id.as_str()) {
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
            if let Some(basis) = &evidence.completeness_basis {
                let subject = format!("evidence '{}'", evidence.id);
                basis.validate(&subject)?;
                if evidence.support != EvidenceSupport::CallSiteResolution {
                    return Err(misplaced_completeness_basis_error(&subject));
                }
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
            if !inputs_by_id.contains_key(claim.acquired_input_id.as_str()) {
                return Err(format!(
                    "correspondence claim '{}' references unknown acquired input '{}'",
                    claim.id, claim.acquired_input_id
                ));
            }
            if !correspondence_references.insert((
                claim.acquired_input_id.as_str(),
                &claim.contributor_callable_identity,
            )) {
                return Err(format!(
                    "contributor callable identity '{}' has duplicate correspondence claims for acquired input '{}'",
                    claim.contributor_callable_identity, claim.acquired_input_id
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
                    || manifestation.contributor_callable_identity
                        != claim.contributor_callable_identity
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
                    &claim.contributor_callable_identity,
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
        for ((acquired_input_id, contributor_callable_identity), group) in &identity_groups {
            if group.observation_context_ids.len() < 2 {
                continue;
            }
            if !self.correspondence_claims.iter().any(|claim| {
                claim.acquired_input_id.as_str() == *acquired_input_id
                    && claim.contributor_callable_identity == **contributor_callable_identity
            }) {
                return Err(format!(
                    "contributor callable identity '{contributor_callable_identity}' has contributor-identity evidence in {} observation contexts of acquired input '{acquired_input_id}' but no correspondence claim",
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
        let mut referenced_resolution_evidence: BTreeMap<&str, usize> = BTreeMap::new();
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
            let mut resolution_evidence_ids = BTreeSet::new();
            let mut declared_completeness = false;
            for evidence_id in &resolution.evidence_ids {
                *referenced_resolution_evidence
                    .entry(evidence_id.as_str())
                    .or_insert(0) += 1;
                if !resolution_evidence_ids.insert(evidence_id.as_str()) {
                    return Err(format!(
                        "resolution for '{}' references evidence '{evidence_id}' more than once",
                        resolution.call_site_id
                    ));
                }
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
                declared_completeness |= evidence.completeness_basis.is_some();
            }
            check_completeness_declaration(
                resolution.resolution,
                declared_completeness,
                &format!("resolution for '{}'", resolution.call_site_id),
            )?;
        }
        for evidence in self
            .evidence_records
            .iter()
            .filter(|evidence| evidence.support == EvidenceSupport::CallSiteResolution)
        {
            match referenced_resolution_evidence
                .get(evidence.id.as_str())
                .copied()
                .unwrap_or_default()
            {
                1 => {}
                0 => {
                    return Err(format!(
                        "call-site-resolution evidence '{}' is not referenced by the resolution of call site '{}'",
                        evidence.id, evidence.subject_entity_id
                    ));
                }
                _ => {
                    return Err(format!(
                        "call-site-resolution evidence '{}' is referenced by more than one call-site resolution",
                        evidence.id
                    ));
                }
            }
        }
        // The manifestations an evidence source declared as callables of its
        // own, keyed by the acquired input and observation context the
        // declaration was read in. A manifestation that exists only because a
        // target claim named it is absent from this index, which is what lets
        // a direct target claim be told from an invented one.
        let declared_callables: BTreeSet<(&str, &str, &str)> = self
            .evidence_records
            .iter()
            .filter(|evidence| evidence.support == EvidenceSupport::ContributorIdentity)
            .flat_map(|evidence| {
                evidence.related_manifestation_ids.iter().map(move |id| {
                    (
                        id.as_str(),
                        evidence.acquired_input_id.as_str(),
                        evidence.observation_context_id.as_str(),
                    )
                })
            })
            .collect();
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
            let mut names_a_direct_call = false;
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
                names_a_direct_call |= evidence.evidence_type == STATIC_DIRECT_CALL_EVIDENCE_TYPE;
            }
            // A direct call names a callable the module itself spells out, so
            // the snapshot has to hold the declaration as well as the claim:
            // the target manifestation is written as one of the callable kinds
            // and is declared by contributor-identity evidence read in the same
            // acquired input and observation context. Target evidence of any
            // other type asserts only that a target was observed among a call
            // site's possible callees, which a manifestation introduced by the
            // claim itself may legitimately be, so this obligation is the
            // direct claim's alone.
            if names_a_direct_call {
                if !DECLARED_CALLABLE_REPRESENTATIONS.contains(&target.representation.as_str()) {
                    return Err(format!(
                        "claim '{}' names manifestation '{}' as a static direct-call target, but its representation '{}' is not a declared callable kind; a direct call names a callable global its evidence source declares",
                        claim.id, target.id, target.representation
                    ));
                }
                if !declared_callables.contains(&(
                    target.id.as_str(),
                    target.acquired_input_id.as_str(),
                    claim.observation_context_id.as_str(),
                )) {
                    return Err(format!(
                        "claim '{}' names manifestation '{}' as a static direct-call target, but no contributor-identity evidence declares that callable in acquired input '{}' and observation context '{}'; a direct call names a callable its evidence source declared, not one the claim itself introduced",
                        claim.id, target.id, target.acquired_input_id, claim.observation_context_id
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
    display_name: String,
    representation: String,
    manifestation_ids_by_input: BTreeMap<AcquiredInputId, ManifestationId>,
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
    caller_callable_identity: ContributorCallableIdentity,
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
    contributor_callable_identity: ContributorCallableIdentity,
    manifestation_ids: Vec<ManifestationId>,
}

type CallableIdentities =
    BTreeMap<ObservationContextId, BTreeMap<ContributorCallableIdentity, CallableIdentity>>;

#[allow(clippy::too_many_arguments)]
fn ensure_callable(
    contributor_callable_identity: &ContributorCallableIdentity,
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
    let context_identities = identities.entry(context_id.clone()).or_default();
    if let Some(identity) = context_identities.get_mut(contributor_callable_identity) {
        if identity.display_name != name || identity.representation != representation {
            return Err(format!(
                "contributed callable identity '{contributor_callable_identity}' has conflicting labels or representations in observation context '{context_id}'"
            ));
        }
        let manifestation_id = if let Some(manifestation_id) =
            identity.manifestation_ids_by_input.get(acquired_input_id)
        {
            manifestation_id.clone()
        } else {
            if contributor_callable_identity.scope() == CallableIdentityScope::AcquiredInput {
                let first_input = identity
                    .manifestation_ids_by_input
                    .keys()
                    .next()
                    .expect("an existing callable identity must have a manifestation");
                return Err(format!(
                    "contributor callable identity '{contributor_callable_identity}' is scoped to acquired input '{first_input}' but also manifests in acquired input '{acquired_input_id}'; a callable that is not visible beyond one acquired input is a different callable in another"
                ));
            }
            let callable_index = manifestations.len();
            let manifestation_id = ManifestationId::new(format!(
                "manifestation:{snapshot_id}:input:{input_index}:callable:{callable_index}"
            ));
            manifestations.push(Manifestation {
                id: manifestation_id.clone(),
                entity_id: identity.entity_id.clone(),
                acquired_input_id: acquired_input_id.clone(),
                contributor_callable_identity: contributor_callable_identity.clone(),
                observation_context_id: context_id.clone(),
                representation: representation.into(),
                defined,
            });
            identity
                .manifestation_ids_by_input
                .insert(acquired_input_id.clone(), manifestation_id.clone());
            manifestation_id
        };
        if defined {
            manifestations
                .iter_mut()
                .find(|manifestation| manifestation.id == manifestation_id)
                .expect("callable manifestation must exist")
                .defined = true;
        }
        return Ok(CallableReference {
            entity_id: identity.entity_id.clone(),
            manifestation_id,
            display_name: identity.display_name.clone(),
        });
    }

    let callable_index = manifestations.len();
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
        contributor_callable_identity: contributor_callable_identity.clone(),
        observation_context_id: context_id.clone(),
        representation: representation.into(),
        defined,
    });
    context_identities.insert(
        contributor_callable_identity.clone(),
        CallableIdentity {
            entity_id: entity_id.clone(),
            display_name: name.into(),
            representation: representation.into(),
            manifestation_ids_by_input: BTreeMap::from([(
                acquired_input_id.clone(),
                manifestation_id.clone(),
            )]),
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
        completeness_basis: contributed.completeness_basis,
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
    // Linkage-namespace identities intentionally live across acquired-input
    // iterations. Their explicit contributor identity evidence makes one
    // program entity, with one manifestation per input. Acquired-input
    // identities are rejected if the same scoped identity reaches a second
    // input.
    let mut identities = BTreeMap::new();
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

        for (callable_index, function) in contribution.callables.into_iter().enumerate() {
            let callable = ensure_callable(
                &function.callable_identity,
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
                    function.callable_identity
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
                            context_identities.get(&call.caller_callable_identity)
                        })
                        .and_then(|identity| {
                            identity
                                .manifestation_ids_by_input
                                .get(&input_id)
                                .map(|manifestation_id| CallableReference {
                                    entity_id: identity.entity_id.clone(),
                                    manifestation_id: manifestation_id.clone(),
                                    display_name: identity.display_name.clone(),
                                })
                        })
                        .ok_or_else(|| {
                            format!(
                                "call-site evidence references uncontributed caller identity '{}' in observation context '{}'",
                                call.caller_callable_identity, call.observation_context_id
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
                            caller_callable_identity: call.caller_callable_identity,
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
                    if published.caller_callable_identity != reference.caller_callable_identity {
                        return Err(format!(
                            "call-site attachment names caller identity '{}' but contributor call-site identity '{}' was contributed by caller '{}'",
                            reference.caller_callable_identity,
                            reference.contributor_call_site_id,
                            published.caller_callable_identity
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
                    &target.target_callable_identity,
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
    }

    let input_indexes = acquired_inputs
        .iter()
        .enumerate()
        .map(|(index, input)| (input.id.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let mut manifestations_by_input_and_identity: BTreeMap<
        (AcquiredInputId, ContributorCallableIdentity),
        Vec<ManifestationId>,
    > = BTreeMap::new();
    for manifestation in &manifestations {
        manifestations_by_input_and_identity
            .entry((
                manifestation.acquired_input_id.clone(),
                manifestation.contributor_callable_identity.clone(),
            ))
            .or_default()
            .push(manifestation.id.clone());
    }
    correspondence_seeds.extend(
        manifestations_by_input_and_identity
            .into_iter()
            .filter(|(_, manifestation_ids)| manifestation_ids.len() > 1)
            .map(
                |((acquired_input_id, contributor_callable_identity), manifestation_ids)| {
                    CorrespondenceSeed {
                        input_index: input_indexes[&acquired_input_id],
                        acquired_input_id,
                        contributor_callable_identity,
                        manifestation_ids,
                    }
                },
            ),
    );

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
            contributor_callable_identity: seed.contributor_callable_identity,
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
