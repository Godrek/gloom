use gloom::app::{Application, NamedQuery};
use gloom::{
    CONTRIBUTED_EVIDENCE_TARGET_RULE, CONTRIBUTOR_IDENTITY_CORRESPONDENCE_RULE,
    CallRelationshipsResult, CallableIdentityScope, CompletenessBasis, ContributedCallKind,
    ContributedCallSite, ContributedCallable, ContributedEvidence, ContributedEvidenceLocation,
    ContributedInput, ContributedTargetClaim, ContributorCallSiteId, ContributorCallableIdentity,
    ContributorIdentity, EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability,
    EvidenceContribution, EvidenceContributor, EvidenceScope, EvidenceSupport, LlvmTextContributor,
    ObservationContext, ProgramEntityKind, PublishedSnapshot, Resolution,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture_context() -> ObservationContext {
    ObservationContext::static_analysis(
        "snapshot:indirect-call-fixture",
        "indirect-call-fixture",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    )
}

fn callable_identity(value: &str) -> ContributorCallableIdentity {
    ContributorCallableIdentity::new(value, CallableIdentityScope::LinkageNamespace).unwrap()
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

/// Complete resolution requires an explicit completeness basis, so a fixture
/// that publishes a complete call site declares why its target set is closed.
fn completeness_basis() -> CompletenessBasis {
    CompletenessBasis {
        boundary: "the fixture call instruction".into(),
        guarantee: "the fixture enumerates every target this call site can reach".into(),
    }
}

struct PossibleTargetsFixture;

impl EvidenceContributor for PossibleTargetsFixture {
    fn identity(&self) -> ContributorIdentity {
        ContributorIdentity {
            name: "fixture.possible-targets".into(),
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
        let runtime_context = ObservationContext::runtime_analysis(
            context.program_snapshot_id.as_str(),
            context.build_target.clone(),
            context.build_configuration.clone(),
            context.toolchain.clone(),
            context.extraction_method.clone(),
            context.extraction_version.clone(),
            "runtime target tracing",
            "dispatch fixture workload",
        );
        let artifact = input.display().to_string();
        Ok(EvidenceContribution {
            input: ContributedInput {
                path: input.display().to_string(),
                evidence_artifact: artifact.clone(),
                media_type: "application/x-gloom-fixture".into(),
                acquisition_method: "semantic-fixture".into(),
                content_fingerprint: "fixture:possible-targets".into(),
            },
            observation_contexts: vec![context.clone(), runtime_context.clone()],
            callables: ["dispatch", "first_target", "second_target"]
                .into_iter()
                .enumerate()
                .map(|(index, name)| ContributedCallable {
                    callable_identity: callable_identity(name),
                    display_name: name.into(),
                    defined: true,
                    representation: "fixture-callable".into(),
                    observation_context_id: context.id.clone(),
                    line: index + 1,
                    identity_evidence: evidence(
                        "static-callable-identity",
                        EvidenceScope::Static,
                        EvidenceSupport::ContributorIdentity,
                        &artifact,
                        index + 1,
                    ),
                })
                .chain([ContributedCallable {
                    callable_identity: callable_identity("first_target"),
                    display_name: "first_target".into(),
                    defined: true,
                    representation: "runtime-fixture-callable".into(),
                    observation_context_id: runtime_context.id.clone(),
                    line: 4,
                    identity_evidence: evidence(
                        "runtime-callable-identity",
                        EvidenceScope::Runtime,
                        EvidenceSupport::ContributorIdentity,
                        &artifact,
                        4,
                    ),
                }])
                .collect(),
            call_sites: vec![ContributedCallSite {
                contributor_call_site_id: ContributorCallSiteId::new("dispatch:7").unwrap(),
                kind: ContributedCallKind::Indirect,
                caller_callable_identity: callable_identity("dispatch"),
                line: 7,
                observation_context_id: context.id.clone(),
                resolution: Resolution::Partial,
                evidence: evidence(
                    "static-indirect-call",
                    EvidenceScope::Static,
                    EvidenceSupport::CallSiteResolution,
                    &artifact,
                    7,
                ),
                target_claims: vec![
                    ContributedTargetClaim {
                        target_callable_identity: callable_identity("first_target"),
                        callee_display_name: "first_target".into(),
                        target_representation: "fixture-callable".into(),
                        observation_context_id: context.id.clone(),
                        evidence: vec![evidence(
                            "static-possible-target",
                            EvidenceScope::Static,
                            EvidenceSupport::TargetClaim,
                            &artifact,
                            7,
                        )],
                    },
                    ContributedTargetClaim {
                        target_callable_identity: callable_identity("first_target"),
                        callee_display_name: "first_target".into(),
                        target_representation: "runtime-fixture-callable".into(),
                        observation_context_id: runtime_context.id.clone(),
                        evidence: vec![evidence(
                            "runtime-observed-target",
                            EvidenceScope::Runtime,
                            EvidenceSupport::TargetClaim,
                            &artifact,
                            7,
                        )],
                    },
                    ContributedTargetClaim {
                        target_callable_identity: callable_identity("second_target"),
                        callee_display_name: "second_target".into(),
                        target_representation: "fixture-callable".into(),
                        observation_context_id: context.id.clone(),
                        evidence: vec![evidence(
                            "static-possible-target",
                            EvidenceScope::Static,
                            EvidenceSupport::TargetClaim,
                            &artifact,
                            7,
                        )],
                    },
                ],
            }],
            call_site_attachments: Vec::new(),
        })
    }
}

struct SameLabelCallersFixture;

impl EvidenceContributor for SameLabelCallersFixture {
    fn identity(&self) -> ContributorIdentity {
        ContributorIdentity {
            name: "fixture.same-label-callers".into(),
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
        let runtime_context = ObservationContext::runtime_analysis(
            context.program_snapshot_id.as_str(),
            context.build_target.clone(),
            context.build_configuration.clone(),
            context.toolchain.clone(),
            context.extraction_method.clone(),
            context.extraction_version.clone(),
            "runtime tracing",
            "same-label workload",
        );
        let artifact = input.display().to_string();
        Ok(EvidenceContribution {
            input: ContributedInput {
                path: input.display().to_string(),
                evidence_artifact: artifact.clone(),
                media_type: "application/x-gloom-fixture".into(),
                acquisition_method: "semantic-fixture".into(),
                content_fingerprint: "fixture:same-label-callers".into(),
            },
            observation_contexts: vec![context.clone(), runtime_context.clone()],
            callables: vec![
                ContributedCallable {
                    callable_identity: callable_identity("static-worker-a"),
                    display_name: "worker".into(),
                    defined: true,
                    representation: "static-worker-a".into(),
                    observation_context_id: context.id.clone(),
                    line: 1,
                    identity_evidence: evidence(
                        "static-callable-identity",
                        EvidenceScope::Static,
                        EvidenceSupport::ContributorIdentity,
                        &artifact,
                        1,
                    ),
                },
                ContributedCallable {
                    callable_identity: callable_identity("static-worker-b"),
                    display_name: "worker".into(),
                    defined: true,
                    representation: "static-worker-b".into(),
                    observation_context_id: context.id.clone(),
                    line: 2,
                    identity_evidence: evidence(
                        "static-callable-identity",
                        EvidenceScope::Static,
                        EvidenceSupport::ContributorIdentity,
                        &artifact,
                        2,
                    ),
                },
                ContributedCallable {
                    callable_identity: callable_identity("runtime-worker"),
                    display_name: "worker".into(),
                    defined: true,
                    representation: "runtime-worker".into(),
                    observation_context_id: runtime_context.id.clone(),
                    line: 3,
                    identity_evidence: evidence(
                        "runtime-callable-identity",
                        EvidenceScope::Runtime,
                        EvidenceSupport::ContributorIdentity,
                        &artifact,
                        3,
                    ),
                },
            ],
            call_sites: vec![
                ContributedCallSite {
                    contributor_call_site_id: ContributorCallSiteId::new("static-worker-a:1")
                        .unwrap(),
                    kind: ContributedCallKind::Indirect,
                    caller_callable_identity: callable_identity("static-worker-a"),
                    line: 1,
                    observation_context_id: context.id.clone(),
                    resolution: Resolution::Absent,
                    evidence: evidence(
                        "static-indirect-call",
                        EvidenceScope::Static,
                        EvidenceSupport::CallSiteResolution,
                        &artifact,
                        1,
                    ),
                    target_claims: Vec::new(),
                },
                ContributedCallSite {
                    contributor_call_site_id: ContributorCallSiteId::new("static-worker-b:2")
                        .unwrap(),
                    kind: ContributedCallKind::Indirect,
                    caller_callable_identity: callable_identity("static-worker-b"),
                    line: 2,
                    observation_context_id: context.id.clone(),
                    resolution: Resolution::Absent,
                    evidence: evidence(
                        "static-indirect-call",
                        EvidenceScope::Static,
                        EvidenceSupport::CallSiteResolution,
                        &artifact,
                        2,
                    ),
                    target_claims: Vec::new(),
                },
                ContributedCallSite {
                    contributor_call_site_id: ContributorCallSiteId::new("runtime-worker:3")
                        .unwrap(),
                    kind: ContributedCallKind::Indirect,
                    caller_callable_identity: callable_identity("runtime-worker"),
                    line: 3,
                    observation_context_id: runtime_context.id.clone(),
                    resolution: Resolution::Absent,
                    evidence: evidence(
                        "runtime-indirect-call",
                        EvidenceScope::Runtime,
                        EvidenceSupport::CallSiteResolution,
                        &artifact,
                        3,
                    ),
                    target_claims: Vec::new(),
                },
            ],
            call_site_attachments: Vec::new(),
        })
    }
}

struct WorkloadUnqualifiedRuntimeEvidenceFixture;

impl EvidenceContributor for WorkloadUnqualifiedRuntimeEvidenceFixture {
    fn identity(&self) -> ContributorIdentity {
        ContributorIdentity {
            name: "fixture.invalid-runtime-scope".into(),
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
        let artifact = input.display().to_string();
        Ok(EvidenceContribution {
            input: ContributedInput {
                path: input.display().to_string(),
                evidence_artifact: artifact.clone(),
                media_type: "application/x-gloom-fixture".into(),
                acquisition_method: "semantic-fixture".into(),
                content_fingerprint: "fixture:invalid-runtime-scope".into(),
            },
            observation_contexts: vec![context.clone()],
            callables: ["caller", "runtime_target"]
                .into_iter()
                .enumerate()
                .map(|(index, name)| ContributedCallable {
                    callable_identity: callable_identity(name),
                    display_name: name.into(),
                    defined: true,
                    representation: "fixture-callable".into(),
                    observation_context_id: context.id.clone(),
                    line: index + 1,
                    identity_evidence: evidence(
                        "static-callable-identity",
                        EvidenceScope::Static,
                        EvidenceSupport::ContributorIdentity,
                        &artifact,
                        index + 1,
                    ),
                })
                .collect(),
            call_sites: vec![ContributedCallSite {
                contributor_call_site_id: ContributorCallSiteId::new("caller:1").unwrap(),
                kind: ContributedCallKind::Indirect,
                caller_callable_identity: callable_identity("caller"),
                line: 1,
                observation_context_id: context.id.clone(),
                resolution: Resolution::Complete,
                evidence: ContributedEvidence {
                    completeness_basis: Some(completeness_basis()),
                    ..evidence(
                        "static-indirect-call",
                        EvidenceScope::Static,
                        EvidenceSupport::CallSiteResolution,
                        &artifact,
                        1,
                    )
                },
                target_claims: vec![ContributedTargetClaim {
                    target_callable_identity: callable_identity("runtime_target"),
                    callee_display_name: "runtime_target".into(),
                    target_representation: "fixture-callable".into(),
                    observation_context_id: context.id.clone(),
                    evidence: vec![evidence(
                        "runtime-observed-target",
                        EvidenceScope::Runtime,
                        EvidenceSupport::TargetClaim,
                        &artifact,
                        1,
                    )],
                }],
            }],
            call_site_attachments: Vec::new(),
        })
    }
}

#[test]
fn runtime_evidence_requires_a_workload_qualified_context() {
    let context = ObservationContext::static_analysis(
        "snapshot:invalid-runtime-scope",
        "invalid-runtime-scope",
        "debug fixture",
        "semantic fixture",
        "fixture.invalid-runtime-scope",
        "1",
        "static analysis",
    );
    let error = Application
        .publish_snapshot(
            &[PathBuf::from("invalid-runtime-scope.fixture")],
            context,
            &WorkloadUnqualifiedRuntimeEvidenceFixture,
        )
        .unwrap_err();

    assert!(error.contains("Runtime evidence"));
    assert!(error.contains("incompatible with observation context"));
}

#[test]
fn callable_identity_and_named_queries_do_not_blend_same_label_callers() {
    let application = Application;
    let context = ObservationContext::static_analysis(
        "snapshot:same-label-callers",
        "same-label-callers",
        "debug fixture",
        "semantic fixture",
        "fixture.same-label-callers",
        "1",
        "static analysis",
    );
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("same-label-callers.fixture")],
            context,
            &SameLabelCallersFixture,
        )
        .unwrap();
    let workers = snapshot
        .program_entities()
        .iter()
        .filter(|entity| {
            entity.kind == ProgramEntityKind::Callable && entity.display_name == "worker"
        })
        .collect::<Vec<_>>();
    assert_eq!(workers.len(), 3);
    assert_eq!(
        workers
            .iter()
            .map(|worker| {
                snapshot
                    .manifestations()
                    .iter()
                    .find(|manifestation| manifestation.entity_id == worker.id)
                    .unwrap()
                    .representation
                    .as_str()
            })
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["runtime-worker", "static-worker-a", "static-worker-b"])
    );

    let ambiguous = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "worker".into(),
                caller_entity_id: None,
            },
        )
        .unwrap_err();
    assert!(ambiguous.contains("is ambiguous"));

    let selected = workers[0];
    let selected_manifestation = snapshot
        .manifestations()
        .iter()
        .find(|manifestation| manifestation.entity_id == selected.id)
        .unwrap();
    let selected_result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "worker".into(),
                caller_entity_id: Some(selected.id.clone()),
            },
        )
        .unwrap();
    let selected_result = selected_result.call_relationships().unwrap();
    assert_eq!(selected_result.selected_callable_entity_id, selected.id);
    assert_eq!(
        selected_result.selected_callable_observation_context_id,
        selected_manifestation.observation_context_id
    );
    assert_eq!(selected_result.call_sites.len(), 1);
    assert_eq!(selected_result.call_sites[0].caller_entity_id, selected.id);

    let static_worker = snapshot
        .manifestations()
        .iter()
        .find(|manifestation| manifestation.representation == "static-worker-a")
        .unwrap();
    let runtime_worker = snapshot
        .manifestations()
        .iter()
        .find(|manifestation| manifestation.representation == "runtime-worker")
        .unwrap();
    let static_site = snapshot
        .call_graph_projection()
        .call_sites
        .iter()
        .find(|site| site.caller_entity_id == static_worker.entity_id)
        .unwrap();
    let mut cross_context_caller = serde_json::to_value(&snapshot).unwrap();
    cross_context_caller["program_entities"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entity| entity["id"] == serde_json::json!(static_site.call_site_id))
        .unwrap()["caller_entity_id"] = serde_json::json!(runtime_worker.entity_id);
    cross_context_caller["call_graph_projection"]["call_sites"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|site| site["call_site_id"] == serde_json::json!(static_site.call_site_id))
        .unwrap()["caller_entity_id"] = serde_json::json!(runtime_worker.entity_id);
    let error = application
        .load_snapshot_json(&serde_json::to_string(&cross_context_caller).unwrap())
        .unwrap_err();
    assert!(error.contains("has no caller manifestation in observation context"));

    let static_worker_b = snapshot
        .manifestations()
        .iter()
        .find(|manifestation| manifestation.representation == "static-worker-b")
        .unwrap();
    let mut same_context_collapse = serde_json::to_value(&snapshot).unwrap();
    same_context_collapse["manifestations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|manifestation| manifestation["id"] == serde_json::json!(static_worker_b.id))
        .unwrap()["entity_id"] = serde_json::json!(static_worker.entity_id);
    same_context_collapse["program_entities"]
        .as_array_mut()
        .unwrap()
        .retain(|entity| entity["id"] != serde_json::json!(static_worker_b.entity_id));
    let error = application
        .load_snapshot_json(&serde_json::to_string(&same_context_collapse).unwrap())
        .unwrap_err();
    assert!(
        error.contains("merges distinct contributor callable identities"),
        "unexpected error: {error}"
    );

    let path = std::env::temp_dir().join(format!(
        "gloom-same-label-callers-{}.json",
        std::process::id()
    ));
    fs::write(&path, application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let ambiguous_cli = Command::new(env!("CARGO_BIN_EXE_gloom"))
        .arg("query-snapshot")
        .arg(&path)
        .args(["--callees", "worker"])
        .output()
        .unwrap();
    let selected_cli = Command::new(env!("CARGO_BIN_EXE_gloom"))
        .arg("query-snapshot")
        .arg(&path)
        .args(["--callees", "worker", "--caller-entity-id"])
        .arg(selected.id.as_str())
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();

    assert_eq!(ambiguous_cli.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&ambiguous_cli.stderr).contains("is ambiguous"));
    assert!(selected_cli.status.success());
    let selected_cli_result: serde_json::Value =
        serde_json::from_slice(&selected_cli.stdout).unwrap();
    assert_eq!(
        selected_cli_result["selected_callable_entity_id"],
        serde_json::json!(selected.id)
    );
    assert_eq!(
        selected_cli_result["selected_callable_observation_context_id"],
        serde_json::json!(selected_manifestation.observation_context_id)
    );
    assert_eq!(
        selected_cli_result["call_sites"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn partial_resolution_keeps_multiple_targets_and_independent_evidence_types() {
    let application = Application;
    let context = ObservationContext::static_analysis(
        "snapshot:possible-targets-fixture",
        "possible-targets-fixture",
        "debug fixture",
        "semantic fixture",
        "fixture.possible-targets",
        "1",
        "target analysis",
    );
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("possible-targets.fixture")],
            context,
            &PossibleTargetsFixture,
        )
        .unwrap();

    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "dispatch".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let result = result.call_relationships().unwrap();
    let call_site = &result.call_sites[0];
    assert_eq!(call_site.resolution, Resolution::Partial);
    assert_eq!(
        call_site
            .targets
            .iter()
            .map(|target| target.callee_display_name.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["first_target", "second_target"])
    );

    let first_targets = call_site
        .targets
        .iter()
        .filter(|target| target.callee_display_name == "first_target")
        .collect::<Vec<_>>();
    assert_eq!(first_targets.len(), 2);
    let first_claims = first_targets
        .iter()
        .map(|target| {
            snapshot
                .target_claims()
                .iter()
                .find(|claim| claim.id == target.target_claim_id)
                .unwrap()
        })
        .collect::<Vec<_>>();
    let claim_for_evidence_type = |evidence_type: &str| {
        first_claims.iter().copied().find(|claim| {
            claim.evidence_ids.iter().any(|id| {
                snapshot
                    .evidence_records()
                    .iter()
                    .any(|record| record.id == *id && record.evidence_type == evidence_type)
            })
        })
    };
    let static_claim = claim_for_evidence_type("static-possible-target").unwrap();
    let runtime_claim = claim_for_evidence_type("runtime-observed-target").unwrap();
    let static_context = snapshot
        .observation_contexts()
        .iter()
        .find(|context| context.id == static_claim.observation_context_id)
        .unwrap();
    let runtime_context = snapshot
        .observation_contexts()
        .iter()
        .find(|context| context.id == runtime_claim.observation_context_id)
        .unwrap();
    assert_eq!(static_context.runtime_workload, None);
    assert_eq!(
        runtime_context.runtime_workload.as_deref(),
        Some("dispatch fixture workload")
    );
    assert_ne!(static_context.id, runtime_context.id);
    assert_eq!(
        call_site.resolution_observation_context_id,
        static_context.id
    );
    assert!(first_claims.iter().all(|claim| {
        claim.evidence_ids.iter().all(|id| {
            snapshot
                .evidence_records()
                .iter()
                .find(|record| record.id == *id)
                .unwrap()
                .observation_context_id
                == claim.observation_context_id
        })
    }));
    let static_manifestation = snapshot
        .manifestations()
        .iter()
        .find(|manifestation| manifestation.id == static_claim.target_manifestation_id)
        .unwrap();
    let runtime_manifestation = snapshot
        .manifestations()
        .iter()
        .find(|manifestation| manifestation.id == runtime_claim.target_manifestation_id)
        .unwrap();
    assert_eq!(
        runtime_manifestation.observation_context_id,
        runtime_context.id
    );
    assert_eq!(
        static_manifestation.observation_context_id,
        static_context.id
    );
    assert_eq!(static_manifestation.representation, "fixture-callable");
    assert_eq!(
        static_manifestation.contributor_callable_identity.as_str(),
        "first_target"
    );
    assert_eq!(
        runtime_manifestation.contributor_callable_identity.as_str(),
        "first_target"
    );
    assert_eq!(
        runtime_manifestation.representation,
        "runtime-fixture-callable"
    );
    assert_ne!(
        static_manifestation.entity_id,
        runtime_manifestation.entity_id
    );
    let static_target = first_targets
        .iter()
        .copied()
        .find(|target| target.target_claim_id == static_claim.id)
        .unwrap();
    let runtime_target = first_targets
        .iter()
        .copied()
        .find(|target| target.target_claim_id == runtime_claim.id)
        .unwrap();
    assert_eq!(
        static_target.target_observation_context_id,
        static_context.id
    );
    assert_eq!(
        runtime_target.target_observation_context_id,
        runtime_context.id
    );
    assert_ne!(
        static_target.callee_entity_id,
        runtime_target.callee_entity_id
    );
    let correspondence = snapshot.correspondence_claims().first().unwrap();
    assert_eq!(snapshot.correspondence_claims().len(), 1);
    assert_eq!(
        correspondence.rule,
        CONTRIBUTOR_IDENTITY_CORRESPONDENCE_RULE
    );
    assert_eq!(
        correspondence.contributor_callable_identity.as_str(),
        "first_target"
    );
    assert_eq!(
        correspondence
            .manifestation_ids
            .iter()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([&static_manifestation.id, &runtime_manifestation.id])
    );
    let correspondence_evidence = correspondence
        .evidence_ids
        .iter()
        .map(|id| {
            snapshot
                .evidence_records()
                .iter()
                .find(|record| record.id == *id)
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert!(
        correspondence_evidence
            .iter()
            .all(|record| record.support == EvidenceSupport::ContributorIdentity),
        "correspondence must cite only contributor-identity evidence"
    );
    assert_eq!(
        correspondence_evidence
            .iter()
            .flat_map(|record| &record.related_manifestation_ids)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([&static_manifestation.id, &runtime_manifestation.id])
    );
    let cited_evidence_ids = correspondence.evidence_ids.iter().collect::<BTreeSet<_>>();
    assert!(
        static_claim
            .evidence_ids
            .iter()
            .chain(&runtime_claim.evidence_ids)
            .chain(
                snapshot
                    .call_site_resolutions()
                    .iter()
                    .flat_map(|resolution| &resolution.evidence_ids)
            )
            .all(|id| !cited_evidence_ids.contains(id)),
        "correspondence must cite neither target nor resolution evidence"
    );
    assert_eq!(
        static_target.correspondence_claim_ids,
        std::slice::from_ref(&correspondence.id)
    );
    assert_eq!(
        runtime_target.correspondence_claim_ids,
        std::slice::from_ref(&correspondence.id)
    );
    assert_eq!(
        result.correspondence_claims,
        std::slice::from_ref(correspondence)
    );
    let static_relationship = result
        .relationships
        .iter()
        .find(|relationship| relationship.target_claim_id == static_claim.id)
        .unwrap();
    let runtime_relationship = result
        .relationships
        .iter()
        .find(|relationship| relationship.target_claim_id == runtime_claim.id)
        .unwrap();
    assert_eq!(
        static_relationship.target_observation_context_id,
        static_context.id
    );
    assert_eq!(
        runtime_relationship.target_observation_context_id,
        runtime_context.id
    );
    assert_eq!(
        static_relationship.resolution_observation_context_id,
        static_context.id
    );
    assert_eq!(
        runtime_relationship.resolution_observation_context_id,
        static_context.id
    );
    assert_eq!(runtime_relationship.resolution, Resolution::Partial);
    assert_eq!(
        runtime_relationship.correspondence_claim_ids,
        std::slice::from_ref(&correspondence.id)
    );
    assert!(
        serde_json::to_value(static_claim)
            .unwrap()
            .get("resolution")
            .is_none(),
        "target evidence must not carry target-set resolution"
    );

    let exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let exported_runtime_relationship = exported["call_graph_projection"]["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .find(|relationship| relationship["target_claim_id"] == serde_json::json!(runtime_claim.id))
        .unwrap();
    assert_eq!(
        exported_runtime_relationship["target_observation_context_id"],
        serde_json::json!(runtime_context.id)
    );
    assert_eq!(
        exported_runtime_relationship["resolution_observation_context_id"],
        serde_json::json!(static_context.id)
    );
    assert_eq!(
        exported["correspondence_claims"][0],
        serde_json::to_value(correspondence).unwrap()
    );

    let explanation = application
        .explain_snapshot(&snapshot, &call_site.explanation_handle)
        .unwrap();
    assert_eq!(
        explanation.correspondence_claims,
        std::slice::from_ref(correspondence)
    );

    let mut mismatched_projection_export = exported.clone();
    let runtime_relationship_export =
        mismatched_projection_export["call_graph_projection"]["relationships"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|relationship| {
                relationship["target_claim_id"] == serde_json::json!(runtime_claim.id)
            })
            .unwrap();
    runtime_relationship_export["resolution_observation_context_id"] =
        serde_json::json!(runtime_context.id);
    let error = application
        .load_snapshot_json(&serde_json::to_string(&mismatched_projection_export).unwrap())
        .unwrap_err();
    assert!(error.contains("does not match its target claim"));

    let mut collapsed_identity_export = exported.clone();
    let runtime_manifestation_export = collapsed_identity_export["manifestations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|manifestation| manifestation["id"] == serde_json::json!(runtime_manifestation.id))
        .unwrap();
    runtime_manifestation_export["entity_id"] = serde_json::json!(static_manifestation.entity_id);
    let error = application
        .load_snapshot_json(&serde_json::to_string(&collapsed_identity_export).unwrap())
        .unwrap_err();
    assert!(error.contains("merged across observation contexts"));

    let mut incomplete_export = exported.clone();
    incomplete_export["call_graph_projection"]["call_sites"][0]["targets"]
        .as_array_mut()
        .unwrap()
        .pop();
    let error = application
        .load_snapshot_json(&serde_json::to_string(&incomplete_export).unwrap())
        .unwrap_err();
    assert!(error.contains("does not preserve every target claim"));

    let html = application.render_snapshot_viewer(&snapshot).unwrap();
    assert!(html.contains("\"target_set_incomplete\":true"));
    assert!(html.contains("Target contexts"));
    assert!(html.contains("Target correspondence"));
    assert!(html.contains(correspondence.id.as_str()));
    assert!(html.contains(CONTRIBUTOR_IDENTITY_CORRESPONDENCE_RULE));
    assert!(html.contains(correspondence.contributor_callable_identity.as_str()));
    assert!(html.contains("Resolution context"));
    assert!(html.contains(runtime_context.id.as_str()));

    let mut missing_relationship_export = exported.clone();
    missing_relationship_export["call_graph_projection"]["relationships"]
        .as_array_mut()
        .unwrap()
        .pop();
    let error = application
        .load_snapshot_json(&serde_json::to_string(&missing_relationship_export).unwrap())
        .unwrap_err();
    assert!(error.contains("exactly one relationship for every target claim"));

    let mut unknown_rule_export = exported.clone();
    unknown_rule_export["correspondence_claims"][0]["rule"] =
        serde_json::json!("same-display-name");
    let error = application
        .load_snapshot_json(&serde_json::to_string(&unknown_rule_export).unwrap())
        .unwrap_err();
    assert!(
        error.contains("unknown derivation rule"),
        "unexpected error: {error}"
    );

    let mut unsupported_correspondence_export = exported.clone();
    unsupported_correspondence_export["correspondence_claims"][0]["evidence_ids"]
        .as_array_mut()
        .unwrap()
        .pop();
    let error = application
        .load_snapshot_json(&serde_json::to_string(&unsupported_correspondence_export).unwrap())
        .unwrap_err();
    assert!(error.contains("lacks evidence for every manifestation"));

    let mut duplicate_relationship_export = exported;
    let duplicate =
        duplicate_relationship_export["call_graph_projection"]["relationships"][0].clone();
    duplicate_relationship_export["call_graph_projection"]["relationships"]
        .as_array_mut()
        .unwrap()
        .push(duplicate);
    let error = application
        .load_snapshot_json(&serde_json::to_string(&duplicate_relationship_export).unwrap())
        .unwrap_err();
    assert!(error.contains("exactly one relationship for every target claim"));
}

#[test]
fn named_queries_and_exported_projections_preserve_unresolved_call_sites() {
    let application = Application;
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/indirect-calls.ll")],
            fixture_context(),
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();

    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "dispatch".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let result = result.call_relationships().unwrap();

    assert_eq!(result.call_sites.len(), 2);
    assert!(result.call_sites.iter().all(|site| {
        site.resolution == Resolution::Absent
            && site.targets.is_empty()
            && !site.explanation_handle.as_str().is_empty()
    }));
    assert_ne!(
        result.call_sites[0].explanation_handle,
        result.call_sites[1].explanation_handle
    );
    let callback_result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "run_callback".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let callback_result = callback_result.call_relationships().unwrap();
    assert_eq!(callback_result.call_sites.len(), 2);
    assert_eq!(
        callback_result
            .call_sites
            .iter()
            .map(|site| {
                snapshot
                    .program_entities()
                    .iter()
                    .find(|entity| entity.id == site.call_site_id)
                    .unwrap()
                    .source_location
                    .as_ref()
                    .unwrap()
                    .line
            })
            .collect::<Vec<_>>(),
        [10, 11]
    );
    assert!(
        callback_result
            .call_sites
            .iter()
            .all(|site| site.resolution == Resolution::Absent && site.targets.is_empty())
    );

    let exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    assert_eq!(
        exported["call_graph_projection"]["call_sites"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn tokenized_llvm_calls_ignore_comments_newlines_and_label_placement() {
    let application = Application;
    let context = ObservationContext::static_analysis(
        "snapshot:tokenized-call-fixture",
        "tokenized-call-fixture",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from(
                "tests/fixtures/tokenized-call-instructions.ll",
            )],
            context,
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "tokenized_calls".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let result = result.call_relationships().unwrap();

    assert_eq!(result.call_sites.len(), 3);
    assert_eq!(result.relationships.len(), 1);
    assert_eq!(result.relationships[0].callee_display_name, "callee");
    assert_eq!(
        result
            .call_sites
            .iter()
            .map(|site| site.resolution)
            .collect::<Vec<_>>(),
        [Resolution::Complete, Resolution::Absent, Resolution::Absent]
    );
    assert_eq!(
        result
            .call_sites
            .iter()
            .map(|site| {
                snapshot
                    .program_entities()
                    .iter()
                    .find(|entity| entity.id == site.call_site_id)
                    .unwrap()
                    .source_location
                    .as_ref()
                    .unwrap()
                    .line
            })
            .collect::<Vec<_>>(),
        [4, 7, 11]
    );
    let exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    assert_eq!(
        exported["call_graph_projection"]["call_sites"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn llvm_metadata_and_aggregate_prefixes_preserve_instruction_boundaries() {
    let application = Application;
    let context = ObservationContext::static_analysis(
        "snapshot:metadata-prefix-fixture",
        "metadata-prefix-fixture",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/metadata-prefix-calls.ll")],
            context,
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();

    let metadata_only = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "metadata_only".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let metadata_only = metadata_only.call_relationships().unwrap();
    assert!(metadata_only.call_sites.is_empty());

    let aggregate_prefix = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "aggregate_prefix".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let aggregate_prefix = aggregate_prefix.call_relationships().unwrap();
    assert_eq!(aggregate_prefix.call_sites.len(), 1);
    assert_eq!(
        aggregate_prefix.call_sites[0].resolution,
        Resolution::Absent
    );
    assert!(aggregate_prefix.call_sites[0].targets.is_empty());
}

#[test]
fn explicit_function_types_do_not_hide_the_callee_operand() {
    let application = Application;
    let context = ObservationContext::static_analysis(
        "snapshot:named-type-call-fixture",
        "named-type-call-fixture",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/named-type-calls.ll")],
            context,
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "named_type_caller".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let result = result.call_relationships().unwrap();

    assert_eq!(
        result
            .call_sites
            .iter()
            .map(|site| site.resolution)
            .collect::<Vec<_>>(),
        [Resolution::Complete, Resolution::Absent, Resolution::Absent]
    );
    assert_eq!(result.relationships.len(), 1);
    assert_eq!(result.relationships[0].callee_display_name, "returns_pair");
    assert_eq!(
        result
            .call_sites
            .iter()
            .map(|site| {
                snapshot
                    .program_entities()
                    .iter()
                    .find(|entity| entity.id == site.call_site_id)
                    .unwrap()
                    .source_location
                    .as_ref()
                    .unwrap()
                    .line
            })
            .collect::<Vec<_>>(),
        [7, 8, 9]
    );
}

#[test]
fn literal_aggregate_return_types_do_not_end_the_callee_search() {
    let application = Application;
    let context = ObservationContext::static_analysis(
        "snapshot:aggregate-return-call-fixture",
        "aggregate-return-call-fixture",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/aggregate-return-calls.ll")],
            context,
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "aggregate_return_caller".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let result = result.call_relationships().unwrap();

    assert_eq!(
        result
            .call_sites
            .iter()
            .map(|site| site.resolution)
            .collect::<Vec<_>>(),
        [Resolution::Complete, Resolution::Absent]
    );
    assert_eq!(result.relationships.len(), 1);
    assert_eq!(result.relationships[0].callee_display_name, "pair");
    assert_eq!(
        result
            .call_sites
            .iter()
            .map(|site| {
                snapshot
                    .program_entities()
                    .iter()
                    .find(|entity| entity.id == site.call_site_id)
                    .unwrap()
                    .source_location
                    .as_ref()
                    .unwrap()
                    .line
            })
            .collect::<Vec<_>>(),
        [6, 8]
    );
}

#[test]
fn loaded_snapshots_revalidate_derivation_rules_and_call_site_locations() {
    let application = Application;
    let context = ObservationContext::static_analysis(
        "snapshot:revalidation-fixture",
        "revalidation-fixture",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/quoted-brace-call.ll")],
            context,
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    let exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    assert_eq!(
        exported["derivations"][0]["rule"],
        serde_json::json!(CONTRIBUTED_EVIDENCE_TARGET_RULE)
    );
    let load = |value: &serde_json::Value| {
        application
            .load_snapshot_json(&serde_json::to_string(value).unwrap())
            .unwrap_err()
    };

    let mut renamed_rule = exported.clone();
    renamed_rule["derivations"][0]["rule"] = serde_json::json!("same-name-means-target");
    let error = load(&renamed_rule);
    assert!(error.contains("unknown rule"), "unexpected error: {error}");

    let mut orphan_derivation = exported.clone();
    let mut orphan = orphan_derivation["derivations"][0].clone();
    orphan["output_claim_id"] = serde_json::json!("claim:revalidation-fixture:orphan");
    orphan_derivation["derivations"]
        .as_array_mut()
        .unwrap()
        .push(orphan);
    let error = load(&orphan_derivation);
    assert!(
        error.contains("derivations for claims it does not publish"),
        "unexpected error: {error}"
    );

    let call_site_index = exported["program_entities"]
        .as_array()
        .unwrap()
        .iter()
        .position(|entity| entity["kind"] == serde_json::json!("call-site"))
        .unwrap();
    let mut zero_line_entity = exported.clone();
    zero_line_entity["program_entities"][call_site_index]["source_location"]["line"] =
        serde_json::json!(0);
    let error = load(&zero_line_entity);
    assert!(
        error.contains("has no location within its evidence artifact"),
        "unexpected error: {error}"
    );

    let mut moved_entity = exported.clone();
    moved_entity["program_entities"][call_site_index]["source_location"]["line"] =
        serde_json::json!(999);
    let error = load(&moved_entity);
    assert!(
        error.contains("disagree about the call-site location"),
        "unexpected error: {error}"
    );

    let mut zero_line_evidence = exported.clone();
    zero_line_evidence["evidence_records"][0]["source_location"]["line"] = serde_json::json!(0);
    let error = load(&zero_line_evidence);
    assert!(
        error.contains("no location within its evidence artifact"),
        "unexpected error: {error}"
    );

    let callable_index = exported["program_entities"]
        .as_array()
        .unwrap()
        .iter()
        .position(|entity| entity["kind"] == serde_json::json!("callable"))
        .unwrap();
    let mut located_callable = exported.clone();
    located_callable["program_entities"][callable_index]["source_location"] =
        exported["program_entities"][call_site_index]["source_location"].clone();
    let error = load(&located_callable);
    assert!(
        error.contains("carries a source location its evidence does not preserve"),
        "unexpected error: {error}"
    );

    let mut dropped_location = exported.clone();
    dropped_location["program_entities"][call_site_index]["source_location"] =
        serde_json::Value::Null;
    let error = load(&dropped_location);
    assert!(
        error.contains("has no source location"),
        "unexpected error: {error}"
    );
}

#[test]
fn quoted_identifier_braces_do_not_hide_following_indirect_calls() {
    let application = Application;
    let context = ObservationContext::static_analysis(
        "snapshot:quoted-brace-call-fixture",
        "quoted-brace-call-fixture",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/quoted-brace-call.ll")],
            context,
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "quoted_brace_caller".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let result = result.call_relationships().unwrap();

    assert_eq!(result.call_sites.len(), 2);
    assert_eq!(result.call_sites[0].resolution, Resolution::Complete);
    assert_eq!(result.call_sites[1].resolution, Resolution::Absent);
    assert!(result.call_sites[1].targets.is_empty());
    assert_eq!(
        result
            .call_sites
            .iter()
            .map(|site| {
                snapshot
                    .program_entities()
                    .iter()
                    .find(|entity| entity.id == site.call_site_id)
                    .unwrap()
                    .source_location
                    .as_ref()
                    .unwrap()
                    .line
            })
            .collect::<Vec<_>>(),
        [5, 6]
    );
}

#[test]
fn unresolved_call_site_explanations_are_inspectable_in_the_viewer() {
    let application = Application;
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/indirect-calls.ll")],
            fixture_context(),
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "dispatch".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let result = result.call_relationships().unwrap();
    let unresolved = &result.call_sites[0];

    let explanation = application
        .explain_snapshot(&snapshot, &unresolved.explanation_handle)
        .unwrap();
    assert_eq!(
        explanation.call_site_resolution.resolution,
        Resolution::Absent
    );
    assert!(explanation.target_claims.is_empty());
    assert!(explanation.derivations.is_empty());
    assert_eq!(explanation.evidence_records.len(), 1);
    assert_eq!(
        explanation.evidence_records[0].evidence_type,
        "static-indirect-call"
    );

    let html = application.render_snapshot_viewer(&snapshot).unwrap();
    assert!(html.contains(&serde_json::to_string(unresolved).unwrap()));
    assert!(html.contains("Unresolved call site"));
    assert!(html.contains(unresolved.explanation_handle.as_str()));
    assert!(html.contains(explanation.evidence_records[0].id.as_str()));
}

#[test]
fn publishes_each_unresolved_indirect_call_as_a_distinct_call_site() {
    let snapshot = Application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/indirect-calls.ll")],
            fixture_context(),
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();

    let call_sites = snapshot
        .program_entities()
        .iter()
        .filter(|entity| entity.kind == ProgramEntityKind::CallSite)
        .collect::<Vec<_>>();
    let identities = call_sites
        .iter()
        .map(|call_site| &call_site.id)
        .collect::<BTreeSet<_>>();

    assert_eq!(call_sites.len(), 4);
    assert_eq!(identities.len(), 4);
    assert_eq!(
        call_sites
            .iter()
            .map(|call_site| call_site.source_location.as_ref().unwrap().line)
            .collect::<Vec<_>>(),
        [3, 4, 10, 11]
    );

    let resolutions = snapshot.call_site_resolutions();
    assert_eq!(resolutions.len(), 4);
    assert!(
        resolutions
            .iter()
            .all(|site| site.resolution == Resolution::Absent)
    );
    assert_eq!(
        resolutions
            .iter()
            .map(|site| &site.call_site_id)
            .collect::<BTreeSet<_>>(),
        identities
    );
}

/// Publishes one LLVM fixture through the application seam and queries one
/// caller's callees, with each call site's source line.
fn llvm_callee_query(
    build_target: &str,
    fixture: &str,
    caller_name: &str,
) -> (PublishedSnapshot, CallRelationshipsResult, Vec<usize>) {
    let application = Application;
    let context = ObservationContext::static_analysis(
        format!("snapshot:{build_target}"),
        build_target,
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from(fixture)],
            context,
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: caller_name.into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let result = result.call_relationships().unwrap().clone();
    let lines = result
        .call_sites
        .iter()
        .map(|site| {
            snapshot
                .program_entities()
                .iter()
                .find(|entity| entity.id == site.call_site_id)
                .unwrap()
                .source_location
                .as_ref()
                .unwrap()
                .line
        })
        .collect();
    (snapshot, result, lines)
}

fn resolutions(result: &CallRelationshipsResult) -> Vec<Resolution> {
    result
        .call_sites
        .iter()
        .map(|site| site.resolution)
        .collect()
}

fn callable_names(snapshot: &PublishedSnapshot) -> BTreeSet<&str> {
    snapshot
        .program_entities()
        .iter()
        .filter(|entity| entity.kind == ProgramEntityKind::Callable)
        .map(|entity| entity.display_name.as_str())
        .collect()
}

#[test]
fn cast_wrapped_callees_resolve_through_constant_casts() {
    let (_, result, lines) = llvm_callee_query(
        "cast-wrapped-callee-fixture",
        "tests/fixtures/cast-wrapped-callee.ll",
        "cast_wrapped_caller",
    );

    assert_eq!(
        resolutions(&result),
        [
            Resolution::Complete,
            Resolution::Complete,
            Resolution::Absent,
            Resolution::Complete,
            Resolution::Absent,
        ]
    );
    assert_eq!(
        result
            .call_sites
            .iter()
            .map(|site| {
                site.targets
                    .iter()
                    .map(|target| target.callee_display_name.as_str())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [
            vec!["target"],
            vec!["target"],
            Vec::new(),
            vec!["target"],
            Vec::new(),
        ]
    );
    assert_eq!(lines, [5, 6, 7, 8, 9]);
}

#[test]
fn calls_through_a_global_variable_stay_unresolved_and_name_no_callable() {
    let (snapshot, result, lines) = llvm_callee_query(
        "global-variable-callee-fixture",
        "tests/fixtures/global-variable-callee.ll",
        "global_variable_caller",
    );

    assert_eq!(
        resolutions(&result),
        [Resolution::Absent, Resolution::Complete]
    );
    assert!(result.call_sites[0].targets.is_empty());
    assert_eq!(lines, [5, 6]);
    assert_eq!(result.relationships.len(), 1);
    assert_eq!(
        result.relationships[0].callee_display_name,
        "declared_target"
    );
    assert_eq!(
        callable_names(&snapshot),
        BTreeSet::from(["declared_target", "global_variable_caller"])
    );
}

#[test]
fn calls_through_an_alias_to_data_stay_unresolved_and_name_no_callable() {
    let (snapshot, result, lines) = llvm_callee_query(
        "data-alias-callee-fixture",
        "tests/fixtures/data-alias-callee.ll",
        "data_alias_caller",
    );

    assert_eq!(
        resolutions(&result),
        [Resolution::Absent, Resolution::Absent]
    );
    assert!(result.call_sites.iter().all(|site| site.targets.is_empty()));
    assert!(result.relationships.is_empty());
    assert_eq!(lines, [7, 8]);
    assert_eq!(
        callable_names(&snapshot),
        BTreeSet::from(["data_alias_caller"])
    );
}

#[test]
fn alias_and_ifunc_callees_resolve_as_direct_targets_of_their_own_kind() {
    let (snapshot, result, lines) = llvm_callee_query(
        "alias-callee-fixture",
        "tests/fixtures/alias-callees.ll",
        "alias_caller",
    );

    assert_eq!(resolutions(&result), [Resolution::Complete; 8]);
    assert_eq!(
        result
            .relationships
            .iter()
            .map(|relationship| relationship.callee_display_name.as_str())
            .collect::<Vec<_>>(),
        [
            "aliased",
            "resolved",
            "split",
            "cast_aliased",
            "partitioned",
            "wrapped",
            "before_module_asm",
            "before_attributes",
        ]
    );
    assert_eq!(lines, [22, 23, 24, 25, 26, 27, 28, 29]);

    let exported: serde_json::Value =
        serde_json::from_str(&Application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let representations = exported["manifestations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|manifestation| {
            let entity_id = manifestation["entity_id"].as_str().unwrap();
            let display_name = exported["program_entities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entity| entity["id"] == entity_id)
                .unwrap()["display_name"]
                .as_str()
                .unwrap()
                .to_owned();
            (
                display_name,
                manifestation["representation"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        representations,
        BTreeMap::from([
            ("alias_caller".to_owned(), "llvm-function".to_owned()),
            ("aliased".to_owned(), "llvm-alias".to_owned()),
            ("aliasee".to_owned(), "llvm-function".to_owned()),
            ("before_attributes".to_owned(), "llvm-alias".to_owned()),
            ("before_module_asm".to_owned(), "llvm-alias".to_owned()),
            ("cast_aliased".to_owned(), "llvm-alias".to_owned()),
            ("partitioned".to_owned(), "llvm-alias".to_owned()),
            ("resolved".to_owned(), "llvm-ifunc".to_owned()),
            ("resolver".to_owned(), "llvm-function".to_owned()),
            ("split".to_owned(), "llvm-alias".to_owned()),
            ("variadic_aliasee".to_owned(), "llvm-function".to_owned()),
            ("wrapped".to_owned(), "llvm-alias".to_owned()),
        ])
    );
}

#[test]
fn calls_through_an_alias_to_an_unsupported_expression_stay_unresolved() {
    let (snapshot, result, lines) = llvm_callee_query(
        "select-alias-callee-fixture",
        "tests/fixtures/select-alias-callee.ll",
        "select_alias_caller",
    );

    assert_eq!(
        resolutions(&result),
        [Resolution::Absent, Resolution::Absent]
    );
    assert!(result.call_sites.iter().all(|site| site.targets.is_empty()));
    assert!(result.relationships.is_empty());
    assert_eq!(lines, [10, 11]);
    assert_eq!(
        callable_names(&snapshot),
        BTreeSet::from(["function", "select_alias_caller"])
    );
}

#[test]
fn wrapped_callee_operands_resolve_to_the_function_they_wrap() {
    let (snapshot, result, lines) = llvm_callee_query(
        "wrapped-callee-fixture",
        "tests/fixtures/wrapped-callees.ll",
        "wrapped_caller",
    );

    assert_eq!(
        resolutions(&result),
        [
            Resolution::Complete,
            Resolution::Complete,
            Resolution::Absent,
            Resolution::Absent,
        ]
    );
    assert_eq!(
        result
            .relationships
            .iter()
            .map(|relationship| relationship.callee_display_name.as_str())
            .collect::<Vec<_>>(),
        ["real", "real"]
    );
    assert_eq!(lines, [5, 6, 7, 10]);
    assert_eq!(
        callable_names(&snapshot),
        BTreeSet::from(["real", "wrapped_caller"])
    );
    assert_eq!(
        snapshot
            .manifestations()
            .iter()
            .map(|manifestation| manifestation.representation.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["llvm-function"])
    );
}

/// A pointer type may name the address space it points into, so an alias
/// written `alias void (), ptr addrspace(1) @target` still names its aliasee:
/// the `addrspace` clause belongs to the aliasee's type, not to the operand
/// that follows it.
#[test]
fn address_space_qualified_aliasee_types_do_not_hide_the_aliasee() {
    let (snapshot, result, lines) = llvm_callee_query(
        "address-space-alias-fixture",
        "tests/fixtures/address-space-alias.ll",
        "address_space_caller",
    );

    assert_eq!(resolutions(&result), [Resolution::Complete; 4]);
    assert_eq!(
        result
            .relationships
            .iter()
            .map(|relationship| relationship.callee_display_name.as_str())
            .collect::<Vec<_>>(),
        ["opaque_space", "typed_space", "no_space", "space_target"]
    );
    assert_eq!(lines, [17, 18, 19, 20]);
    assert_eq!(
        callable_names(&snapshot),
        BTreeSet::from([
            "address_space_caller",
            "no_space",
            "opaque_space",
            "plain_target",
            "space_target",
            "typed_space",
        ])
    );
}
