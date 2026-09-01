mod analysis;
pub mod app;
mod contributor;
mod llvm;
mod model;
mod snapshot;
mod viewer;

pub use contributor::{
    ContributedCallable, ContributedDirectCall, ContributedInput, ContributorIdentity,
    EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability, EvidenceContribution,
    EvidenceContributor,
};
pub use llvm::LlvmTextContributor;
pub use model::{AnalysisSummary, Document, Edge, Metadata, Node};
pub use snapshot::{
    AcquiredInput, AcquiredInputId, CallGraphProjection, CallRelationship, Derivation, EvidenceId,
    EvidenceRecord, Explanation, ExplanationHandle, Manifestation, ManifestationId,
    NamedQueryResult, ObservationContext, ObservationContextId, ProgramEntity, ProgramEntityId,
    ProgramEntityKind, ProgramSnapshot, ProgramSnapshotId, PublishedSnapshot, Resolution,
    SNAPSHOT_SCHEMA_VERSION, SourceLocation, TargetClaim, TargetClaimId,
};
