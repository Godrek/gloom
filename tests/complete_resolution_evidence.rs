use gloom::app::Application;
use gloom::{
    ContributedCallKind, ContributedCallSite, ContributedCallable, ContributedEvidence,
    ContributedInput, ContributedTargetClaim, ContributorIdentity,
    EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability, EvidenceContribution,
    EvidenceContributor, EvidenceScope, EvidenceSupport, ObservationContext, PublishedSnapshot,
    Resolution,
};
use std::path::{Path, PathBuf};

const POLICY_SUBJECT: &str = "declares Complete resolution without static-scoped resolution";
const POLICY_REQUIREMENT: &str =
    "requires at least one call-site-resolution evidence record whose scope is Static";

/// One caller observed twice: a statically analysed call site whose targets are
/// closed, and a traced call site whose resolution the fixture varies so tests
/// can drive the completeness policy from the contributor seam.
struct DispatchFixture {
    traced_resolution: Resolution,
}

fn evidence(
    evidence_type: &str,
    scope: EvidenceScope,
    support: EvidenceSupport,
) -> ContributedEvidence {
    ContributedEvidence {
        evidence_type: evidence_type.into(),
        scope,
        support,
    }
}

fn callable(contributor_callable_id: &str, context: &ObservationContext) -> ContributedCallable {
    ContributedCallable {
        contributor_callable_id: contributor_callable_id.into(),
        display_name: contributor_callable_id.into(),
        defined: true,
        representation: "fixture-callable".into(),
        observation_context_id: context.id.clone(),
    }
}

impl EvidenceContributor for DispatchFixture {
    fn identity(&self) -> ContributorIdentity {
        ContributorIdentity {
            name: "fixture.completeness".into(),
            version: "1".into(),
            contract_version: EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION.into(),
            capabilities: vec![
                EvidenceCapability::CallableManifestations,
                EvidenceCapability::DirectCallEvidence,
                EvidenceCapability::IndirectCallEvidence,
            ],
        }
    }

    fn contribute(
        &self,
        input: &Path,
        context: &ObservationContext,
    ) -> Result<EvidenceContribution, String> {
        let traced_context = traced_context(context);
        Ok(EvidenceContribution {
            input: ContributedInput {
                path: input.display().to_string(),
                evidence_artifact: input.display().to_string(),
                media_type: "application/x-gloom-fixture".into(),
                acquisition_method: "semantic-fixture".into(),
                content_fingerprint: "fixture:completeness".into(),
            },
            observation_contexts: vec![context.clone(), traced_context.clone()],
            callables: vec![
                callable("dispatch", context),
                callable("static_target", context),
                callable("dispatch", &traced_context),
                callable("traced_target", &traced_context),
            ],
            call_sites: vec![
                ContributedCallSite {
                    kind: ContributedCallKind::Direct,
                    caller_callable_id: "dispatch".into(),
                    line: 1,
                    observation_context_id: context.id.clone(),
                    resolution: Resolution::Complete,
                    evidence: evidence(
                        "static-call-site",
                        EvidenceScope::Static,
                        EvidenceSupport::CallSiteResolution,
                    ),
                    target_claims: vec![ContributedTargetClaim {
                        target_callable_id: "static_target".into(),
                        callee_display_name: "static_target".into(),
                        target_representation: "fixture-callable".into(),
                        observation_context_id: context.id.clone(),
                        evidence: vec![evidence(
                            "static-direct-call",
                            EvidenceScope::Static,
                            EvidenceSupport::TargetClaim,
                        )],
                    }],
                },
                ContributedCallSite {
                    kind: ContributedCallKind::Indirect,
                    caller_callable_id: "dispatch".into(),
                    line: 2,
                    observation_context_id: traced_context.id.clone(),
                    resolution: self.traced_resolution,
                    evidence: evidence(
                        "runtime-indirect-call",
                        EvidenceScope::Runtime,
                        EvidenceSupport::CallSiteResolution,
                    ),
                    target_claims: vec![ContributedTargetClaim {
                        target_callable_id: "traced_target".into(),
                        callee_display_name: "traced_target".into(),
                        target_representation: "fixture-callable".into(),
                        observation_context_id: traced_context.id.clone(),
                        evidence: vec![evidence(
                            "runtime-observed-target",
                            EvidenceScope::Runtime,
                            EvidenceSupport::TargetClaim,
                        )],
                    }],
                },
            ],
        })
    }
}

fn fixture_context() -> ObservationContext {
    ObservationContext::static_analysis(
        "snapshot:completeness-fixture",
        "completeness-fixture",
        "debug fixture",
        "semantic fixture",
        "fixture.completeness",
        "1",
        "static analysis",
    )
}

