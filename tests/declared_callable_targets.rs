//! A direct target claim names a callable the evidence source declared.
//!
//! Issue #16 stopped the LLVM contributor inventing a callable for a call
//! through a global variable, but the published snapshot kept no trace of
//! which callable manifestations came from a declaration, so a hand-edited
//! export could restore the very claim #16 removed. Since #19 every
//! contributed callable carries contributor-identity evidence at its
//! declaration line, and a manifestation a target claim introduced carries
//! none; these tests hold that a static direct-call claim may only name the
//! former.
//!
//! Every tamper goes through all three doors into a published snapshot, so a
//! consumer who deserializes an export gets the answer the loader gives.

use gloom::app::{Application, NamedQuery};
use gloom::{
    EvidenceSupport, LlvmTextContributor, ObservationContext, ProgramEntityKind, PublishedSnapshot,
    Resolution,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::PathBuf;

const GLOBAL_VARIABLE_FIXTURE: &str = "tests/fixtures/global-variable-callee.ll";
const MISSING_DECLARATION: &str =
    "but no contributor-identity evidence declares that callable in acquired input";
const NOT_A_CALLABLE_KIND: &str = "is not a declared callable kind";

fn publish(fixture: &str, build_target: &str) -> PublishedSnapshot {
    let context = ObservationContext::static_analysis(
        format!("snapshot:{build_target}"),
        build_target,
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    );
    Application
        .publish_snapshot(
            &[PathBuf::from(fixture)],
            context,
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap()
}

fn exported(snapshot: &PublishedSnapshot) -> Value {
    serde_json::from_str(&Application.export_snapshot_json(snapshot).unwrap()).unwrap()
}

/// Applies one hand edit to a coherent export and reports how the crate answers
/// a consumer who reads the result back, insisting that every door — the
/// loader, `from_str`, and `from_value` — answers identically.
fn rejection(fixture: &str, build_target: &str, edit: impl FnOnce(&mut Value)) -> String {
    let mut document = exported(&publish(fixture, build_target));
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
    loaded
}

fn entry_mut<'a>(document: &'a mut Value, collection: &str, id: &str) -> &'a mut Value {
    document[collection]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|entry| entry["id"] == id)
        .unwrap_or_else(|| panic!("{collection} must contain '{id}'"))
}

