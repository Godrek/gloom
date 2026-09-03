//! A published snapshot is coherent however a consumer obtains one.
//!
//! Publishing validates, and so does deserializing: a hand-edited export never
//! becomes a `PublishedSnapshot`, so it can never be handed back to a query, an
//! explanation, the viewer, or a re-export. These tests drive the tampered
//! documents through serde directly, outside the crate, and check that serde
//! answers exactly what the loader answers.

use gloom::app::Application;
use gloom::{LlvmTextContributor, ObservationContext, PublishedSnapshot};
use std::path::PathBuf;

/// A snapshot published from the direct-call fixture: one caller, one callee,
/// and one completely resolved call site with the evidence behind it.
fn published() -> PublishedSnapshot {
    let context = ObservationContext::static_analysis(
        "snapshot:deserialization-fixture",
        "direct-call-fixture",
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    Application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/direct-call.ll")],
            context,
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap()
}

fn exported() -> serde_json::Value {
    serde_json::from_str(&Application.export_snapshot_json(&published()).unwrap()).unwrap()
}

/// Applies one hand edit to a coherent export and reports how the crate answers
/// a consumer who deserializes the result.
///
/// Every door into a snapshot must answer the same way: `serde_json::from_str`,
/// `serde_json::from_value`, and `Application::load_snapshot_json` all have to
/// reject the document with the message validation reports for it, unadorned by
/// serde's parse position.
fn rejection(edit: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut document = exported();
    edit(&mut document);
    let text = serde_json::to_string(&document).unwrap();

    let loaded = Application.load_snapshot_json(&text).unwrap_err();
    let from_str = serde_json::from_str::<PublishedSnapshot>(&text)
        .unwrap_err()
        .to_string();
    let from_value = serde_json::from_value::<PublishedSnapshot>(document)
        .unwrap_err()
        .to_string();

    assert_eq!(
        from_str, loaded,
        "deserializing a tampered export must fail with the error the loader reports"
    );
    assert_eq!(
        from_value, loaded,
        "deserializing a tampered value must fail with the error the loader reports"
    );
    assert!(
        !loaded.contains("at line"),
        "a validation failure should read as the invariant it broke, not as a parse position: {loaded}"
    );
    loaded
}

#[test]
fn a_coherent_export_deserializes_into_the_snapshot_it_was_exported_from() {
    let snapshot = published();
    let text = Application.export_snapshot_json(&snapshot).unwrap();

    let deserialized: PublishedSnapshot = serde_json::from_str(&text).unwrap();

    assert_eq!(deserialized, snapshot);
    assert_eq!(
        Application.export_snapshot_json(&deserialized).unwrap(),
        text
    );
}

#[test]
fn a_document_of_another_schema_never_becomes_a_snapshot() {
    let error = rejection(|document| {
        document["schema_version"] = serde_json::json!("1.0");
    });

    assert!(error.contains("unsupported snapshot schema"), "{error}");
}

#[test]
fn an_acquired_input_the_snapshot_never_acquired_never_becomes_a_snapshot() {
    let error = rejection(|document| {
        document["acquired_inputs"][0]["id"] = serde_json::json!("input:forged");
    });

    assert!(
        error.contains("acquired input 'input:forged' is not identified as"),
        "{error}"
    );
}

#[test]
fn evidence_reclassified_onto_another_kind_of_support_never_becomes_a_snapshot() {
    let error = rejection(|document| {
        let target_evidence_id = document["target_claims"][0]["evidence_ids"][0].clone();
        let evidence = document["evidence_records"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|evidence| evidence["id"] == target_evidence_id)
            .unwrap();
        evidence["support"] = serde_json::json!("contributor-identity");
    });

    assert!(
        error.contains("contributor-identity evidence")
            && error.contains("does not identify a callable"),
        "{error}"
    );
}

#[test]
fn complete_resolution_stripped_of_its_completeness_basis_never_becomes_a_snapshot() {
    let error = rejection(|document| {
        for evidence in document["evidence_records"].as_array_mut().unwrap() {
            evidence
                .as_object_mut()
                .unwrap()
                .remove("completeness_basis");
        }
    });

    assert!(
        error.contains("declares Complete resolution without a completeness basis"),
        "{error}"
    );
}

#[test]
fn a_claim_derived_by_an_invented_rule_never_becomes_a_snapshot() {
    let error = rejection(|document| {
        document["derivations"][0]["rule"] = serde_json::json!("target-from-matching-names");
    });

    assert!(
        error.contains("uses unknown rule 'target-from-matching-names'"),
        "{error}"
    );
}

#[test]
fn a_projection_that_contradicts_its_claims_never_becomes_a_snapshot() {
    let error = rejection(|document| {
        let caller_entity_id =
            document["call_graph_projection"]["relationships"][0]["caller_entity_id"].clone();
        document["call_graph_projection"]["relationships"][0]["callee_entity_id"] =
            caller_entity_id;
    });

    assert!(error.contains("does not match its target claim"), "{error}");
}
