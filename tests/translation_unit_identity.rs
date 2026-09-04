//! Callable identity survives the boundary between translation units.
//!
//! `local-shadow-first.ll` and `local-shadow-second.ll` are two translation
//! units of one program. Each writes an `internal` `@helper` that calls a
//! callable the other never mentions, so the two are different callables that
//! happen to share a label; both were verified with
//! `clang -x ir -mllvm -opaque-pointers -c` and linked together, where they
//! coexist as two local symbols. `@shared_service` has external linkage, is
//! defined in the first unit and declared in the second, and is one symbol at
//! link time — the case that must keep working while the locals are separated.
//!
//! These tests hold that acquisition, search, query, export, and display all
//! treat a display name as a label and never as an identity.

use gloom::app::{Application, NamedQuery, Query};
use gloom::{
    CallPathResult, CallRelationshipsResult, CallableIdentityScope, LlvmTextContributor,
    Manifestation, ObservationContext, ProgramEntity, ProgramEntityKind, PublishedSnapshot,
    SearchedCallable,
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

const FIRST_UNIT: &str = "tests/fixtures/local-shadow-first.ll";
const SECOND_UNIT: &str = "tests/fixtures/local-shadow-second.ll";
const LINKAGE_PREFIXES: &str = "tests/fixtures/linkage-prefixes.ll";

fn context(build_target: &str) -> ObservationContext {
    ObservationContext::static_analysis(
        format!("snapshot:{build_target}"),
        build_target,
        "debug fixture",
        "textual LLVM IR",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "llvm-ir extraction",
    )
}

fn publish_units(build_target: &str, units: &[&str]) -> Result<PublishedSnapshot, String> {
    Application.publish_snapshot(
        &units.iter().map(PathBuf::from).collect::<Vec<_>>(),
        context(build_target),
        &LlvmTextContributor::new("clang", &[]),
    )
}

fn published(build_target: &str) -> PublishedSnapshot {
    publish_units(build_target, &[FIRST_UNIT, SECOND_UNIT]).unwrap()
}

fn callables<'a>(snapshot: &'a PublishedSnapshot, label: &str) -> Vec<&'a ProgramEntity> {
    snapshot
        .program_entities()
        .iter()
        .filter(|entity| entity.kind == ProgramEntityKind::Callable && entity.display_name == label)
        .collect()
}

fn manifestations_of<'a>(snapshot: &'a PublishedSnapshot, label: &str) -> Vec<&'a Manifestation> {
    let entity_ids = callables(snapshot, label)
        .into_iter()
        .map(|entity| entity.id.clone())
        .collect::<BTreeSet<_>>();
    snapshot
        .manifestations()
        .iter()
        .filter(|manifestation| entity_ids.contains(&manifestation.entity_id))
        .collect()
}

/// The callees one caller entity reaches, as `caller -> callee` labels.
fn callees(snapshot: &PublishedSnapshot, caller: &ProgramEntity) -> Vec<String> {
    let result = Application
        .query_snapshot(
            snapshot,
            NamedQuery::Callees {
                caller_name: caller.display_name.clone(),
                caller_entity_id: Some(caller.id.clone()),
            },
        )
        .unwrap();
    result
        .call_relationships()
        .unwrap()
        .relationships
        .iter()
        .map(|relationship| {
            format!(
                "{} -> {}",
                relationship.caller_display_name, relationship.callee_display_name
            )
        })
        .collect()
}

fn relationship_query(snapshot: &PublishedSnapshot, query: NamedQuery) -> CallRelationshipsResult {
    let result = Application.query_snapshot(snapshot, query).unwrap();
    result.call_relationships().unwrap().clone()
}

fn path_query(snapshot: &PublishedSnapshot, query: NamedQuery) -> CallPathResult {
    let result = Application.query_snapshot(snapshot, query).unwrap();
    result.call_path().unwrap().clone()
}

fn search(snapshot: &PublishedSnapshot, label: &str) -> Vec<SearchedCallable> {
    let result = Application
        .query_snapshot(
            snapshot,
            NamedQuery::CallableSearch {
                label: label.into(),
            },
        )
        .unwrap();
    result.callable_search().unwrap().callables.clone()
}

