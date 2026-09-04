mod analysis;
pub mod app;
mod contributor;
mod llvm;
mod model;
mod snapshot;
mod viewer;

pub use contributor::{
    ContributedCallKind, ContributedCallSite, ContributedCallSiteAttachment,
    ContributedCallSiteReference, ContributedCallable, ContributedEvidence,
    ContributedEvidenceLocation, ContributedInput, ContributedTargetClaim, ContributorCallSiteId,
    ContributorIdentity, EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability,
    EvidenceContribution, EvidenceContributor,
};
pub use llvm::LlvmTextContributor;
pub use model::{AnalysisSummary, Document, Edge, Metadata, Node};
pub use snapshot::{
    AcquiredInput, AcquiredInputId, CONTRIBUTED_EVIDENCE_TARGET_RULE,
    CONTRIBUTOR_IDENTITY_CORRESPONDENCE_RULE, CallGraphProjection, CallRelationship,
    CallSiteResolution, CompletenessBasis, CorrespondenceClaim, CorrespondenceClaimId,
    DECLARED_CALLABLE_REPRESENTATIONS, Derivation, EvidenceId, EvidenceRecord, EvidenceScope,
    EvidenceSupport, Explanation, ExplanationHandle, LLVM_ALIAS_REPRESENTATION,
    LLVM_FUNCTION_REPRESENTATION, LLVM_IFUNC_REPRESENTATION, Manifestation, ManifestationId,
    NamedQueryResult, ObservationContext, ObservationContextId, ProgramEntity, ProgramEntityId,
    ProgramEntityKind, ProgramSnapshot, ProgramSnapshotId, ProjectedCallSite, ProjectedCallTarget,
    PublishedSnapshot, Resolution, SNAPSHOT_SCHEMA_VERSION, STATIC_DIRECT_CALL_EVIDENCE_TYPE,
    SourceLocation, TargetClaim, TargetClaimId,
};
