use gloom::app::Application;
use gloom::{
    CompletenessBasis, ContributedCallKind, ContributedCallSite, ContributedCallable,
    ContributedEvidence, ContributedInput, ContributedTargetClaim, ContributorIdentity,
    EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability, EvidenceContribution,
    EvidenceContributor, EvidenceScope, EvidenceSupport, LlvmTextContributor, ObservationContext,
    PublishedSnapshot, Resolution,
};
use std::path::{Path, PathBuf};

const MISSING_BASIS: &str = "declares Complete resolution without a completeness basis";
const BASIS_REQUIREMENT: &str =
    "requires at least one call-site-resolution evidence record carrying a completeness basis";
const CONTRADICTORY_BASIS: &str =
    "carries a completeness basis without declaring Complete resolution";
const INCOMPLETE_BASIS: &str = "without both a boundary and a guarantee";
const MISPLACED_BASIS: &str = "on evidence that does not support a call-site resolution";

/// One call site observed statically and one observed in a traced workload, each
/// with a resolution and an optional completeness basis the tests choose, so the
/// policy can be driven from the public contributor seam in either scope.
#[derive(Clone)]
struct SiteSpec {
    resolution: Resolution,
    basis: Option<CompletenessBasis>,
    target_basis: Option<CompletenessBasis>,
}

struct DispatchFixture {
    static_site: SiteSpec,
    traced_site: SiteSpec,
}

fn site(resolution: Resolution, basis: Option<CompletenessBasis>) -> SiteSpec {
    SiteSpec {
        resolution,
        basis,
        target_basis: None,
    }
}

fn basis() -> CompletenessBasis {
    CompletenessBasis {
        boundary: "whole-program link of target completeness-fixture".into(),
        guarantee: "the linked artifact contains every callable this site can reach".into(),
    }
}

