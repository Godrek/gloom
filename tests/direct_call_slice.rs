use gloom::app::{Application, NamedQuery};
use gloom::{
    EvidenceScope, EvidenceSupport, LlvmTextContributor, ObservationContext, ProgramEntityKind,
};
use std::path::PathBuf;

#[test]
fn explains_one_direct_call_from_evidence_to_projection() {
    let application = Application;
    let context = ObservationContext::static_analysis(
        "snapshot:direct-call-fixture",
        "direct-call-fixture",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    let other_target_context = ObservationContext::static_analysis(
        "snapshot:direct-call-fixture",
        "another-target",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    assert_ne!(context.id, other_target_context.id);
    let contributor = LlvmTextContributor::new("clang", &[]);

    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/direct-call.ll")],
            context,
            &contributor,
        )
        .unwrap();

    let caller = snapshot
        .program_entities()
        .iter()
        .find(|entity| {
            entity.kind == ProgramEntityKind::Callable && entity.display_name == "caller"
        })
        .unwrap();
    let callee = snapshot
        .program_entities()
        .iter()
        .find(|entity| {
            entity.kind == ProgramEntityKind::Callable && entity.display_name == "callee"
        })
        .unwrap();
    let call_site = snapshot
        .program_entities()
        .iter()
        .find(|entity| entity.kind == ProgramEntityKind::CallSite)
        .unwrap();
    let callee_manifestation = snapshot
        .manifestations()
        .iter()
        .find(|manifestation| manifestation.entity_id == callee.id)
        .unwrap();

    assert_ne!(caller.id, callee.id);
    assert_ne!(caller.id, call_site.id);
    assert_ne!(callee.id, call_site.id);
    assert_ne!(callee.id.as_str(), callee_manifestation.id.as_str());
    assert!(
        caller
            .id
            .as_str()
            .contains(snapshot.program_snapshot().id.as_str()),
        "entity identity should be scoped to its program snapshot"
    );

    let claim = snapshot.target_claims().first().unwrap();
    let target_evidence = snapshot
        .evidence_records()
        .iter()
        .find(|evidence| claim.evidence_ids.contains(&evidence.id))
        .unwrap();
    let call_site_resolution = snapshot.call_site_resolutions().first().unwrap();
    let resolution_evidence = snapshot
        .evidence_records()
        .iter()
        .find(|evidence| call_site_resolution.evidence_ids.contains(&evidence.id))
        .unwrap();
    assert_eq!(claim.call_site_id, call_site.id);
    assert_eq!(claim.target_manifestation_id, callee_manifestation.id);
    assert_eq!(
        claim.evidence_ids,
        std::slice::from_ref(&target_evidence.id)
    );
    assert_eq!(target_evidence.evidence_type, "static-direct-call");
    assert_eq!(resolution_evidence.evidence_type, "static-call-site");
    assert_eq!(target_evidence.scope, EvidenceScope::Static);
    assert_eq!(target_evidence.support, EvidenceSupport::TargetClaim);
    assert_eq!(resolution_evidence.scope, EvidenceScope::Static);
    assert_eq!(
        resolution_evidence.support,
        EvidenceSupport::CallSiteResolution
    );
    assert_eq!(
        claim.observation_context_id,
        target_evidence.observation_context_id
    );

    let observation = snapshot.observation_contexts().first().unwrap();
    assert_eq!(claim.observation_context_id, observation.id);
    assert_eq!(
        observation.program_snapshot_id,
        snapshot.program_snapshot().id
    );
    assert_eq!(observation.build_target, "direct-call-fixture");
    assert_eq!(observation.build_configuration, "debug fixture");
    assert_eq!(observation.toolchain, "textual LLVM IR");
    assert_eq!(observation.extraction_method, "gloom.llvm-text");
    assert_eq!(observation.extraction_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(observation.analysis_stage, "llvm-ir extraction");
    assert_eq!(observation.runtime_workload, None);

    let result = application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "caller".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();
    let result = result.call_relationships().unwrap();
    assert_eq!(result.query_name, "callees");
    assert_eq!(result.relationships.len(), 1);
    let relationship = &result.relationships[0];
    assert_eq!(relationship.caller_entity_id, caller.id);
    assert_eq!(relationship.callee_entity_id, callee.id);
    assert_eq!(relationship.call_site_id, call_site.id);
    assert_eq!(relationship.target_claim_id, claim.id);
    assert!(!relationship.explanation_handle.as_str().is_empty());
    let compact_result = serde_json::to_string(&result).unwrap();
    assert!(!compact_result.contains(target_evidence.id.as_str()));
    assert!(!compact_result.contains(resolution_evidence.id.as_str()));
    assert!(!compact_result.contains("derivation"));

    let explanation = application
        .explain_snapshot(&snapshot, &relationship.explanation_handle)
        .unwrap();
    assert_eq!(
        explanation.call_site_resolution.resolution,
        gloom::Resolution::Complete
    );
    assert_eq!(explanation.target_claims, std::slice::from_ref(claim));
    assert_eq!(explanation.evidence_records.len(), 2);
    assert!(explanation.evidence_records.contains(target_evidence));
    assert!(explanation.evidence_records.contains(resolution_evidence));
    assert_eq!(explanation.derivations[0].output_claim_id, claim.id);
    assert_eq!(
        explanation.derivations[0].input_evidence_ids,
        claim.evidence_ids
    );

    let json = application.export_snapshot_json(&snapshot).unwrap();
    let exported: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(
        exported["call_graph_projection"]["relationships"][0],
        serde_json::to_value(relationship).unwrap()
    );
    let mut incoherent_export = exported.clone();
    incoherent_export["call_graph_projection"]["relationships"][0]["callee_entity_id"] =
        serde_json::json!(caller.id);
    let error = application
        .load_snapshot_json(&serde_json::to_string(&incoherent_export).unwrap())
        .unwrap_err();
    assert!(error.contains("does not match its target claim"));

    let mut duplicate_claim_export = exported.clone();
    let duplicate_claim = duplicate_claim_export["target_claims"][0].clone();
    duplicate_claim_export["target_claims"]
        .as_array_mut()
        .unwrap()
        .push(duplicate_claim);
    let error = application
        .load_snapshot_json(&serde_json::to_string(&duplicate_claim_export).unwrap())
        .unwrap_err();
    assert!(error.contains("duplicate target-claim identities"));

    let mut invalid_resolution_support = exported.clone();
    let reclassified = invalid_resolution_support["evidence_records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|evidence| evidence["id"] == serde_json::json!(resolution_evidence.id))
        .unwrap();
    reclassified["support"] = serde_json::json!("target-claim");
    reclassified
        .as_object_mut()
        .unwrap()
        .remove("completeness_basis");
    let error = application
        .load_snapshot_json(&serde_json::to_string(&invalid_resolution_support).unwrap())
        .unwrap_err();
    assert!(error.contains("support semantics"));

    let html = application.render_snapshot_viewer(&snapshot).unwrap();
    assert!(html.contains(
        &serde_json::to_string(&snapshot.call_graph_projection().call_sites[0]).unwrap()
    ));
    assert!(html.contains("<button type=\"button\" class=\"summary\" aria-expanded=\"false\">"));
    assert!(html.contains("caller"));
    assert!(html.contains("callee"));
    assert!(html.contains(relationship.explanation_handle.as_str()));
    assert!(html.contains(target_evidence.id.as_str()));

    let legacy = application
        .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
        .unwrap();
    assert_eq!(legacy.schema_version, "1.0");
    assert_eq!(legacy.nodes.len(), 3);
}