/// Criterion 1: two translation units that each write an identically named
/// local callable produce distinct program entities and distinct
/// manifestations, under distinct contributor callable identities scoped to
/// the acquired input each was read from.
#[test]
fn identically_named_local_callables_are_distinct_entities_and_manifestations() {
    let snapshot = published("local-shadow-identity");

    let helpers = callables(&snapshot, "helper");
    assert_eq!(helpers.len(), 2, "{helpers:?}");
    assert_ne!(helpers[0].id, helpers[1].id);

    let manifestations = manifestations_of(&snapshot, "helper");
    assert_eq!(manifestations.len(), 2, "{manifestations:?}");
    assert_ne!(manifestations[0].id, manifestations[1].id);
    assert_ne!(
        manifestations[0].contributor_callable_identity,
        manifestations[1].contributor_callable_identity,
        "a callable private to one translation unit cannot carry another unit's identity"
    );
    assert_ne!(
        manifestations[0].acquired_input_id,
        manifestations[1].acquired_input_id
    );
    for manifestation in &manifestations {
        assert_eq!(
            manifestation.contributor_callable_identity.scope(),
            CallableIdentityScope::AcquiredInput
        );
        assert!(manifestation.defined);
    }
}

/// Criterion 2: each unit's direct calls reach that unit's own local callable,
/// and that callable's own callee, never the other unit's.
#[test]
fn direct_calls_resolve_to_the_local_callable_of_their_own_translation_unit() {
    let snapshot = published("local-shadow-direct-calls");
    let helpers = callables(&snapshot, "helper");

    let first_helper = helpers
        .iter()
        .find(|helper| helper.id.as_str().contains("input:0"))
        .unwrap();
    let second_helper = helpers
        .iter()
        .find(|helper| helper.id.as_str().contains("input:1"))
        .unwrap();

    assert_eq!(callees(&snapshot, first_helper), ["helper -> first_only"]);
    assert_eq!(callees(&snapshot, second_helper), ["helper -> second_only"]);

    // Each entry point reaches the `helper` of its own unit, by identity.
    for (entry, helper) in [
        ("first_entry", first_helper),
        ("second_entry", second_helper),
    ] {
        let caller = callables(&snapshot, entry)[0];
        let result = relationship_query(
            &snapshot,
            NamedQuery::Callees {
                caller_name: entry.into(),
                caller_entity_id: Some(caller.id.clone()),
            },
        );
        let reached = result
            .relationships
            .iter()
            .find(|relationship| relationship.callee_display_name == "helper")
            .unwrap();
        assert_eq!(reached.callee_entity_id, helper.id);
    }
}

