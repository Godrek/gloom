use gloom::app::{Application, NamedQuery};
use gloom::{
    ContributedCallKind, ContributedCallSite, ContributedCallSiteAttachment,
    ContributedCallSiteReference, ContributedCallable, ContributedEvidence,
    ContributedEvidenceLocation, ContributedInput, ContributedTargetClaim, ContributorCallSiteId,
    ContributorIdentity, EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability,
    EvidenceContribution, EvidenceContributor, EvidenceScope, EvidenceSupport, LlvmTextContributor,
    ObservationContext, ObservationContextId, ProgramEntityKind, PublishedSnapshot, Resolution,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const STATIC_INPUT: &str = "static-analysis.fixture";
const TRACE_INPUT: &str = "runtime-trace.fixture";
const STATIC_FINGERPRINT: &str = "fixture:static-analysis";
const TRACE_FINGERPRINT: &str = "fixture:runtime-trace";
const CALL_SITE_IDENTITY: &str = "dispatch#0";
const CALL_SITE_LINE: usize = 7;
const TRACE_LINE: usize = 42;
const FIRST_LLVM_INPUT: &str = "tests/fixtures/single-call-first.ll";
const SECOND_LLVM_INPUT: &str = "tests/fixtures/single-call-second.ll";

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
            EvidenceCapability::DirectCallEvidence,
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
        completeness_basis: None,
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
    scope: EvidenceScope,
    evidence_artifact: &str,
    line: usize,
) -> ContributedCallable {
    ContributedCallable {
        contributor_callable_id: name.into(),
        display_name: name.into(),
        defined: true,
        representation: representation.into(),
        observation_context_id: observation_context_id.clone(),
        line,
        // Identity evidence is read where the callable is declared: the same
        // acquired input, at the callable's own line.
        identity_evidence: evidence(
            "contributed-callable-identity",
            scope,
            EvidenceSupport::ContributorIdentity,
            evidence_artifact,
            line,
        ),
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

fn contributed_input(
    input: &Path,
    acquisition_method: &str,
    content_fingerprint: &str,
) -> ContributedInput {
    ContributedInput {
        path: input.display().to_string(),
        evidence_artifact: input.display().to_string(),
        media_type: "application/x-gloom-fixture".into(),
        acquisition_method: acquisition_method.into(),
        content_fingerprint: content_fingerprint.into(),
    }
}

/// One statically observed indirect call site with one statically possible
/// target.
fn static_contribution(input: &Path, context: &ObservationContext) -> EvidenceContribution {
    let artifact = input.display().to_string();
    EvidenceContribution {
        input: contributed_input(input, "semantic-fixture", STATIC_FINGERPRINT),
        observation_contexts: vec![context.clone()],
        callables: vec![
            callable(
                "dispatch",
                "fixture-callable",
                &context.id,
                EvidenceScope::Static,
                &artifact,
                1,
            ),
            callable(
                "first_target",
                "fixture-callable",
                &context.id,
                EvidenceScope::Static,
                &artifact,
                2,
            ),
        ],
        call_sites: vec![ContributedCallSite {
            contributor_call_site_id: ContributorCallSiteId::new(CALL_SITE_IDENTITY).unwrap(),
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
    /// The trace names a real call-site identity but the wrong acquired input.
    UnknownAcquiredInput,
    /// The trace names a real call-site identity but the wrong caller.
    MismatchedCaller,
    /// The trace cites provenance in an artifact it never acquired.
    ForeignArtifactProvenance,
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
            input: contributed_input(input, "runtime-trace", TRACE_FINGERPRINT),
            observation_contexts: vec![context.clone(), runtime.clone()],
            callables: vec![
                callable(
                    "first_target",
                    "traced-callable",
                    &runtime.id,
                    EvidenceScope::Runtime,
                    &artifact,
                    11,
                ),
                callable(
                    "second_target",
                    "traced-callable",
                    &runtime.id,
                    EvidenceScope::Runtime,
                    &artifact,
                    12,
                ),
            ],
            call_sites: Vec::new(),
            call_site_attachments: vec![ContributedCallSiteAttachment {
                call_site: ContributedCallSiteReference {
                    observation_context_id: context.id.clone(),
                    acquired_input_fingerprint: match self.attachment {
                        TraceAttachment::UnknownAcquiredInput => "fixture:never-acquired".into(),
                        _ => STATIC_FINGERPRINT.into(),
                    },
                    caller_callable_id: match self.attachment {
                        TraceAttachment::MismatchedCaller => "first_target".into(),
                        _ => "dispatch".into(),
                    },
                    contributor_call_site_id: match self.attachment {
                        TraceAttachment::UnknownCallSiteIdentity => {
                            ContributorCallSiteId::new("dispatch#9").unwrap()
                        }
                        _ => ContributorCallSiteId::new(CALL_SITE_IDENTITY).unwrap(),
                    },
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

/// Two textual LLVM IR inputs plus a trace. The LLVM contributor names call
/// sites per artifact, so both `.ll` inputs publish `llvm-call:0`; the trace
/// picks one by the acquired input's content fingerprint.
struct LlvmAndTraceFixture {
    fingerprints: RefCell<BTreeMap<String, String>>,
    attach_to: &'static str,
    caller_callable_id: &'static str,
    unknown_acquired_input: bool,
}

impl LlvmAndTraceFixture {
    const NAME: &'static str = "fixture.llvm-and-trace";

    fn new(attach_to: &'static str, caller_callable_id: &'static str) -> Self {
        Self {
            fingerprints: RefCell::new(BTreeMap::new()),
            attach_to,
            caller_callable_id,
            unknown_acquired_input: false,
        }
    }

    fn with_unknown_acquired_input(mut self) -> Self {
        self.unknown_acquired_input = true;
        self
    }
}

impl EvidenceContributor for LlvmAndTraceFixture {
    fn identity(&self) -> ContributorIdentity {
        identity(Self::NAME)
    }

    fn contribute(
        &self,
        input: &Path,
        context: &ObservationContext,
    ) -> Result<EvidenceContribution, String> {
        if !input.ends_with(TRACE_INPUT) {
            let contribution = LlvmTextContributor::new("clang", &[]).contribute(input, context)?;
            // A contributor that later acquires a trace knows which artifact
            // the trace belongs to by the fingerprint it computed for it.
            self.fingerprints.borrow_mut().insert(
                input.display().to_string(),
                contribution.input.content_fingerprint.clone(),
            );
            return Ok(contribution);
        }
        let artifact = input.display().to_string();
        let runtime = runtime_context(context);
        let acquired_input_fingerprint = if self.unknown_acquired_input {
            "fnv1a64:0000000000000000".to_string()
        } else {
            self.fingerprints
                .borrow()
                .get(self.attach_to)
                .cloned()
                .expect("the attached artifact must be acquired before the trace")
        };
        Ok(EvidenceContribution {
            input: contributed_input(input, "runtime-trace", TRACE_FINGERPRINT),
            observation_contexts: vec![context.clone(), runtime.clone()],
            callables: vec![callable(
                "traced_callee",
                "traced-callable",
                &runtime.id,
                EvidenceScope::Runtime,
                &artifact,
                11,
            )],
            call_sites: Vec::new(),
            call_site_attachments: vec![ContributedCallSiteAttachment {
                call_site: ContributedCallSiteReference {
                    observation_context_id: context.id.clone(),
                    acquired_input_fingerprint,
                    caller_callable_id: self.caller_callable_id.into(),
                    contributor_call_site_id: ContributorCallSiteId::new("llvm-call:0").unwrap(),
                },
                target_claims: vec![target_claim(
                    "traced_callee",
                    "traced-callable",
                    &runtime.id,
                    vec![evidence(
                        "runtime-observed-target",
                        EvidenceScope::Runtime,
                        EvidenceSupport::TargetClaim,
                        &artifact,
                        TRACE_LINE,
                    )],
                )],
            }],
        })
    }
}

fn publish_static_and_trace(attachment: TraceAttachment) -> Result<PublishedSnapshot, String> {
    Application.publish_snapshot(
        &[PathBuf::from(STATIC_INPUT), PathBuf::from(TRACE_INPUT)],
        publication_context(StaticAndTraceFixture::NAME),
        &StaticAndTraceFixture { attachment },
    )
}

/// Rewrites every reference to one evidence identity in an exported snapshot,
/// so a hand-edited record stays internally consistent and the load has to
/// reject it on provenance grounds rather than on a dangling reference.
fn rename_evidence_reference(
    exported: &mut serde_json::Value,
    from: &serde_json::Value,
    to: &serde_json::Value,
) {
    for claim in exported["target_claims"].as_array_mut().unwrap() {
        for id in claim["evidence_ids"].as_array_mut().unwrap() {
            if id == from {
                *id = to.clone();
            }
        }
    }
    for derivation in exported["derivations"].as_array_mut().unwrap() {
        for id in derivation["input_evidence_ids"].as_array_mut().unwrap() {
            if id == from {
                *id = to.clone();
            }
        }
    }
    for resolution in exported["call_site_resolutions"].as_array_mut().unwrap() {
        for id in resolution["evidence_ids"].as_array_mut().unwrap() {
            if id == from {
                *id = to.clone();
            }
        }
    }
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

    // The viewer builds its evidence rows in the browser from the embedded
    // explanation, so the rendered row is asserted in two parts: the evidence
    // <dd> renders each record's own artifact, line, and acquired input, and
    // the embedded record carries the trace provenance those fields bind to.
    let html = application.render_snapshot_viewer(&snapshot).unwrap();
    let evidence_row = html
        .split("<dt>Evidence</dt><dd>${evidence}</dd>")
        .next()
        .unwrap();
    assert!(
        evidence_row.contains(
            "Provenance: <code>${escapeHtml(record.source_location.artifact)}:${record.source_location.line}</code> from acquired input <code>${escapeHtml(record.acquired_input_id)}</code>"
        ),
        "every evidence row must render its own provenance"
    );
    let trace_evidence = explanation
        .evidence_records
        .iter()
        .find(|record| record.evidence_type == "runtime-observed-target")
        .unwrap();
    assert!(html.contains(&serde_json::to_string(trace_evidence).unwrap()));
    assert!(html.contains("<dt>Call-site location</dt>"));
}

#[test]
fn unknown_or_mismatched_call_site_references_are_rejected() {
    let unknown = publish_static_and_trace(TraceAttachment::UnknownCallSiteIdentity).unwrap_err();
    assert!(
        unknown.contains("unknown contributor call-site identity 'dispatch#9'"),
        "unexpected error: {unknown}"
    );

    let unknown_input =
        publish_static_and_trace(TraceAttachment::UnknownAcquiredInput).unwrap_err();
    assert!(
        unknown_input.contains("unknown contributor call-site identity 'dispatch#0'")
            && unknown_input.contains("fixture:never-acquired"),
        "unexpected error: {unknown_input}"
    );

    let mismatched = publish_static_and_trace(TraceAttachment::MismatchedCaller).unwrap_err();
    assert!(
        mismatched.contains("names caller identity 'first_target'")
            && mismatched.contains("was contributed by caller 'dispatch'"),
        "unexpected error: {mismatched}"
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
    let acquired_input_id = |path: &str| {
        serde_json::json!(
            snapshot
                .acquired_inputs()
                .iter()
                .find(|input| input.path == path)
                .unwrap()
                .id
        )
    };
    let static_input_id = acquired_input_id(STATIC_INPUT);
    let trace_input_id = acquired_input_id(TRACE_INPUT);
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

    // (a) Re-attributing a trace's observation to the static artifact — the
    // exact failure mode this issue is about — is rejected even when input id,
    // artifact, and line are rewritten consistently, because core-generated
    // identities record the acquired input they were published from.
    let mut relocated_runtime_evidence = exported.clone();
    relocated_runtime_evidence["evidence_records"][runtime_evidence_index]["acquired_input_id"] =
        serde_json::json!(static_input_id);
    relocated_runtime_evidence["evidence_records"][runtime_evidence_index]["source_location"] = serde_json::json!({
        "input_id": static_input_id,
        "artifact": STATIC_INPUT,
        "line": CALL_SITE_LINE,
    });
    let error = load(&relocated_runtime_evidence);
    assert!(
        error.contains("was not published from acquired input"),
        "unexpected error: {error}"
    );

    // (a, continued) Rewriting the evidence identity as well does not help: an
    // evidence record and the manifestations it relates are read from the same
    // acquired input, and the traced manifestation stays in the trace input.
    let mut renamed_runtime_evidence = relocated_runtime_evidence;
    let runtime_evidence_id = exported["evidence_records"][runtime_evidence_index]["id"].clone();
    let relabelled = serde_json::json!("evidence:snapshot:evidence-provenance:input:0:relabelled");
    renamed_runtime_evidence["evidence_records"][runtime_evidence_index]["id"] = relabelled.clone();
    rename_evidence_reference(
        &mut renamed_runtime_evidence,
        &runtime_evidence_id,
        &relabelled,
    );
    let error = load(&renamed_runtime_evidence);
    assert!(
        error.contains("were read from different acquired inputs"),
        "unexpected error: {error}"
    );

    // (b) Reassigning the call site's own resolution evidence to the trace
    // input does not escape provenance checking either: the caller
    // manifestation the record relates stays in the input that published the
    // call site, and evidence never spans acquired inputs.
    let resolution_evidence_index = exported["evidence_records"]
        .as_array()
        .unwrap()
        .iter()
        .position(|record| record["evidence_type"] == serde_json::json!("static-indirect-call"))
        .unwrap();
    let mut reassigned_resolution_evidence = exported.clone();
    let resolution_evidence_id =
        exported["evidence_records"][resolution_evidence_index]["id"].clone();
    let relabelled = serde_json::json!("evidence:snapshot:evidence-provenance:input:1:relabelled");
    reassigned_resolution_evidence["evidence_records"][resolution_evidence_index]["id"] =
        relabelled.clone();
    reassigned_resolution_evidence["evidence_records"][resolution_evidence_index]["acquired_input_id"] =
        serde_json::json!(trace_input_id);
    reassigned_resolution_evidence["evidence_records"][resolution_evidence_index]["source_location"] = serde_json::json!({
        "input_id": trace_input_id,
        "artifact": TRACE_INPUT,
        "line": TRACE_LINE,
    });
    rename_evidence_reference(
        &mut reassigned_resolution_evidence,
        &resolution_evidence_id,
        &relabelled,
    );
    let error = load(&reassigned_resolution_evidence);
    assert!(
        error.contains("were read from different acquired inputs"),
        "unexpected error: {error}"
    );

    // (c) Emptying an acquired input's evidence artifact, and the artifacts of
    // the evidence read from it, no longer loads.
    let mut empty_artifact = exported.clone();
    for input in empty_artifact["acquired_inputs"].as_array_mut().unwrap() {
        if input["path"] == serde_json::json!(TRACE_INPUT) {
            input["evidence_artifact"] = serde_json::json!("");
        }
    }
    for record in empty_artifact["evidence_records"].as_array_mut().unwrap() {
        if record["acquired_input_id"] == serde_json::json!(trace_input_id) {
            record["source_location"]["artifact"] = serde_json::json!("");
        }
    }
    let error = load(&empty_artifact);
    assert!(
        error.contains("acquired input evidence artifact cannot be empty"),
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

#[test]
fn call_site_references_select_one_of_two_llvm_inputs_sharing_a_call_site_identity() {
    let application = Application;
    let snapshot = application
        .publish_snapshot(
            &[
                PathBuf::from(FIRST_LLVM_INPUT),
                PathBuf::from(SECOND_LLVM_INPUT),
                PathBuf::from(TRACE_INPUT),
            ],
            publication_context(LlvmAndTraceFixture::NAME),
            &LlvmAndTraceFixture::new(SECOND_LLVM_INPUT, "second_caller"),
        )
        .unwrap();

    // Both `.ll` inputs published the contributor call-site identity
    // `llvm-call:0`; the acquired-input fingerprint in the reference selects
    // the second one, and no third call-site entity appears.
    let call_sites = snapshot
        .program_entities()
        .iter()
        .filter(|entity| entity.kind == ProgramEntityKind::CallSite)
        .collect::<Vec<_>>();
    assert_eq!(call_sites.len(), 2);
    assert_eq!(
        call_sites
            .iter()
            .map(|entity| entity.source_location.as_ref().unwrap().artifact.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([FIRST_LLVM_INPUT, SECOND_LLVM_INPUT])
    );

    let callees = |caller: &str| {
        application
            .query_snapshot(
                &snapshot,
                NamedQuery::Callees {
                    caller_name: caller.into(),
                    caller_entity_id: None,
                },
            )
            .unwrap()
    };
    let second = callees("second_caller");
    assert_eq!(second.call_sites.len(), 1);
    assert_eq!(
        second
            .call_sites
            .iter()
            .flat_map(|site| &site.targets)
            .map(|target| target.callee_display_name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["second_callee", "traced_callee"])
    );
    let first = callees("first_caller");
    assert_eq!(first.call_sites.len(), 1);
    assert_eq!(
        first
            .call_sites
            .iter()
            .flat_map(|site| &site.targets)
            .map(|target| target.callee_display_name.as_str())
            .collect::<Vec<_>>(),
        ["first_callee"]
    );

    assert_eq!(
        located_evidence(&snapshot, "runtime-observed-target"),
        [(TRACE_INPUT.to_string(), TRACE_LINE)]
    );
    application
        .load_snapshot_json(&application.export_snapshot_json(&snapshot).unwrap())
        .unwrap();
}

#[test]
fn call_site_references_to_unknown_or_indistinguishable_acquired_inputs_are_rejected() {
    let unknown = Application
        .publish_snapshot(
            &[PathBuf::from(SECOND_LLVM_INPUT), PathBuf::from(TRACE_INPUT)],
            publication_context(LlvmAndTraceFixture::NAME),
            &LlvmAndTraceFixture::new(SECOND_LLVM_INPUT, "second_caller")
                .with_unknown_acquired_input(),
        )
        .unwrap_err();
    assert!(
        unknown.contains("unknown contributor call-site identity 'llvm-call:0'")
            && unknown.contains("fnv1a64:0000000000000000"),
        "unexpected error: {unknown}"
    );

    // The same artifact acquired twice has one content fingerprint, so the
    // qualifier cannot separate the two inputs and the reference is ambiguous
    // rather than silently resolved to whichever came first.
    let ambiguous = Application
        .publish_snapshot(
            &[
                PathBuf::from(SECOND_LLVM_INPUT),
                PathBuf::from(SECOND_LLVM_INPUT),
                PathBuf::from(TRACE_INPUT),
            ],
            publication_context(LlvmAndTraceFixture::NAME),
            &LlvmAndTraceFixture::new(SECOND_LLVM_INPUT, "second_caller"),
        )
        .unwrap_err();
    assert!(
        ambiguous.contains("ambiguous contributor call-site identity 'llvm-call:0'"),
        "unexpected error: {ambiguous}"
    );
}

/// Rewrites every reference to one acquired-input identity in an exported
/// snapshot, so a hand-edited input is internally consistent and the load has
/// to reject it on ownership grounds rather than on a dangling reference.
fn rebind_acquired_input(
    exported: &mut serde_json::Value,
    from: &serde_json::Value,
    to: &serde_json::Value,
) {
    for input in exported["acquired_inputs"].as_array_mut().unwrap() {
        if input["id"] == *from {
            input["id"] = to.clone();
        }
    }
    rebind_acquired_input_records(exported, from, to);
}

/// Moves the records read from one acquired input onto another, leaving the
/// acquired inputs themselves alone.
fn rebind_acquired_input_records(
    exported: &mut serde_json::Value,
    from: &serde_json::Value,
    to: &serde_json::Value,
) {
    for record in exported["evidence_records"].as_array_mut().unwrap() {
        if record["acquired_input_id"] == *from {
            record["acquired_input_id"] = to.clone();
        }
        if record["source_location"]["input_id"] == *from {
            record["source_location"]["input_id"] = to.clone();
        }
    }
    for manifestation in exported["manifestations"].as_array_mut().unwrap() {
        if manifestation["acquired_input_id"] == *from {
            manifestation["acquired_input_id"] = to.clone();
        }
    }
    for entity in exported["program_entities"].as_array_mut().unwrap() {
        if entity["source_location"]["input_id"] == *from {
            entity["source_location"]["input_id"] = to.clone();
        }
    }
    for claim in exported["correspondence_claims"].as_array_mut().unwrap() {
        if claim["acquired_input_id"] == *from {
            claim["acquired_input_id"] = to.clone();
        }
    }
}

#[test]
fn forged_or_misnumbered_acquired_inputs_are_rejected_on_load() {
    let application = Application;
    let snapshot = publish_static_and_trace(TraceAttachment::Attached).unwrap();
    let exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let load = |value: &serde_json::Value| {
        application
            .load_snapshot_json(&serde_json::to_string(value).unwrap())
            .unwrap_err()
    };
    let input_id = |path: &str| {
        serde_json::json!(
            snapshot
                .acquired_inputs()
                .iter()
                .find(|input| input.path == path)
                .unwrap()
                .id
        )
    };
    let trace_input_id = input_id(TRACE_INPUT);
    let static_input = exported["acquired_inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["path"] == serde_json::json!(STATIC_INPUT))
        .unwrap()
        .clone();

    // A forged acquired input carrying the static artifact, with the runtime
    // evidence and its manifestation rebound to it and their own identities
    // untouched, would report the runtime observation as static provenance.
    let mut forged = exported.clone();
    let mut forged_input = static_input.clone();
    forged_input["id"] = serde_json::json!("forged:1");
    forged["acquired_inputs"]
        .as_array_mut()
        .unwrap()
        .push(forged_input);
    rebind_acquired_input_records(&mut forged, &trace_input_id, &serde_json::json!("forged:1"));
    let error = load(&forged);
    assert!(
        error.contains("acquired input 'forged:1' is not identified as")
            && error.contains("at position 2"),
        "unexpected error: {error}"
    );

    // The same forgery with a well-formed identity does not get further: the
    // records rebound to it were published from acquired input 1, and their
    // core identities say so.
    let mut well_formed_forgery = exported.clone();
    let mut appended_input = static_input;
    let appended_id = serde_json::json!(format!(
        "input:{}:2",
        snapshot.program_snapshot().id.as_str()
    ));
    appended_input["id"] = appended_id.clone();
    well_formed_forgery["acquired_inputs"]
        .as_array_mut()
        .unwrap()
        .push(appended_input);
    rebind_acquired_input_records(&mut well_formed_forgery, &trace_input_id, &appended_id);
    let error = load(&well_formed_forgery);
    assert!(
        error.contains("was not published from acquired input"),
        "unexpected error: {error}"
    );

    // An input identity naming the wrong position, or another program
    // snapshot, is rejected even when every reference to it is consistent.
    for wrong in [
        format!("input:{}:7", snapshot.program_snapshot().id.as_str()),
        "input:snapshot:another-program:1".to_string(),
    ] {
        let mut misnumbered = exported.clone();
        rebind_acquired_input(&mut misnumbered, &trace_input_id, &serde_json::json!(wrong));
        let error = load(&misnumbered);
        assert!(
            error.contains("is not identified as") && error.contains("at position 1"),
            "unexpected error for {wrong}: {error}"
        );
    }
}
