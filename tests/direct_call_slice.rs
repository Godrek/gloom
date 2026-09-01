use gloom::app::{Application, NamedQuery};
use gloom::{LlvmTextContributor, ObservationContext, ProgramEntityKind};
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
    let evidence = snapshot.evidence_records().first().unwrap();
    assert_eq!(claim.call_site_id, call_site.id);
    assert_eq!(claim.target_manifestation_id, callee_manifestation.id);
    assert_eq!(claim.evidence_ids, std::slice::from_ref(&evidence.id));
    assert_eq!(
        claim.observation_context_id,
        evidence.observation_context_id
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
            },
        )
        .unwrap();
    assert_eq!(result.query_name, "callees");
    assert_eq!(result.relationships.len(), 1);
    let relationship = &result.relationships[0];
    assert_eq!(relationship.caller_entity_id, caller.id);
    assert_eq!(relationship.callee_entity_id, callee.id);
    assert_eq!(relationship.call_site_id, call_site.id);
    assert!(!relationship.explanation_handle.as_str().is_empty());
    let compact_result = serde_json::to_string(&result).unwrap();
    assert!(!compact_result.contains(evidence.id.as_str()));
    assert!(!compact_result.contains("derivation"));

    let explanation = application
        .explain_snapshot(&snapshot, &relationship.explanation_handle)
        .unwrap();
    assert_eq!(explanation.target_claim.id, claim.id);
    assert_eq!(explanation.evidence_records, std::slice::from_ref(evidence));
    assert_eq!(explanation.derivation.output_claim_id, claim.id);
    assert_eq!(
        explanation.derivation.input_evidence_ids,
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

    let html = application.render_snapshot_viewer(&snapshot).unwrap();
    assert!(html.contains(&serde_json::to_string(relationship).unwrap()));
    assert!(html.contains("<button type=\"button\" class=\"summary\" aria-expanded=\"false\">"));
    assert!(html.contains("caller"));
    assert!(html.contains("callee"));
    assert!(html.contains(relationship.explanation_handle.as_str()));
    assert!(html.contains(evidence.id.as_str()));

    let legacy = application
        .build(&[PathBuf::from("tests/fixtures/simple.ll")], "clang", &[])
        .unwrap();
    assert_eq!(legacy.schema_version, "1.0");
    assert_eq!(legacy.nodes.len(), 3);
}
