use gloom::app::{Application, NamedQuery};
use gloom::{CallableSelector, LlvmTextContributor, ObservationContext};
use std::collections::BTreeSet;
use std::path::PathBuf;

#[test]
fn byte_identifiers_and_trailing_whitespace_survive_publication_and_roundtrip() {
    let application = Application;
    let snapshot = application
        .publish_snapshot(
            &[PathBuf::from("tests/fixtures/byte-identifiers.ll")],
            ObservationContext::static_analysis(
                "snapshot:byte-identifiers",
                "byte-identifiers",
                "debug fixture",
                "textual LLVM IR",
                "gloom.llvm-text",
                env!("CARGO_PKG_VERSION"),
                "llvm-ir extraction",
            ),
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    let exported = application.export_snapshot_json(&snapshot).unwrap();
    let reloaded = application.load_snapshot_json(&exported).unwrap();

    for snapshot in [&snapshot, &reloaded] {
        let search = application
            .query_snapshot(snapshot, NamedQuery::CallableSearch { label: "".into() })
            .unwrap();
        let callables = &search.callable_search().unwrap().callables;
        assert_eq!(callables.len(), 6);
        assert_eq!(
            callables
                .iter()
                .map(|callable| &callable.entity_id)
                .collect::<BTreeSet<_>>()
                .len(),
            6
        );
        assert!(
            callables
                .iter()
                .any(|callable| callable.display_name == "trailing ")
        );

        let result = application
            .query_snapshot(
                snapshot,
                NamedQuery::Callees {
                    caller: CallableSelector::by_label("caller"),
                },
            )
            .unwrap();
        let relationships = &result.call_relationships().unwrap().relationships;
        assert_eq!(relationships.len(), 5);
        // Declaration order and call order correspond, even for names whose
        // readable labels use the same Unicode replacement character.
        for (offset, relationship) in relationships.iter().enumerate() {
            let declaration = callables
                .iter()
                .find(|callable| {
                    callable.manifestations[0]
                        .declaration
                        .as_ref()
                        .unwrap()
                        .source_location
                        .line
                        == offset + 2
                })
                .unwrap();
            assert_eq!(relationship.callee_entity_id, declaration.entity_id);
        }
    }
}

#[test]
fn prototype_graph_keeps_byte_identifiers_distinct_through_export() {
    let application = Application;
    let document = application
        .build(
            &[PathBuf::from("tests/fixtures/byte-identifiers.ll")],
            "clang",
            &[],
        )
        .unwrap();
    let reloaded = application
        .load_json(&application.export_json(&document).unwrap())
        .unwrap();

    for document in [&document, &reloaded] {
        assert_eq!(document.nodes.len(), 6);
        let ids = document
            .nodes
            .iter()
            .map(|node| node.id.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 6);
        let caller = document
            .nodes
            .iter()
            .find(|node| node.label == "caller")
            .unwrap();
        assert_eq!(document.edges.len(), 5);
        assert!(document.edges.iter().all(|edge| edge.source == caller.id));
        let targets = document
            .edges
            .iter()
            .map(|edge| edge.target.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            targets,
            ids.into_iter().filter(|id| *id != caller.id).collect()
        );
    }
}