fn evidence(
    evidence_type: &str,
    scope: EvidenceScope,
    support: EvidenceSupport,
    completeness_basis: Option<CompletenessBasis>,
) -> ContributedEvidence {
    ContributedEvidence {
        evidence_type: evidence_type.into(),
        scope,
        support,
        completeness_basis,
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

fn call_site(
    spec: &SiteSpec,
    line: usize,
    target_callable_id: &str,
    evidence_type: &str,
    scope: EvidenceScope,
    context: &ObservationContext,
) -> ContributedCallSite {
    let target_claims = if spec.resolution == Resolution::Absent {
        Vec::new()
    } else {
        vec![ContributedTargetClaim {
            target_callable_id: target_callable_id.into(),
            callee_display_name: target_callable_id.into(),
            target_representation: "fixture-callable".into(),
            observation_context_id: context.id.clone(),
            evidence: vec![evidence(
                "observed-target",
                scope,
                EvidenceSupport::TargetClaim,
                spec.target_basis.clone(),
            )],
        }]
    };
    ContributedCallSite {
        kind: ContributedCallKind::Indirect,
        caller_callable_id: "dispatch".into(),
        line,
        observation_context_id: context.id.clone(),
        resolution: spec.resolution,
        evidence: evidence(
            evidence_type,
            scope,
            EvidenceSupport::CallSiteResolution,
            spec.basis.clone(),
        ),
        target_claims,
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
                call_site(
                    &self.static_site,
                    1,
                    "static_target",
                    "static-indirect-call",
                    EvidenceScope::Static,
                    context,
                ),
                call_site(
                    &self.traced_site,
                    2,
                    "traced_target",
                    "runtime-indirect-call",
                    EvidenceScope::Runtime,
                    &traced_context,
                ),
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

fn publish(static_site: SiteSpec, traced_site: SiteSpec) -> Result<PublishedSnapshot, String> {
    Application.publish_snapshot(
        &[PathBuf::from("completeness.fixture")],
        fixture_context(),
        &DispatchFixture {
            static_site,
            traced_site,
        },
    )
}

/// A snapshot whose static call site declares a completeness basis and whose
/// traced call site resolves partially without one.
fn published_fixture() -> PublishedSnapshot {
    publish(
        site(Resolution::Complete, Some(basis())),
        site(Resolution::Partial, None),
    )
    .unwrap()
}

fn export(snapshot: &PublishedSnapshot) -> serde_json::Value {
    serde_json::from_str(&Application.export_snapshot_json(snapshot).unwrap()).unwrap()
}

fn load_error(
    mut exported: serde_json::Value,
    edit: impl FnOnce(&mut serde_json::Value),
) -> String {
    edit(&mut exported);
    Application
        .load_snapshot_json(&serde_json::to_string(&exported).unwrap())
        .unwrap_err()
}

/// The identity of the call site resolved this way and of its first resolution
/// evidence record.
fn resolved_site(
    exported: &serde_json::Value,
    kind: &str,
) -> (serde_json::Value, serde_json::Value) {
    let resolution = exported["call_site_resolutions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|resolution| resolution["resolution"] == serde_json::json!(kind))
        .unwrap();
    (
        resolution["call_site_id"].clone(),
        resolution["evidence_ids"][0].clone(),
    )
}

fn complete_site(exported: &serde_json::Value) -> (serde_json::Value, serde_json::Value) {
    resolved_site(exported, "complete")
}

/// Copies one evidence record under a new identity that no call-site resolution
/// references, optionally giving the copy a completeness basis.
fn add_unreferenced_resolution_evidence(
    exported: &mut serde_json::Value,
    evidence_id: &serde_json::Value,
    new_id: &str,
    basis: Option<CompletenessBasis>,
) {
    let mut orphan = exported["evidence_records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|evidence| evidence["id"] == *evidence_id)
        .unwrap()
        .clone();
    orphan["id"] = serde_json::json!(new_id);
    match basis {
        Some(basis) => orphan["completeness_basis"] = serde_json::to_value(basis).unwrap(),
        None => {
            orphan.as_object_mut().unwrap().remove("completeness_basis");
        }
    }
    exported["evidence_records"]
        .as_array_mut()
        .unwrap()
        .push(orphan);
}

fn set_resolution(exported: &mut serde_json::Value, call_site_id: &serde_json::Value, to: &str) {
    let to = serde_json::json!(to);
    for resolution in exported["call_site_resolutions"].as_array_mut().unwrap() {
        if resolution["call_site_id"] == *call_site_id {
            resolution["resolution"] = to.clone();
        }
    }
    for projected in exported["call_graph_projection"]["call_sites"]
        .as_array_mut()
        .unwrap()
    {
        if projected["call_site_id"] == *call_site_id {
            projected["resolution"] = to.clone();
        }
    }
    for relationship in exported["call_graph_projection"]["relationships"]
        .as_array_mut()
        .unwrap()
    {
        if relationship["call_site_id"] == *call_site_id {
            relationship["resolution"] = to.clone();
        }
    }
}

fn evidence_record<'a>(
    exported: &'a mut serde_json::Value,
    evidence_id: &serde_json::Value,
) -> &'a mut serde_json::Value {
    exported["evidence_records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|evidence| evidence["id"] == *evidence_id)
        .unwrap()
}

#[test]
fn complete_resolution_without_a_basis_is_rejected_at_contribution_in_either_scope() {
    let static_error = publish(
        site(Resolution::Complete, None),
        site(Resolution::Partial, None),
    )
    .unwrap_err();
    let runtime_error = publish(
        site(Resolution::Complete, Some(basis())),
        site(Resolution::Complete, None),
    )
    .unwrap_err();

    for error in [&static_error, &runtime_error] {
        assert!(error.contains(MISSING_BASIS), "unexpected error: {error}");
        assert!(
            error.contains(BASIS_REQUIREMENT),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn complete_resolution_with_a_basis_is_accepted_in_either_scope() {
    let snapshot = publish(
        site(Resolution::Complete, Some(basis())),
        site(Resolution::Complete, Some(basis())),
    )
    .unwrap();
    let scopes = snapshot
        .call_site_resolutions()
        .iter()
        .map(|resolution| {
            assert_eq!(resolution.resolution, Resolution::Complete);
            let evidence = snapshot
                .evidence_records()
                .iter()
                .find(|record| resolution.evidence_ids.contains(&record.id))
                .unwrap();
            assert_eq!(evidence.completeness_basis.as_ref(), Some(&basis()));
            evidence.scope
        })
        .collect::<Vec<_>>();

    assert_eq!(scopes, [EvidenceScope::Static, EvidenceScope::Runtime]);
}

#[test]
fn complete_resolution_without_a_basis_is_rejected_when_loading_a_tampered_export() {
    let snapshot = published_fixture();
    let exported = export(&snapshot);
    let (call_site_id, evidence_id) = complete_site(&exported);

    let error = load_error(exported, |exported| {
        evidence_record(exported, &evidence_id)
            .as_object_mut()
            .unwrap()
            .remove("completeness_basis");
    });

    assert!(
        error.contains(call_site_id.as_str().unwrap()),
        "unexpected error: {error}"
    );
    assert!(error.contains(MISSING_BASIS), "unexpected error: {error}");
    assert!(
        error.contains(BASIS_REQUIREMENT),
        "unexpected error: {error}"
    );
}

#[test]
fn a_completeness_basis_contradicts_partial_and_absent_resolution_at_contribution() {
    for resolution in [Resolution::Partial, Resolution::Absent] {
        let error = publish(site(resolution, Some(basis())), site(resolution, None)).unwrap_err();
        assert!(
            error.contains(CONTRADICTORY_BASIS),
            "unexpected error for {resolution:?}: {error}"
        );
    }
}

#[test]
fn a_completeness_basis_contradicts_partial_resolution_when_loading_a_tampered_export() {
    let snapshot = published_fixture();
    let exported = export(&snapshot);
    let (call_site_id, _) = complete_site(&exported);

    let error = load_error(exported, |exported| {
        set_resolution(exported, &call_site_id, "partial");
    });

    assert!(
        error.contains(CONTRADICTORY_BASIS),
        "unexpected error: {error}"
    );
}

#[test]
fn a_completeness_basis_needs_both_a_boundary_and_a_guarantee() {
    for incomplete in [
        CompletenessBasis {
            boundary: "  ".into(),
            guarantee: basis().guarantee,
        },
        CompletenessBasis {
            boundary: basis().boundary,
            guarantee: String::new(),
        },
    ] {
        let error = publish(
            site(Resolution::Complete, Some(incomplete.clone())),
            site(Resolution::Partial, None),
        )
        .unwrap_err();
        assert!(
            error.contains(INCOMPLETE_BASIS),
            "unexpected error for {incomplete:?}: {error}"
        );
    }

    let snapshot = published_fixture();
    let exported = export(&snapshot);
    let (_, evidence_id) = complete_site(&exported);
    let error = load_error(exported, |exported| {
        evidence_record(exported, &evidence_id)["completeness_basis"]["boundary"] =
            serde_json::json!("");
    });
    assert!(
        error.contains(INCOMPLETE_BASIS),
        "unexpected error: {error}"
    );
}

#[test]
fn only_call_site_resolution_evidence_may_carry_a_completeness_basis() {
    let error = publish(
        SiteSpec {
            target_basis: Some(basis()),
            ..site(Resolution::Complete, Some(basis()))
        },
        site(Resolution::Partial, None),
    )
    .unwrap_err();

    assert!(error.contains(MISPLACED_BASIS), "unexpected error: {error}");
}

#[test]
fn duplicate_resolution_evidence_is_rejected_when_loading_a_tampered_export() {
    let snapshot = published_fixture();
    let exported = export(&snapshot);
    let (call_site_id, evidence_id) = complete_site(&exported);

    let error = load_error(exported, |exported| {
        for resolution in exported["call_site_resolutions"].as_array_mut().unwrap() {
            if resolution["call_site_id"] == call_site_id {
                resolution["evidence_ids"]
                    .as_array_mut()
                    .unwrap()
                    .push(evidence_id.clone());
            }
        }
    });

    assert!(
        error.contains("more than once"),
        "unexpected error: {error}"
    );
}

#[test]
fn exported_snapshots_round_trip_the_completeness_basis() {
    let application = Application;
    let snapshot = published_fixture();
    let exported = export(&snapshot);
    let (_, evidence_id) = complete_site(&exported);
    let exported_evidence = exported["evidence_records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|evidence| evidence["id"] == evidence_id)
        .unwrap();

    assert_eq!(
        exported_evidence["completeness_basis"],
        serde_json::to_value(basis()).unwrap()
    );
    assert!(
        exported["evidence_records"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|evidence| evidence["id"] != evidence_id)
            .all(|evidence| evidence.get("completeness_basis").is_none()),
        "evidence without a basis must omit the field"
    );

    let reloaded = application
        .load_snapshot_json(&application.export_snapshot_json(&snapshot).unwrap())
        .unwrap();
    assert_eq!(reloaded.evidence_records(), snapshot.evidence_records());
    assert_eq!(
        reloaded.call_site_resolutions(),
        snapshot.call_site_resolutions()
    );
}

#[test]
fn an_unreferenced_completeness_basis_cannot_ride_along_with_an_open_resolution() {
    for (kind, snapshot) in [
        (
            "absent",
            publish(
                site(Resolution::Absent, None),
                site(Resolution::Partial, None),
            )
            .unwrap(),
        ),
        (
            "partial",
            publish(
                site(Resolution::Partial, None),
                site(Resolution::Partial, None),
            )
            .unwrap(),
        ),
    ] {
        let exported = export(&snapshot);
        let (_, evidence_id) = resolved_site(&exported, kind);
        let error = load_error(exported, |exported| {
            add_unreferenced_resolution_evidence(
                exported,
                &evidence_id,
                "evidence:orphan-basis",
                Some(basis()),
            );
        });
        assert!(
            error.contains("evidence:orphan-basis")
                && error.contains("is not referenced by the resolution of call site"),
            "unexpected error for {kind}: {error}"
        );
    }
}

#[test]
fn unreferenced_call_site_resolution_evidence_is_rejected_even_without_a_basis() {
    let snapshot = published_fixture();
    let exported = export(&snapshot);
    let (_, evidence_id) = resolved_site(&exported, "partial");

    let error = load_error(exported, |exported| {
        add_unreferenced_resolution_evidence(exported, &evidence_id, "evidence:orphan", None);
    });

    assert!(
        error.contains("evidence:orphan")
            && error.contains("is not referenced by the resolution of call site"),
        "unexpected error: {error}"
    );
}

/// A call whose callee operand is a data global is not a call to a callable of
/// that name, so it must stay unresolved and must not declare completeness.
#[test]
#[ignore = "requires #22 (declared-callable resolution) on main"]
fn a_call_through_a_data_global_declares_no_completeness() {
    let application = Application;
    let context = ObservationContext::static_analysis(
        "snapshot:global-pointer-call",
        "global-pointer-call",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/global-pointer-call.ll")],
            context,
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();

    let projected = &snapshot.call_graph_projection().call_sites;
    assert_eq!(projected.len(), 1);
    assert_eq!(projected[0].resolution, Resolution::Absent);
    assert!(projected[0].targets.is_empty());
    assert!(snapshot.target_claims().is_empty());
    assert!(
        snapshot
            .evidence_records()
            .iter()
            .all(|evidence| evidence.completeness_basis.is_none())
    );
}