/// The tampering sequence issue #25 describes, against a real export of
/// `global-variable-callee.ll`.
///
/// The module calls through `@handler`, a global variable, and separately
/// calls `@declared_target`, which it declares. The edit restores the false
/// claim that the first call reaches a callable named `handler`: it introduces
/// the `handler` entity and manifestation the module never declares — exactly
/// the manifestation Gloom itself published before #16 — moves the direct
/// target claim and its evidence onto the global-variable call site, swaps the
/// two resolutions along with the completeness basis that closes one of them,
/// and rewrites the relationship and the projection to agree throughout. The
/// document is internally consistent in every respect the earlier rules check;
/// what it cannot supply is the declaration evidence a direct call rests on.
#[test]
fn a_callable_invented_by_a_target_claim_never_becomes_a_published_snapshot() {
    let error = rejection(
        GLOBAL_VARIABLE_FIXTURE,
        "tampered-global-variable-callee",
        |document| {
            let snapshot = document["program_snapshot"]["id"]
                .as_str()
                .unwrap()
                .to_owned();
            let handler_entity = format!("entity:{snapshot}:input:0:callable:2");
            let handler_manifestation = format!("manifestation:{snapshot}:input:0:callable:2");
            let unresolved_site = format!("entity:{snapshot}:input:0:call-site:0");
            let input = format!("input:{snapshot}:0");
            let context = document["observation_contexts"][0]["id"].clone();

            document["program_entities"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "id": handler_entity,
                    "display_name": "handler",
                    "kind": "callable",
                }));
            document["manifestations"]
                .as_array_mut()
                .unwrap()
                .push(json!({
                    "id": handler_manifestation,
                    "entity_id": handler_entity,
                    "acquired_input_id": input,
                    "contributor_callable_id": "handler",
                    "observation_context_id": context,
                    "representation": "llvm-function",
                    "defined": false,
                }));

            let claim = &mut document["target_claims"][0];
            claim["call_site_id"] = json!(unresolved_site);
            claim["target_manifestation_id"] = json!(handler_manifestation);
            let claim_evidence = claim["evidence_ids"][0].as_str().unwrap().to_owned();

            let evidence = entry_mut(document, "evidence_records", &claim_evidence);
            evidence["subject_entity_id"] = json!(unresolved_site);
            evidence["related_manifestation_ids"] = json!([handler_manifestation]);
            evidence["source_location"]["line"] = json!(5);

            // The claimed site now closes its target set, and the site that
            // lost the claim no longer may: a completeness basis belongs to
            // exactly the resolution that declares Complete.
            let basis = entry_mut(
                document,
                "evidence_records",
                &format!("evidence:{snapshot}:input:0:call-site:1"),
            )
            .as_object_mut()
            .unwrap()
            .remove("completeness_basis")
            .unwrap();
            entry_mut(
                document,
                "evidence_records",
                &format!("evidence:{snapshot}:input:0:call-site:0"),
            )["completeness_basis"] = basis;

            for resolution in document["call_site_resolutions"].as_array_mut().unwrap() {
                resolution["resolution"] = if resolution["call_site_id"] == *unresolved_site {
                    json!("complete")
                } else {
                    json!("absent")
                };
            }

            let relationship = &mut document["call_graph_projection"]["relationships"][0];
            relationship["call_site_id"] = json!(unresolved_site);
            relationship["callee_entity_id"] = json!(handler_entity);
            relationship["callee_display_name"] = json!("handler");
            relationship["resolution"] = json!("complete");
            relationship["explanation_handle"] = json!(format!("explanation:{unresolved_site}"));

            let projected = document["call_graph_projection"]["call_sites"]
                .as_array_mut()
                .unwrap();
            let mut targets = projected[1]["targets"].take();
            targets[0]["callee_entity_id"] = json!(handler_entity);
            targets[0]["callee_display_name"] = json!("handler");
            projected[0]["targets"] = targets;
            projected[0]["resolution"] = json!("complete");
            projected[1]["targets"] = json!([]);
            projected[1]["resolution"] = json!("absent");
        },
    );

    assert!(error.contains(MISSING_DECLARATION), "{error}");
    assert!(error.contains("as a static direct-call target"), "{error}");
}

/// The declaration evidence is what the claim rests on, so removing it alone
/// is enough: the manifestation the claim names becomes one nothing but the
/// claim asserts.
#[test]
fn a_direct_target_claim_whose_declaration_evidence_is_removed_never_becomes_a_published_snapshot()
{
    let error = rejection(
        GLOBAL_VARIABLE_FIXTURE,
        "stripped-global-variable-callee",
        |document| {
            let target = document["target_claims"][0]["target_manifestation_id"].clone();
            document["evidence_records"]
                .as_array_mut()
                .unwrap()
                .retain(|evidence| {
                    evidence["support"] != "contributor-identity"
                        || evidence["related_manifestation_ids"][0] != target
                });
        },
    );

    assert!(error.contains(MISSING_DECLARATION), "{error}");
}

/// A direct call reaches a callable global: a function, an alias, or an ifunc.
/// A manifestation written as anything else is not one a call instruction can
/// name, whatever evidence stands behind it.
#[test]
fn a_direct_target_claim_naming_an_uncallable_kind_never_becomes_a_published_snapshot() {
    let error = rejection(
        GLOBAL_VARIABLE_FIXTURE,
        "miskinded-global-variable-callee",
        |document| {
            let target = document["target_claims"][0]["target_manifestation_id"]
                .as_str()
                .unwrap()
                .to_owned();
            entry_mut(document, "manifestations", &target)["representation"] =
                json!("llvm-global-variable");
        },
    );

    assert!(error.contains(NOT_A_CALLABLE_KIND), "{error}");
}

