mod analysis;
pub mod app;
mod llvm;
mod model;
mod viewer;

pub use model::{
    AcquiredInput, AcquisitionContext, AnalysisSummary, CallGraphProjection, CallSite, Document,
    Edge, EvidenceContributorMetadata, EvidenceRecord, Knowledge, Manifestation, Metadata, Node,
    ObservationContext, ProgramEntity, ProjectedCall, PublishedSnapshot, TargetClaim,
};