/// Criterion 3: a search on a label answers with identities plus the acquired
/// input and declaration behind each, which is what lets a user pick the
/// callable they meant.
#[test]
fn callable_search_distinguishes_identically_named_locals() {
    let snapshot = published("local-shadow-search");

    let matches = search(&snapshot, "helper");
    assert_eq!(matches.len(), 2, "{matches:?}");
    assert_ne!(matches[0].entity_id, matches[1].entity_id);
    assert_eq!(matches[0].display_name, matches[1].display_name);

    let described = matches
        .iter()
        .map(|matched| {
            let manifestation = &matched.manifestations[0];
            let declaration = manifestation.declaration.as_ref().unwrap();
            (
                manifestation.acquired_input_path.clone(),
                declaration.source_location.line,
                manifestation.contributor_callable_identity.scope(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        described,
        [
            (
                FIRST_UNIT.to_string(),
                12,
                CallableIdentityScope::AcquiredInput
            ),
            (
                SECOND_UNIT.to_string(),
                12,
                CallableIdentityScope::AcquiredInput
            ),
        ]
    );

    // A search that matches nothing is an empty answer, not an error: absence
    // in an open-world projection means only that nothing matched.
    assert!(search(&snapshot, "no_such_callable").is_empty());

    // A name-only callees query refuses to pick, and offers each candidate
    // with the declaration that tells it from the other.
    let ambiguous = Application
        .query_snapshot(
            &snapshot,
            NamedQuery::Callees {
                caller_name: "helper".into(),
                caller_entity_id: None,
            },
        )
        .unwrap_err();
    assert!(
        ambiguous.contains("is ambiguous")
            && ambiguous.contains(&format!("declared at {FIRST_UNIT}:12"))
            && ambiguous.contains(&format!("declared at {SECOND_UNIT}:12")),
        "unexpected error: {ambiguous}"
    );
}

/// Criterion 4: the shared named-query seam's caller, callee, and bounded path
/// behavior never crosses between unrelated local callables.
#[test]
fn named_queries_never_cross_between_unrelated_local_callables() {
    let snapshot = published("local-shadow-queries");
    let helpers = callables(&snapshot, "helper");
    for (helper, expected_caller, expected_callee) in [
        (helpers[0], "first_entry", "first_only"),
        (helpers[1], "second_entry", "second_only"),
    ] {
        let reached = callees(&snapshot, helper);
        assert_eq!(reached, [format!("helper -> {expected_callee}")]);

        let callers = relationship_query(
            &snapshot,
            NamedQuery::Callers {
                callee_name: "helper".into(),
                callee_entity_id: Some(helper.id.clone()),
            },
        );
        assert_eq!(callers.relationships.len(), 1);
        assert_eq!(
            callers.relationships[0].caller_display_name,
            expected_caller
        );
        assert_eq!(callers.relationships[0].callee_entity_id, helper.id);
    }

    let first_entry = callables(&snapshot, "first_entry")[0];
    let first_only = callables(&snapshot, "first_only")[0];
    let second_only = callables(&snapshot, "second_only")[0];

    let local_path = path_query(
        &snapshot,
        NamedQuery::CallPath {
            start_name: "first_entry".into(),
            start_entity_id: Some(first_entry.id.clone()),
            end_name: "first_only".into(),
            end_entity_id: Some(first_only.id.clone()),
            max_relationships: 2,
        },
    );
    assert_eq!(
        local_path
            .path
            .unwrap()
            .iter()
            .map(|relationship| relationship.callee_display_name.as_str())
            .collect::<Vec<_>>(),
        ["helper", "first_only"]
    );

    let cross_unit = path_query(
        &snapshot,
        NamedQuery::CallPath {
            start_name: "first_entry".into(),
            start_entity_id: Some(first_entry.id.clone()),
            end_name: "second_only".into(),
            end_entity_id: Some(second_only.id.clone()),
            max_relationships: 4,
        },
    );
    assert!(cross_unit.path.is_none());

    let bounded = path_query(
        &snapshot,
        NamedQuery::CallPath {
            start_name: "first_entry".into(),
            start_entity_id: Some(first_entry.id.clone()),
            end_name: "first_only".into(),
            end_entity_id: Some(first_only.id.clone()),
            max_relationships: 1,
        },
    );
    assert!(
        bounded.path.is_none(),
        "the relationship bound must be enforced"
    );

    let error = Application
        .query_snapshot(
            &snapshot,
            NamedQuery::CallPath {
                start_name: "helper".into(),
                start_entity_id: None,
                end_name: "first_only".into(),
                end_entity_id: Some(first_only.id.clone()),
                max_relationships: 2,
            },
        )
        .unwrap_err();
    assert!(
        error.contains("is ambiguous")
            && error.contains(&format!("declared at {FIRST_UNIT}:12"))
            && error.contains(&format!("declared at {SECOND_UNIT}:12")),
        "unexpected error: {error}"
    );
}

/// Criterion 5: labels stay the display names a person reads, while identity
/// is carried separately, in the snapshot export, the prototype export, and
/// the viewer.
#[test]
fn export_and_viewer_labels_stay_readable_without_being_identities() {
    let snapshot = published("local-shadow-display");

    let projected = snapshot
        .call_graph_projection()
        .call_sites
        .iter()
        .filter(|site| site.caller_display_name == "helper")
        .collect::<Vec<_>>();
    assert_eq!(projected.len(), 2, "{projected:?}");
    assert_ne!(projected[0].caller_entity_id, projected[1].caller_entity_id);

    // The viewer receives each call site's caller identity and the artifact its
    // resolution evidence was read in, so two same-named callers are told
    // apart without the label being the identity.
    let html = Application.render_snapshot_viewer(&snapshot).unwrap();
    for unit in [FIRST_UNIT, SECOND_UNIT] {
        assert!(html.contains(unit), "the viewer must name {unit}");
    }
    assert!(html.contains(projected[0].caller_entity_id.as_str()));
    assert!(html.contains(projected[1].caller_entity_id.as_str()));
    assert!(html.contains("site.caller_entity_id"));

    // The prototype export keeps the readable label on both nodes while giving
    // each its own identity.
    let document = Application
        .build(
            &[PathBuf::from(FIRST_UNIT), PathBuf::from(SECOND_UNIT)],
            "clang",
            &[],
        )
        .unwrap();
    let helpers = document
        .nodes
        .iter()
        .filter(|node| node.label == "helper")
        .collect::<Vec<_>>();
    assert_eq!(helpers.len(), 2, "{helpers:?}");
    assert_ne!(helpers[0].id, helpers[1].id);
    for helper in &helpers {
        assert!(helper.id.starts_with("callable:fnv1a64:"));
        assert!(!helper.id.contains(&helper.label));
        assert!(!helper.id.contains(FIRST_UNIT));
        assert!(!helper.id.contains(SECOND_UNIT));
    }
    // The one exported callable both units name is one node, as the link makes
    // it one symbol.
    assert_eq!(
        document
            .nodes
            .iter()
            .filter(|node| node.label == "shared_service")
            .count(),
        1
    );

    let reachable = serde_json::to_value(
        Application
            .query(
                document,
                Query::Reachable {
                    start: "first_entry".into(),
                },
            )
            .unwrap(),
    )
    .unwrap();
    let reachable = reachable.as_array().unwrap();
    assert!(reachable.iter().all(|entity| {
        entity["entity_id"]
            .as_str()
            .unwrap()
            .starts_with("callable:")
            && entity["display_name"].as_str().is_some()
    }));
}

/// Criterion 6: nothing corresponds because two callables are spelled the same
/// way. The exported symbol both units name carries one explicitly scoped
/// contributor callable identity in the linkage namespace, so its definition
/// and declaration are manifestations of one entity. The same label on the two
/// input-scoped locals supplies no such evidence and never joins them.
#[test]
fn equal_names_alone_create_no_correspondence_claim() {
    let snapshot = published("local-shadow-correspondence");

    assert!(
        snapshot.correspondence_claims().is_empty(),
        "equal names and similar bodies support no correspondence: {:?}",
        snapshot.correspondence_claims()
    );

    let shared = manifestations_of(&snapshot, "shared_service");
    assert_eq!(shared.len(), 2, "{shared:?}");
    let shared_entities = callables(&snapshot, "shared_service");
    assert_eq!(shared_entities.len(), 1, "{shared_entities:?}");
    assert!(
        shared
            .iter()
            .all(|manifestation| manifestation.entity_id == shared_entities[0].id)
    );
    assert_eq!(
        shared
            .iter()
            .map(|manifestation| manifestation.defined)
            .collect::<Vec<_>>(),
        [true, false]
    );
    assert_ne!(shared[0].acquired_input_id, shared[1].acquired_input_id);
    assert_eq!(
        shared[0].contributor_callable_identity, shared[1].contributor_callable_identity,
        "an exported symbol is one identity in the namespace the link joins it by"
    );
    for manifestation in &shared {
        assert_eq!(
            manifestation.contributor_callable_identity.scope(),
            CallableIdentityScope::LinkageNamespace
        );
    }

    let second_entry = callables(&snapshot, "second_entry")[0];
    let from_second = relationship_query(
        &snapshot,
        NamedQuery::Callees {
            caller_name: "second_entry".into(),
            caller_entity_id: Some(second_entry.id.clone()),
        },
    );
    let shared_call = from_second
        .relationships
        .iter()
        .find(|relationship| relationship.callee_display_name == "shared_service")
        .unwrap();
    assert_eq!(shared_call.callee_entity_id, shared_entities[0].id);

    // A scoped contributor identity establishes sameness within this one
    // observation context. Correspondence remains reserved for manifestations
    // across contexts, so no correspondence claim is invented from the label.
    assert_eq!(snapshot.observation_contexts().len(), 1);
}

/// An identity a contributor scoped to one acquired input may not appear in
/// another, whether a hand edit puts it there or an acquisition does.
#[test]
fn an_input_scoped_callable_identity_may_not_span_acquired_inputs() {
    let snapshot = published("local-shadow-scope");
    let mut document: Value =
        serde_json::from_str(&Application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let joined = manifestations_of(&snapshot, "helper")[0]
        .contributor_callable_identity
        .as_str()
        .to_owned();
    for manifestation in document["manifestations"].as_array_mut().unwrap() {
        if manifestation["contributor_callable_identity"]["scope"]
            == serde_json::json!("acquired-input")
        {
            manifestation["contributor_callable_identity"]["id"] = serde_json::json!(joined);
        }
    }

    let text = serde_json::to_string(&document).unwrap();
    let loaded = Application.load_snapshot_json(&text).unwrap_err();
    assert_eq!(
        serde_json::from_str::<PublishedSnapshot>(&text)
            .unwrap_err()
            .to_string(),
        loaded,
        "deserializing a tampered export must fail with the error the loader reports"
    );
    assert_eq!(
        serde_json::from_value::<PublishedSnapshot>(document)
            .unwrap_err()
            .to_string(),
        loaded,
        "deserializing a tampered value must fail with the error the loader reports"
    );
    assert!(
        loaded.contains("is scoped to acquired input")
            && loaded.contains("is a different callable in another"),
        "unexpected error: {loaded}"
    );

    // Acquiring one translation unit twice is the same conflict reached
    // honestly: the contributor cannot tell the two acquisitions apart, so it
    // asserts one input-scoped identity for what publication treats as two
    // acquired inputs, and the publication is refused rather than merged.
    let repeated = publish_units("local-shadow-repeated", &[FIRST_UNIT, FIRST_UNIT]).unwrap_err();
    assert!(
        repeated.contains("is scoped to acquired input"),
        "unexpected error: {repeated}"
    );
}

/// The linkage keyword decides the identity scope wherever the LangRef allows
/// it to sit, and nowhere else. `linkage-prefixes.ll` writes a linkage keyword
/// beside a preemption specifier, a visibility, a calling convention, return
/// attributes, `unnamed_addr`, a `comdat`, a literal struct return type, an
/// alias, and an ifunc; it also names a callable `@"quoted internal"`, whose
/// spelling contains a linkage keyword that is not one. The expectations below
/// are the object file's own symbol table: `t` and `i` for every local global,
/// `T` and `W` for every exported one, and no symbol at all for the `private`
/// one.
#[test]
fn linkage_decides_identity_scope_wherever_the_keyword_may_sit() {
    let snapshot = publish_units("linkage-prefixes", &[LINKAGE_PREFIXES]).unwrap();

    let scopes = snapshot
        .manifestations()
        .iter()
        .map(|manifestation| {
            let entity = snapshot
                .program_entities()
                .iter()
                .find(|entity| entity.id == manifestation.entity_id)
                .unwrap();
            (
                entity.display_name.as_str(),
                manifestation.contributor_callable_identity.scope(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        scopes,
        [
            ("tricky_local", CallableIdentityScope::AcquiredInput),
            ("struct_local", CallableIdentityScope::AcquiredInput),
            ("exported_odr", CallableIdentityScope::LinkageNamespace),
            ("quoted internal", CallableIdentityScope::AcquiredInput),
            ("alias_local", CallableIdentityScope::AcquiredInput),
            ("alias_public", CallableIdentityScope::LinkageNamespace),
            ("ifunc_local", CallableIdentityScope::AcquiredInput),
            ("resolver", CallableIdentityScope::AcquiredInput),
            ("user", CallableIdentityScope::LinkageNamespace),
        ]
    );
}