/// Aliases and ifuncs are callable globals the module declares, so the
/// contributor declares them too, at the line that writes them, rather than
/// letting a target claim introduce them.
#[test]
fn aliases_and_ifuncs_are_declared_callables_of_their_own_kind() {
    let snapshot = publish("tests/fixtures/alias-callees.ll", "declared-alias-callees");
    let names = snapshot
        .program_entities()
        .iter()
        .map(|entity| (entity.id.as_str(), entity.display_name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let declarations = snapshot
        .manifestations()
        .iter()
        .map(|manifestation| {
            let declaration = snapshot
                .evidence_records()
                .iter()
                .find(|evidence| {
                    evidence.support == EvidenceSupport::ContributorIdentity
                        && evidence
                            .related_manifestation_ids
                            .contains(&manifestation.id)
                })
                .unwrap_or_else(|| {
                    panic!("manifestation '{}' has no declaration", manifestation.id)
                });
            assert_eq!(
                declaration.acquired_input_id,
                manifestation.acquired_input_id
            );
            assert_eq!(
                declaration.observation_context_id,
                manifestation.observation_context_id
            );
            (
                names[manifestation.entity_id.as_str()],
                (
                    manifestation.representation.as_str(),
                    declaration.source_location.line,
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();

    assert_eq!(
        declarations,
        BTreeMap::from([
            ("alias_caller", ("llvm-function", 20)),
            ("aliased", ("llvm-alias", 4)),
            ("aliasee", ("llvm-function", 1)),
            ("before_attributes", ("llvm-alias", 33)),
            ("before_module_asm", ("llvm-alias", 11)),
            ("cast_aliased", ("llvm-alias", 8)),
            ("partitioned", ("llvm-alias", 9)),
            ("resolved", ("llvm-ifunc", 5)),
            ("resolver", ("llvm-function", 15)),
            ("split", ("llvm-alias", 6)),
            ("variadic_aliasee", ("llvm-function", 2)),
            ("wrapped", ("llvm-alias", 10)),
        ])
    );
}

/// An alias the module writes over data, around a cycle, or through an
/// expression the parse cannot follow is no callable, so it is declared as
/// none: the new rule closes a hole without inventing callables to fill it.
#[test]
fn aliases_that_reach_no_function_are_declared_as_no_callable() {
    for (fixture, build_target, expected) in [
        (
            "tests/fixtures/data-alias-callee.ll",
            "declared-data-alias",
            vec!["data_alias_caller"],
        ),
        (
            "tests/fixtures/select-alias-callee.ll",
            "declared-select-alias",
            vec!["function", "select_alias_caller"],
        ),
    ] {
        let snapshot = publish(fixture, build_target);
        let callables = snapshot
            .program_entities()
            .iter()
            .filter(|entity| entity.kind == ProgramEntityKind::Callable)
            .map(|entity| entity.display_name.as_str())
            .collect::<Vec<_>>();

        assert_eq!(callables, expected, "{fixture}");
    }
}

/// Every LLVM fixture still publishes, and the export of each is still a
/// published snapshot that re-exports byte for byte.
#[test]
fn every_llvm_fixture_publishes_and_round_trips_unchanged() {
    let mut fixtures = std::fs::read_dir("tests/fixtures")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ll"))
        .collect::<Vec<_>>();
    fixtures.sort();
    assert!(fixtures.len() >= 18, "{fixtures:?}");

    for fixture in fixtures {
        let build_target = format!(
            "round-trip-{}",
            fixture.file_stem().unwrap().to_str().unwrap()
        );
        let snapshot = publish(fixture.to_str().unwrap(), &build_target);
        let text = Application.export_snapshot_json(&snapshot).unwrap();

        let loaded = Application.load_snapshot_json(&text).unwrap();

        assert_eq!(loaded, snapshot, "{}", fixture.display());
        assert_eq!(
            Application.export_snapshot_json(&loaded).unwrap(),
            text,
            "{}",
            fixture.display()
        );
    }
}

/// The queries the fixtures answer are unchanged: a call through a global
/// variable is still unresolved and names no callable, and the declared call
/// beside it is still complete.
#[test]
fn a_call_through_a_global_variable_still_names_no_callable() {
    let snapshot = publish(GLOBAL_VARIABLE_FIXTURE, "declared-global-variable-callee");
    let result = Application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "global_variable_caller".into(),
                caller_entity_id: None,
            },
        )
        .unwrap();

    assert_eq!(
        result
            .call_sites
            .iter()
            .map(|site| site.resolution)
            .collect::<Vec<_>>(),
        [Resolution::Absent, Resolution::Complete]
    );
    assert!(
        !snapshot
            .program_entities()
            .iter()
            .any(|entity| entity.display_name == "handler"),
        "the global variable must name no callable"
    );
}