fn traced_context(context: &ObservationContext) -> ObservationContext {
    ObservationContext::runtime_analysis(
        context.program_snapshot_id.as_str(),
        context.build_target.clone(),
        context.build_configuration.clone(),
        context.toolchain.clone(),
        context.extraction_method.clone(),
        context.extraction_version.clone(),
        "runtime target tracing",
        "completeness fixture workload",
    )
}

fn publish(traced_resolution: Resolution) -> Result<PublishedSnapshot, String> {
    Application.publish_snapshot(
        &[PathBuf::from("completeness.fixture")],
        fixture_context(),
        &DispatchFixture { traced_resolution },
    )
}

#[test]
fn runtime_scoped_resolution_evidence_cannot_certify_complete_resolution() {
    let error = publish(Resolution::Complete).unwrap_err();

    assert!(error.contains(POLICY_SUBJECT), "unexpected error: {error}");
    assert!(
        error.contains(POLICY_REQUIREMENT),
        "unexpected error: {error}"
    );
}

#[test]
fn runtime_scoped_resolution_evidence_still_supports_partial_resolution() {
    let snapshot = publish(Resolution::Partial).unwrap();
    let traced_context_id = traced_context(&fixture_context()).id;
    let traced = snapshot
        .call_site_resolutions()
        .iter()
        .find(|resolution| resolution.observation_context_id == traced_context_id)
        .unwrap();

    assert_eq!(traced.resolution, Resolution::Partial);
    assert!(traced.evidence_ids.iter().all(|evidence_id| {
        snapshot
            .evidence_records()
            .iter()
            .any(|record| record.id == *evidence_id && record.scope == EvidenceScope::Runtime)
    }));
}

#[test]
fn static_resolution_evidence_still_supports_complete_resolution() {
    let application = Application;
    let snapshot = publish(Resolution::Partial).unwrap();
    let static_context_id = fixture_context().id;
    let complete = snapshot
        .call_site_resolutions()
        .iter()
        .find(|resolution| resolution.resolution == Resolution::Complete)
        .unwrap();

    assert_eq!(complete.observation_context_id, static_context_id);
    assert!(complete.evidence_ids.iter().any(|evidence_id| {
        snapshot
            .evidence_records()
            .iter()
            .any(|record| record.id == *evidence_id && record.scope == EvidenceScope::Static)
    }));

    let reloaded = application
        .load_snapshot_json(&application.export_snapshot_json(&snapshot).unwrap())
        .unwrap();
    assert_eq!(
        reloaded.call_site_resolutions(),
        snapshot.call_site_resolutions()
    );
}

#[test]
fn static_and_runtime_resolution_evidence_coexist_in_their_own_contexts() {
    let snapshot = publish(Resolution::Partial).unwrap();
    let static_context_id = fixture_context().id;
    let traced_context_id = traced_context(&fixture_context()).id;

    assert_eq!(snapshot.call_site_resolutions().len(), 2);
    assert_eq!(
        snapshot
            .call_site_resolutions()
            .iter()
            .map(|resolution| (
                resolution.observation_context_id.clone(),
                resolution.resolution
            ))
            .collect::<Vec<_>>(),
        [
            (static_context_id, Resolution::Complete),
            (traced_context_id, Resolution::Partial),
        ]
    );
}

#[test]
fn loaded_snapshots_reject_complete_resolution_supported_only_by_runtime_evidence() {
    let application = Application;
    let snapshot = publish(Resolution::Partial).unwrap();
    let traced_context_id = traced_context(&fixture_context()).id;
    let traced_call_site_id = snapshot
        .call_site_resolutions()
        .iter()
        .find(|resolution| resolution.observation_context_id == traced_context_id)
        .unwrap()
        .call_site_id
        .clone();

    let mut tampered: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let call_site_id = serde_json::json!(traced_call_site_id);
    let complete = serde_json::json!("complete");
    for resolution in tampered["call_site_resolutions"].as_array_mut().unwrap() {
        if resolution["call_site_id"] == call_site_id {
            resolution["resolution"] = complete.clone();
        }
    }
    for projected in tampered["call_graph_projection"]["call_sites"]
        .as_array_mut()
        .unwrap()
    {
        if projected["call_site_id"] == call_site_id {
            projected["resolution"] = complete.clone();
        }
    }
    for relationship in tampered["call_graph_projection"]["relationships"]
        .as_array_mut()
        .unwrap()
    {
        if relationship["call_site_id"] == call_site_id {
            relationship["resolution"] = complete.clone();
        }
    }

    let error = application
        .load_snapshot_json(&serde_json::to_string(&tampered).unwrap())
        .unwrap_err();

    assert!(
        error.contains(traced_call_site_id.as_str()),
        "unexpected error: {error}"
    );
    assert!(error.contains(POLICY_SUBJECT), "unexpected error: {error}");
    assert!(
        error.contains(POLICY_REQUIREMENT),
        "unexpected error: {error}"
    );
}
