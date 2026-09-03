use gloom::app::{Application, NamedQuery};
use gloom::{
    ContributedCallKind, ContributedCallSite, ContributedCallSiteAttachment,
    ContributedCallSiteReference, ContributedCallable, ContributedEvidence,
    ContributedEvidenceLocation, ContributedInput, ContributedResolutionRevision,
    ContributedTargetClaim, ContributorIdentity, EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION,
    EvidenceCapability, EvidenceContribution, EvidenceContributor, EvidenceScope, EvidenceSupport,
    ObservationContext, ObservationContextId, ProgramEntityKind, PublishedSnapshot, Resolution,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const STATIC_INPUT: &str = "static-analysis.fixture";
const OTHER_STATIC_INPUT: &str = "other-static-analysis.fixture";
const TRACE_INPUT: &str = "runtime-trace.fixture";
const REFINEMENT_INPUT: &str = "whole-program-refinement.fixture";
const CALL_SITE_IDENTITY: &str = "dispatch#0";
const CALL_SITE_LINE: usize = 7;
const TRACE_LINE: usize = 42;
const REFINEMENT_LINE: usize = 3;

fn publication_context(contributor: &str) -> ObservationContext {
    ObservationContext::static_analysis(
        "snapshot:evidence-provenance",
        "evidence-provenance-fixture",
        "debug fixture",
        "semantic fixture",
        contributor,
        "1",
        "static analysis",
    )
}

fn runtime_context(context: &ObservationContext) -> ObservationContext {
    ObservationContext::runtime_analysis(
        context.program_snapshot_id.as_str(),
        context.build_target.clone(),
        context.build_configuration.clone(),
        context.toolchain.clone(),
        context.extraction_method.clone(),
        context.extraction_version.clone(),
        "runtime target tracing",
        "trace fixture workload",
    )
}

fn identity(name: &str) -> ContributorIdentity {
    ContributorIdentity {
        name: name.into(),
        version: "1".into(),
        contract_version: EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION.into(),
        capabilities: vec![
            EvidenceCapability::CallableManifestations,
            EvidenceCapability::IndirectCallEvidence,
        ],
    }
}

fn evidence(
    evidence_type: &str,
    scope: EvidenceScope,
    support: EvidenceSupport,
    evidence_artifact: &str,
    line: usize,
) -> ContributedEvidence {
    ContributedEvidence {
        evidence_type: evidence_type.into(),
        scope,
        support,
        location: ContributedEvidenceLocation {
            evidence_artifact: evidence_artifact.into(),
            line,
        },
    }
}

fn callable(
    name: &str,
    representation: &str,
    observation_context_id: &ObservationContextId,
) -> ContributedCallable {
    ContributedCallable {
        contributor_callable_id: name.into(),
        display_name: name.into(),
        defined: true,
        representation: representation.into(),
        observation_context_id: observation_context_id.clone(),
    }
}

fn target_claim(
    name: &str,
    representation: &str,
    observation_context_id: &ObservationContextId,
    evidence: Vec<ContributedEvidence>,
) -> ContributedTargetClaim {
    ContributedTargetClaim {
        target_callable_id: name.into(),
        callee_display_name: name.into(),
        target_representation: representation.into(),
        observation_context_id: observation_context_id.clone(),
        evidence,
    }
}

fn contributed_input(input: &Path, acquisition_method: &str) -> ContributedInput {
    ContributedInput {
        path: input.display().to_string(),
        evidence_artifact: input.display().to_string(),
        media_type: "application/x-gloom-fixture".into(),
        acquisition_method: acquisition_method.into(),
        content_fingerprint: format!("fixture:{}", input.display()),
    }
}

/// One statically observed indirect call site with one statically possible
/// target. Both acquired inputs of the ambiguity fixture use it, so the same
/// contributor call-site identity is published twice.
fn static_contribution(input: &Path, context: &ObservationContext) -> EvidenceContribution {
    let artifact = input.display().to_string();
    EvidenceContribution {
        input: contributed_input(input, "semantic-fixture"),
        observation_contexts: vec![context.clone()],
        callables: vec![
            callable("dispatch", "fixture-callable", &context.id),
            callable("first_target", "fixture-callable", &context.id),
        ],
        call_sites: vec![ContributedCallSite {
            contributor_call_site_id: CALL_SITE_IDENTITY.into(),
            kind: ContributedCallKind::Indirect,
            caller_callable_id: "dispatch".into(),
            line: CALL_SITE_LINE,
            observation_context_id: context.id.clone(),
            resolution: Resolution::Partial,
            evidence: evidence(
                "static-indirect-call",
                EvidenceScope::Static,
                EvidenceSupport::CallSiteResolution,
                &artifact,
                CALL_SITE_LINE,
            ),
            target_claims: vec![target_claim(
                "first_target",
                "fixture-callable",
                &context.id,
                vec![evidence(
                    "static-possible-target",
                    EvidenceScope::Static,
                    EvidenceSupport::TargetClaim,
                    &artifact,
                    CALL_SITE_LINE,
                )],
            )],
        }],
        call_site_attachments: Vec::new(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TraceAttachment {
    /// The trace names the call site the static input published.
    Attached,
    /// The trace names a call-site identity nobody contributed.
    UnknownCallSiteIdentity,
    /// The trace names a real call-site identity but the wrong caller.
    MismatchedCaller,
    /// The trace cites provenance in an artifact it never acquired.
    ForeignArtifactProvenance,
    /// The trace tries to revise a static target-set resolution.
    RuntimeResolutionRevision,
}

struct StaticAndTraceFixture {
    attachment: TraceAttachment,
}

impl StaticAndTraceFixture {
    const NAME: &'static str = "fixture.static-and-trace";

    fn trace_contribution(
        &self,
        input: &Path,
        context: &ObservationContext,
    ) -> EvidenceContribution {
        let artifact = input.display().to_string();
        let runtime = runtime_context(context);
        let claim_artifact = match self.attachment {
            TraceAttachment::ForeignArtifactProvenance => STATIC_INPUT.to_string(),
            _ => artifact.clone(),
        };
        EvidenceContribution {
            input: contributed_input(input, "runtime-trace"),
            observation_contexts: vec![context.clone(), runtime.clone()],
            callables: vec![
                callable("first_target", "traced-callable", &runtime.id),
                callable("second_target", "traced-callable", &runtime.id),
            ],
            call_sites: Vec::new(),
            call_site_attachments: vec![ContributedCallSiteAttachment {
                call_site: ContributedCallSiteReference {
                    observation_context_id: context.id.clone(),
                    caller_callable_id: match self.attachment {
                        TraceAttachment::MismatchedCaller => "first_target".into(),
                        _ => "dispatch".into(),
                    },
                    contributor_call_site_id: match self.attachment {
                        TraceAttachment::UnknownCallSiteIdentity => "dispatch#9".into(),
                        _ => CALL_SITE_IDENTITY.into(),
                    },
                },
                resolution_revision: match self.attachment {
                    TraceAttachment::RuntimeResolutionRevision => {
                        Some(ContributedResolutionRevision {
                            resolution: Resolution::Complete,
                            evidence: evidence(
                                "runtime-observed-target-set",
                                EvidenceScope::Runtime,
                                EvidenceSupport::CallSiteResolution,
                                &artifact,
                                TRACE_LINE,
                            ),
                        })
                    }
                    _ => None,
                },
                target_claims: vec![
                    target_claim(
                        "first_target",
                        "traced-callable",
                        &runtime.id,
                        vec![evidence(
                            "runtime-observed-target",
                            EvidenceScope::Runtime,
                            EvidenceSupport::TargetClaim,
                            &claim_artifact,
                            TRACE_LINE,
                        )],
                    ),
                    target_claim(
                        "second_target",
                        "traced-callable",
                        &runtime.id,
                        vec![evidence(
                            "runtime-observed-target",
                            EvidenceScope::Runtime,
                            EvidenceSupport::TargetClaim,
                            &claim_artifact,
                            TRACE_LINE + 1,
                        )],
                    ),
                ],
            }],
        }
    }
}

impl EvidenceContributor for StaticAndTraceFixture {
    fn identity(&self) -> ContributorIdentity {
        identity(Self::NAME)
    }

    fn contribute(
        &self,
        input: &Path,
        context: &ObservationContext,
    ) -> Result<EvidenceContribution, String> {
        if input.ends_with(TRACE_INPUT) {
            Ok(self.trace_contribution(input, context))
        } else {
            Ok(static_contribution(input, context))
        }
    }
}

/// A second static acquired input whose whole-program reasoning adds a target
/// to the published call site and declares its target set complete.
struct WholeProgramRefinementFixture;

impl WholeProgramRefinementFixture {
    const NAME: &'static str = "fixture.whole-program-refinement";
}

impl EvidenceContributor for WholeProgramRefinementFixture {
    fn identity(&self) -> ContributorIdentity {
        identity(Self::NAME)
    }

    fn contribute(
        &self,
        input: &Path,
        context: &ObservationContext,
    ) -> Result<EvidenceContribution, String> {
        if !input.ends_with(REFINEMENT_INPUT) {
            return Ok(static_contribution(input, context));
        }
        let artifact = input.display().to_string();
        Ok(EvidenceContribution {
            input: contributed_input(input, "semantic-fixture"),
            observation_contexts: vec![context.clone()],
            callables: vec![callable("second_target", "refined-callable", &context.id)],
            call_sites: Vec::new(),
            call_site_attachments: vec![ContributedCallSiteAttachment {
                call_site: ContributedCallSiteReference {
                    observation_context_id: context.id.clone(),
                    caller_callable_id: "dispatch".into(),
                    contributor_call_site_id: CALL_SITE_IDENTITY.into(),
                },
                resolution_revision: Some(ContributedResolutionRevision {
                    resolution: Resolution::Complete,
                    evidence: evidence(
                        "whole-program-target-set",
                        EvidenceScope::Static,
                        EvidenceSupport::CallSiteResolution,
                        &artifact,
                        REFINEMENT_LINE,
                    ),
                }),
                target_claims: vec![target_claim(
                    "second_target",
                    "refined-callable",
                    &context.id,
                    vec![evidence(
                        "whole-program-possible-target",
                        EvidenceScope::Static,
                        EvidenceSupport::TargetClaim,
                        &artifact,
                        REFINEMENT_LINE,
                    )],
                )],
            }],
        })
    }
}

/// Two acquired inputs that publish the same contributor call-site identity in
/// the same observation context, so a later reference cannot pick one.
struct DuplicateCallSiteIdentityFixture;

impl DuplicateCallSiteIdentityFixture {
    const NAME: &'static str = "fixture.duplicate-call-site-identity";
}

impl EvidenceContributor for DuplicateCallSiteIdentityFixture {
    fn identity(&self) -> ContributorIdentity {
        identity(Self::NAME)
    }

    fn contribute(
        &self,
        input: &Path,
        context: &ObservationContext,
    ) -> Result<EvidenceContribution, String> {
        if input.ends_with(TRACE_INPUT) {
            Ok(StaticAndTraceFixture {
                attachment: TraceAttachment::Attached,
            }
            .trace_contribution(input, context))
        } else {
            Ok(static_contribution(input, context))
        }
    }
}

fn publish_static_and_trace(attachment: TraceAttachment) -> Result<PublishedSnapshot, String> {
    Application.publish_snapshot(
        &[PathBuf::from(STATIC_INPUT), PathBuf::from(TRACE_INPUT)],
        publication_context(StaticAndTraceFixture::NAME),
        &StaticAndTraceFixture { attachment },
    )
}

fn located_evidence(snapshot: &PublishedSnapshot, evidence_type: &str) -> Vec<(String, usize)> {
    snapshot
        .evidence_records()
        .iter()
        .filter(|record| record.evidence_type == evidence_type)
        .map(|record| {
            let input = snapshot
                .acquired_inputs()
                .iter()
                .find(|input| input.id == record.acquired_input_id)
                .expect("evidence must name a published acquired input");
            assert_eq!(record.source_location.input_id, input.id);
            assert_eq!(record.source_location.artifact, input.evidence_artifact);
            (input.path.clone(), record.source_location.line)
        })
        .collect()
}

#[test]
fn runtime_target_evidence_keeps_the_traces_acquired_input_and_location() {
    let application = Application;
    let snapshot = publish_static_and_trace(TraceAttachment::Attached).unwrap();

    // The call site itself stays located in the static artifact it was read
    // from, while the runtime evidence attached to it keeps the trace's input
    // and line rather than inheriting the call site's.
    let call_site = snapshot
        .program_entities()
        .iter()
        .find(|entity| entity.kind == ProgramEntityKind::CallSite)
        .unwrap();
    let call_site_location = call_site.source_location.as_ref().unwrap();
    assert_eq!(call_site_location.artifact, STATIC_INPUT);
    assert_eq!(call_site_location.line, CALL_SITE_LINE);

    assert_eq!(
        located_evidence(&snapshot, "static-possible-target"),
        [(STATIC_INPUT.to_string(), CALL_SITE_LINE)]
    );
    assert_eq!(
        located_evidence(&snapshot, "static-indirect-call"),
        [(STATIC_INPUT.to_string(), CALL_SITE_LINE)]
    );
    assert_eq!(
        located_evidence(&snapshot, "runtime-observed-target"),
        [
            (TRACE_INPUT.to_string(), TRACE_LINE),
            (TRACE_INPUT.to_string(), TRACE_LINE + 1)
        ]
    );

    let exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let trace_input_id = snapshot
        .acquired_inputs()
        .iter()
        .find(|input| input.path == TRACE_INPUT)
        .unwrap()
        .id
        .clone();
    let exported_runtime_evidence = exported["evidence_records"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|record| record["evidence_type"] == serde_json::json!("runtime-observed-target"))
        .collect::<Vec<_>>();
    assert_eq!(exported_runtime_evidence.len(), 2);
    assert!(exported_runtime_evidence.iter().all(|record| {
        record["acquired_input_id"] == serde_json::json!(trace_input_id)
            && record["source_location"]["input_id"] == serde_json::json!(trace_input_id)
            && record["source_location"]["artifact"] == serde_json::json!(TRACE_INPUT)
    }));

    application
        .load_snapshot_json(&application.export_snapshot_json(&snapshot).unwrap())
        .unwrap();
}

#[test]
fn a_trace_acquired_later_attaches_targets_to_the_published_call_site() {
    let application = Application;
    let snapshot = publish_static_and_trace(TraceAttachment::Attached).unwrap();

    // One call-site entity, even though two acquired inputs contributed
    // evidence about it.
    let call_sites = snapshot
        .program_entities()
        .iter()
        .filter(|entity| entity.kind == ProgramEntityKind::CallSite)
        .collect::<Vec<_>>();
    assert_eq!(call_sites.len(), 1);
    assert_eq!(snapshot.call_site_resolutions().len(), 1);

    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "dispatch".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    assert_eq!(result.call_sites.len(), 1);
    let projected = &result.call_sites[0];
    assert_eq!(projected.call_site_id, call_sites[0].id);
    assert_eq!(projected.targets.len(), 3);
    assert_eq!(
        projected
            .targets
            .iter()
            .map(|target| target.callee_display_name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["first_target", "second_target"])
    );

    // The trace said nothing about the target set, so the resolution the
    // static input published is unchanged.
    let static_context = publication_context(StaticAndTraceFixture::NAME);
    assert_eq!(projected.resolution, Resolution::Partial);
    assert_eq!(
        projected.resolution_observation_context_id,
        static_context.id
    );
    assert!(
        result
            .relationships
            .iter()
            .all(|relationship| relationship.resolution == Resolution::Partial)
    );

    // Target claims come from both acquired inputs and keep their own
    // observation contexts.
    let runtime_context_id = runtime_context(&static_context).id;
    let target_contexts = projected
        .targets
        .iter()
        .map(|target| target.target_observation_context_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        target_contexts,
        BTreeSet::from([static_context.id.as_str(), runtime_context_id.as_str()])
    );
    let target_inputs = snapshot
        .target_claims()
        .iter()
        .map(|claim| {
            let evidence = snapshot
                .evidence_records()
                .iter()
                .find(|record| claim.evidence_ids.contains(&record.id))
                .unwrap();
            snapshot
                .acquired_inputs()
                .iter()
                .find(|input| input.id == evidence.acquired_input_id)
                .unwrap()
                .path
                .as_str()
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(target_inputs, BTreeSet::from([STATIC_INPUT, TRACE_INPUT]));

    // `first_target` is both statically possible and dynamically observed, in
    // distinct manifestations that are never collapsed into one entity.
    let first_target_entities = projected
        .targets
        .iter()
        .filter(|target| target.callee_display_name == "first_target")
        .map(|target| target.callee_entity_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(first_target_entities.len(), 2);
}

#[test]
fn explanations_show_static_and_runtime_provenance_side_by_side() {
    let application = Application;
    let snapshot = publish_static_and_trace(TraceAttachment::Attached).unwrap();
    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "dispatch".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();

    let explanation = application
        .explain_snapshot(&snapshot, &result.call_sites[0].explanation_handle)
        .unwrap();
    assert_eq!(explanation.target_claims.len(), 3);
    let provenance = explanation
        .evidence_records
        .iter()
        .map(|record| {
            (
                record.evidence_type.as_str(),
                record.source_location.artifact.as_str(),
                record.source_location.line,
            )
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        provenance,
        BTreeSet::from([
            ("static-indirect-call", STATIC_INPUT, CALL_SITE_LINE),
            ("static-possible-target", STATIC_INPUT, CALL_SITE_LINE),
            ("runtime-observed-target", TRACE_INPUT, TRACE_LINE),
            ("runtime-observed-target", TRACE_INPUT, TRACE_LINE + 1),
        ])
    );
    let explained_inputs = explanation
        .evidence_records
        .iter()
        .map(|record| record.acquired_input_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(explained_inputs.len(), 2);

    // The viewer renders the same explanation, so both provenances reach the
    // projection client without it reinterpreting them.
    let html = application.render_snapshot_viewer(&snapshot).unwrap();
    assert!(html.contains(TRACE_INPUT));
    assert!(html.contains(STATIC_INPUT));
}

#[test]
fn unknown_or_ambiguous_call_site_references_are_rejected() {
    let unknown = publish_static_and_trace(TraceAttachment::UnknownCallSiteIdentity).unwrap_err();
    assert!(
        unknown.contains("unknown contributor call-site identity 'dispatch#9'"),
        "unexpected error: {unknown}"
    );

    let mismatched = publish_static_and_trace(TraceAttachment::MismatchedCaller).unwrap_err();
    assert!(
        mismatched.contains("names caller identity 'first_target'")
            && mismatched.contains("was contributed by caller 'dispatch'"),
        "unexpected error: {mismatched}"
    );

    let ambiguous = Application
        .publish_snapshot(
            &[
                PathBuf::from(STATIC_INPUT),
                PathBuf::from(OTHER_STATIC_INPUT),
                PathBuf::from(TRACE_INPUT),
            ],
            publication_context(DuplicateCallSiteIdentityFixture::NAME),
            &DuplicateCallSiteIdentityFixture,
        )
        .unwrap_err();
    assert!(
        ambiguous.contains("ambiguous contributor call-site identity 'dispatch#0'"),
        "unexpected error: {ambiguous}"
    );
}

#[test]
fn contributed_evidence_must_cite_the_acquired_input_it_was_read_from() {
    let error = publish_static_and_trace(TraceAttachment::ForeignArtifactProvenance).unwrap_err();
    assert!(
        error.contains("claims provenance in evidence artifact")
            && error.contains(STATIC_INPUT)
            && error.contains(TRACE_INPUT),
        "unexpected error: {error}"
    );
}

#[test]
fn runtime_evidence_cannot_revise_a_static_target_set_resolution() {
    let error = publish_static_and_trace(TraceAttachment::RuntimeResolutionRevision).unwrap_err();
    assert!(
        error.contains("Runtime") && error.contains("incompatible with observation context"),
        "unexpected error: {error}"
    );
}

#[test]
fn a_later_acquired_input_may_revise_the_published_target_set_resolution() {
    let application = Application;
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from(STATIC_INPUT), PathBuf::from(REFINEMENT_INPUT)],
            publication_context(WholeProgramRefinementFixture::NAME),
            &WholeProgramRefinementFixture,
        )
        .unwrap();

    let resolution = &snapshot.call_site_resolutions()[0];
    assert_eq!(resolution.resolution, Resolution::Complete);
    // Refinement adds evidence rather than replacing it: the static input's
    // account of the call site survives alongside the revision.
    assert_eq!(resolution.evidence_ids.len(), 2);
    assert_eq!(
        located_evidence(&snapshot, "whole-program-target-set"),
        [(REFINEMENT_INPUT.to_string(), REFINEMENT_LINE)]
    );

    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "dispatch".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    assert_eq!(result.call_sites[0].resolution, Resolution::Complete);
    assert_eq!(result.call_sites[0].targets.len(), 2);
    assert!(
        result
            .relationships
            .iter()
            .all(|relationship| relationship.resolution == Resolution::Complete)
    );
    application
        .load_snapshot_json(&application.export_snapshot_json(&snapshot).unwrap())
        .unwrap();
}

#[test]
fn hand_edited_evidence_provenance_is_rejected_on_load() {
    let application = Application;
    let snapshot = publish_static_and_trace(TraceAttachment::Attached).unwrap();
    let exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let runtime_evidence_index = exported["evidence_records"]
        .as_array()
        .unwrap()
        .iter()
        .position(|record| record["evidence_type"] == serde_json::json!("runtime-observed-target"))
        .unwrap();
    let static_input_id = snapshot
        .acquired_inputs()
        .iter()
        .find(|input| input.path == STATIC_INPUT)
        .unwrap()
        .id
        .clone();
    let load = |value: &serde_json::Value| {
        application
            .load_snapshot_json(&serde_json::to_string(value).unwrap())
            .unwrap_err()
    };

    let mut unknown_input = exported.clone();
    unknown_input["evidence_records"][runtime_evidence_index]["acquired_input_id"] =
        serde_json::json!("input:snapshot:evidence-provenance:9");
    unknown_input["evidence_records"][runtime_evidence_index]["source_location"]["input_id"] =
        serde_json::json!("input:snapshot:evidence-provenance:9");
    let error = load(&unknown_input);
    assert!(
        error.contains("references unknown acquired input"),
        "unexpected error: {error}"
    );

    let mut foreign_location = exported.clone();
    foreign_location["evidence_records"][runtime_evidence_index]["source_location"]["input_id"] =
        serde_json::json!(static_input_id);
    let error = load(&foreign_location);
    assert!(
        error.contains("source location in another acquired input"),
        "unexpected error: {error}"
    );

    let mut wrong_artifact = exported.clone();
    wrong_artifact["evidence_records"][runtime_evidence_index]["source_location"]["artifact"] =
        serde_json::json!(STATIC_INPUT);
    let error = load(&wrong_artifact);
    assert!(
        error.contains("does not identify its acquired evidence artifact"),
        "unexpected error: {error}"
    );

    // Resolution evidence read in the same acquired input as the call site
    // still has to agree with where that call site is.
    let resolution_evidence_index = exported["evidence_records"]
        .as_array()
        .unwrap()
        .iter()
        .position(|record| record["evidence_type"] == serde_json::json!("static-indirect-call"))
        .unwrap();
    let mut moved_resolution_evidence = exported;
    moved_resolution_evidence["evidence_records"][resolution_evidence_index]["source_location"]["line"] =
        serde_json::json!(CALL_SITE_LINE + 100);
    let error = load(&moved_resolution_evidence);
    assert!(
        error.contains("disagree about the call-site location"),
        "unexpected error: {error}"
    );
}
