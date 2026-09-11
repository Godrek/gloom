use gloom::app::Application;
use gloom::queries::*;
use gloom::*;
use std::path::{Path, PathBuf};

fn input(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bounded-queries")
        .join(name)
}
fn context() -> ObservationContext {
    ObservationContext::static_analysis(
        "bounded",
        "server",
        "debug",
        "LLVM fixture",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "LLVM IR",
    )
}
fn other_context() -> ObservationContext {
    ObservationContext::static_analysis(
        "bounded",
        "server",
        "release",
        "LLVM fixture",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "LLVM IR",
    )
}
fn runtime_context() -> ObservationContext {
    ObservationContext::runtime_analysis(
        "bounded",
        "server",
        "debug",
        "LLVM fixture",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "runtime trace",
        "fixture workload",
    )
}
struct Mixed;
impl EvidenceContributor for Mixed {
    fn identity(&self) -> ContributorIdentity {
        LlvmTextContributor::new("clang", &[]).identity()
    }
    fn contribute(
        &self,
        input: &Path,
        context: &ObservationContext,
    ) -> Result<EvidenceContribution, String> {
        let llvm = LlvmTextContributor::new("clang", &[]);
        let mut primary = llvm.contribute(input, context)?;
        for site in &mut primary.call_sites {
            if site
                .target_claims
                .iter()
                .any(|claim| claim.callee_display_name == "c")
            {
                site.kind = ContributedCallKind::Indirect;
                site.resolution = Resolution::Partial;
                site.evidence.completeness_basis = None;
                site.evidence.evidence_type = "static-indirect-call".into();
                for claim in &mut site.target_claims {
                    claim.evidence[0].evidence_type = "static-possible-target".into();
                }
            }
        }
        let other = llvm.contribute(input, &other_context())?;
        primary
            .observation_contexts
            .extend(other.observation_contexts);
        primary.callables.extend(other.callables);
        primary.call_sites.extend(other.call_sites);
        let mut runtime = llvm.contribute(input, &runtime_context())?;
        for callable in &mut runtime.callables {
            callable.identity_evidence.scope = EvidenceScope::Runtime;
            callable.identity_evidence.evidence_type = "runtime-callable-identity".into();
        }
        for site in &mut runtime.call_sites {
            site.evidence.scope = EvidenceScope::Runtime;
            site.evidence.evidence_type = "runtime-site".into();
            for target in &mut site.target_claims {
                target.evidence[0].scope = EvidenceScope::Runtime;
                target.evidence[0].evidence_type = "runtime-observed-target".into();
            }
        }
        // A static partial site also has runtime target evidence in another context.
        if let Some(site) = primary
            .call_sites
            .iter_mut()
            .find(|site| site.resolution == Resolution::Partial)
        {
            let mut target = site.target_claims[0].clone();
            target.observation_context_id = runtime_context().id;
            target.evidence[0].scope = EvidenceScope::Runtime;
            target.evidence[0].evidence_type = "runtime-observed-target".into();
            site.target_claims.push(target);
        }
        primary
            .observation_contexts
            .extend(runtime.observation_contexts);
        primary.callables.extend(runtime.callables);
        primary.call_sites.extend(runtime.call_sites);
        Ok(primary)
    }
}
fn snapshot() -> PublishedSnapshot {
    Application
        .publish_snapshot(&[input("calls.ll"), input("local.ll")], context(), &Mixed)
        .unwrap()
}
fn request(query: Investigation) -> BoundedQuery {
    BoundedQuery {
        scope: QueryScope {
            build_target: "server".into(),
            observation_context_ids: vec![context().id],
        },
        resolution_policy: ResolutionPolicy::IncludePossible,
        world: WorldPolicy::Open,
        bounds: QueryBounds {
            max_depth: 4,
            max_results: 100,
            max_steps: 10000,
        },
        query,
    }
}
fn label(name: &str) -> CallableSelector {
    CallableSelector::by_label(name)
}
fn run(snapshot: &PublishedSnapshot, request: &BoundedQuery) -> BoundedQueryResult {
    Application.investigate_snapshot(snapshot, request).unwrap()
}
fn relationships(result: &BoundedQueryResult) -> Vec<&CallRelationship> {
    result
        .items
        .iter()
        .filter_map(|item| {
            if let InvestigationItem::Relationship { relationship } = item {
                Some(relationship)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn callable_search_counts_manifestations_before_identity_filtering() {
    let snapshot = Application
        .publish_snapshot(
            &[input("search-scan.ll")],
            context(),
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    let mut query = request(Investigation::CallableSearch {
        label: String::new(),
    });
    query.bounds.max_steps = 6;
    let bounded = run(&snapshot, &query);
    assert_eq!(bounded.steps, 6);
    assert_eq!(bounded.truncation, vec!["max-steps"]);
    assert!(bounded.items.len() < 3);

    query.bounds.max_steps = 12;
    let complete = run(&snapshot, &query);
    assert_eq!(complete.steps, 12);
    assert!(complete.truncation.is_empty());
    assert_eq!(complete.items.len(), 3);
}

#[test]
fn expansion_budgets_include_target_matching_and_site_classification() {
    let snapshot = Application
        .publish_snapshot(
            &[input("scan-self.ll")],
            context(),
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    for (investigation, scans_before_emission) in [
        (Investigation::Callees { caller: label("self") }, 1),
        (Investigation::Callers { callee: label("self") }, 2),
    ] {
        let mut query = request(investigation);
        for budget in 1..=scans_before_emission {
            query.bounds.max_steps = budget;
            let result = run(&snapshot, &query);
            assert_eq!(result.steps, budget);
            assert_eq!(result.truncation, vec!["max-steps"]);
            assert!(result.items.is_empty());
        }
        query.bounds.max_steps = scans_before_emission + 1;
        let result = run(&snapshot, &query);
        assert_eq!(result.steps, query.bounds.max_steps);
        assert_eq!(result.truncation, vec!["max-steps"]);
        assert_eq!(result.items.len(), 1);
        assert!(matches!(
            result.items[0],
            InvestigationItem::CallSite {
                targets_omitted_by_scope: false,
                unattributed: false,
                ..
            }
        ));
    }
}

#[test]
fn cycle_classification_exhaustion_never_emits_a_potential_cycle() {
    let snapshot = Application
        .publish_snapshot(
            &[input("scan-self.ll")],
            context(),
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap();
    let mut query = request(Investigation::RecursiveCycles { start: label("self") });
    for world in [
        WorldPolicy::Open,
        WorldPolicy::ClosedCallSites {
            call_site_ids: vec![snapshot.call_graph_projection().call_sites[0].call_site_id.clone()],
        },
    ] {
        query.world = world;
        for budget in 1..=2 {
            query.bounds.max_steps = budget;
            let result = run(&snapshot, &query);
            assert_eq!(result.steps, budget);
            assert_eq!(result.truncation, vec!["max-steps"]);
            assert!(result.items.is_empty());
        }
        query.bounds.max_steps = 3;
        let result = run(&snapshot, &query);
        assert_eq!(result.steps, 3);
        assert!(result.truncation.is_empty());
        assert_eq!(result.items.len(), 1);
        assert!(matches!(
            &result.items[0],
            InvestigationItem::Cycle {
                classification: CycleClassification::DefiniteRecursiveCycle,
                closed_call_site_scope: Some(sites),
                ..
            } if sites.len() == 1
        ));
    }
}

#[test]
fn scopes_search_and_selection_and_keeps_duplicate_labels_distinguishable() {
    let snapshot = snapshot();
    let mut query = request(Investigation::CallableSearch {
        label: "helper".into(),
    });
    let result = run(&snapshot, &query);
    assert_eq!(result.items.len(), 2);
    let ids: Vec<_> = result
        .items
        .iter()
        .map(|item| {
            let InvestigationItem::Callable {
                entity_id,
                manifestation,
                ..
            } = item
            else {
                panic!()
            };
            assert_eq!(manifestation.observation_context_id, context().id);
            assert!(manifestation.declaration.is_some());
            entity_id.clone()
        })
        .collect();
    assert_ne!(ids[0], ids[1]);
    query.query = Investigation::Callees {
        caller: label("helper"),
    };
    assert!(
        Application
            .investigate_snapshot(&snapshot, &query)
            .unwrap_err()
            .contains("ambiguous")
    );
    query.query = Investigation::Callees {
        caller: CallableSelector::by_entity_id(ids[0].clone()),
    };
    assert!(run(&snapshot, &query).items.is_empty());
    query.scope.observation_context_ids = vec![other_context().id];
    assert!(Application.investigate_snapshot(&snapshot, &query).is_err());
    query.scope.build_target = "unrelated-target".into();
    assert!(
        Application
            .investigate_snapshot(&snapshot, &query)
            .unwrap_err()
            .contains("does not belong")
    );
    query.scope.observation_context_ids.clear();
    assert!(Application.investigate_snapshot(&snapshot, &query).is_err());
}

#[test]
fn bounded_expansions_preserve_uncertainty_and_complete_only_excludes_possible_edges() {
    let snapshot = snapshot();
    let mut query = request(Investigation::Callees { caller: label("a") });
    query.bounds.max_depth = 2;
    let result = run(&snapshot, &query);
    let edges = relationships(&result);
    assert_eq!(edges.len(), 2);
    assert_eq!(edges[0].callee_display_name, "b");
    assert_eq!(edges[1].callee_display_name, "c");
    assert_eq!(edges[1].resolution, Resolution::Partial);
    assert!(result.items.iter().any(|item| matches!(
        item,
        InvestigationItem::CallSite {
            resolution: Resolution::Absent,
            unattributed: false,
            ..
        }
    )));
    assert_eq!(result.returned_static_call_site_cardinality, 3);
    assert_eq!(result.runtime_invocation_measure, None);
    assert!(result.truncation.contains(&"max-depth".into()));
    for edge in edges {
        let explanation = Application
            .explain_snapshot(&snapshot, &edge.explanation_handle)
            .unwrap();
        assert!(!explanation.evidence_records.is_empty());
        assert!(!explanation.derivations.is_empty());
        assert_eq!(edge.target_observation_context_id, context().id);
    }
    query.resolution_policy = ResolutionPolicy::CompleteOnly;
    let result = run(&snapshot, &query);
    assert_eq!(relationships(&result).len(), 1);
    assert_eq!(result.returned_static_call_site_cardinality, 1);
    query.query = Investigation::Callers { callee: label("c") };
    assert!(run(&snapshot, &query).items.is_empty());
    query.resolution_policy = ResolutionPolicy::IncludePossible;
    let result = run(&snapshot, &query);
    assert!(
        relationships(&result)
            .iter()
            .any(|r| r.caller_display_name == "b")
    );
    assert!(result.items.iter().any(|item| matches!(
        item,
        InvestigationItem::CallSite {
            resolution: Resolution::Absent,
            unattributed: true,
            ..
        }
    )));
}

#[test]
fn path_uses_only_selected_declared_relationships_and_respects_policies_and_limits() {
    let snapshot = snapshot();
    let mut query = request(Investigation::CallPath {
        start: label("a"),
        end: label("c"),
    });
    let result = run(&snapshot, &query);
    let InvestigationItem::Path { relationships } = &result.items[0] else {
        panic!()
    };
    assert_eq!(relationships.len(), 2);
    assert_eq!(relationships[1].resolution, Resolution::Partial);
    assert!(matches!(result.world, WorldPolicy::Open));
    query.resolution_policy = ResolutionPolicy::CompleteOnly;
    assert!(run(&snapshot, &query).items.is_empty());
    query.scope.observation_context_ids = vec![other_context().id];
    assert_eq!(run(&snapshot, &query).items.len(), 1);
    query.bounds.max_depth = 1;
    let result = run(&snapshot, &query);
    assert!(result.items.is_empty());
    assert!(result.truncation.contains(&"max-depth".into()));
    query.bounds.max_steps = 1;
    let result = run(&snapshot, &query);
    assert_eq!(result.steps, 1);
    assert!(result.truncation.contains(&"max-steps".into()));
}

#[test]
fn cycles_separate_complete_closed_site_support_from_conservative_claims() {
    let snapshot = snapshot();
    let mut query = request(Investigation::RecursiveCycles { start: label("a") });
    let result = run(&snapshot, &query);
    assert_eq!(result.items.len(), 1);
    assert!(
        matches!(&result.items[0], InvestigationItem::Cycle { classification: CycleClassification::PotentialRecursiveCycle, closed_call_site_scope: None, relationships } if relationships.len() == 3)
    );
    query.scope.observation_context_ids = vec![other_context().id];
    let result = run(&snapshot, &query);
    let InvestigationItem::Cycle {
        classification,
        closed_call_site_scope,
        relationships,
    } = &result.items[0]
    else {
        panic!()
    };
    assert_eq!(*classification, CycleClassification::DefiniteRecursiveCycle);
    assert_eq!(closed_call_site_scope.as_ref().unwrap().len(), 3);
    for edge in relationships {
        assert!(
            Application
                .explain_snapshot(&snapshot, &edge.explanation_handle)
                .unwrap()
                .evidence_records
                .iter()
                .any(|evidence| evidence.completeness_basis.is_some())
        );
    }
    query.query = Investigation::RecursiveCycles {
        start: label("self"),
    };
    query.bounds.max_depth = 1;
    assert!(
        matches!(&run(&snapshot, &query).items[0], InvestigationItem::Cycle { relationships, .. } if relationships.len() == 1)
    );
}

#[test]
fn closed_world_is_restricted_to_explicit_complete_sites_and_never_closes_callable_search() {
    let snapshot = snapshot();
    let partial = snapshot
        .call_graph_projection()
        .call_sites
        .iter()
        .find(|site| site.resolution == Resolution::Partial)
        .unwrap();
    let complete = snapshot
        .call_graph_projection()
        .call_sites
        .iter()
        .find(|site| {
            site.caller_display_name == "a"
                && site.resolution == Resolution::Complete
                && site.resolution_observation_context_id == context().id
        })
        .unwrap();
    let mut query = request(Investigation::Callees { caller: label("a") });
    query.world = WorldPolicy::ClosedCallSites {
        call_site_ids: vec![partial.call_site_id.clone()],
    };
    assert!(
        Application
            .investigate_snapshot(&snapshot, &query)
            .unwrap_err()
            .contains("complete resolution")
    );
    query.world = WorldPolicy::ClosedCallSites {
        call_site_ids: vec![complete.call_site_id.clone()],
    };
    let result = run(&snapshot, &query);
    assert_eq!(relationships(&result).len(), 1);
    assert!(result.truncation.is_empty());
    query.query = Investigation::CallableSearch { label: "a".into() };
    assert!(
        Application
            .investigate_snapshot(&snapshot, &query)
            .unwrap_err()
            .contains("callable search")
    );
}

#[test]
fn result_and_step_bounds_are_explicit_and_invalid_or_generic_requests_are_rejected() {
    let snapshot = snapshot();
    let mut query = request(Investigation::CallableSearch {
        label: String::new(),
    });
    query.bounds.max_results = 1;
    let result = run(&snapshot, &query);
    assert_eq!(result.items.len(), 1);
    assert!(result.truncation.contains(&"max-results".into()));
    query.bounds.max_results = 0;
    assert!(Application.investigate_snapshot(&snapshot, &query).is_err());
    let mut wire = serde_json::to_value(request(Investigation::CallPath {
        start: label("a"),
        end: label("c"),
    }))
    .unwrap();
    wire["query"]["name"] = "traverse".into();
    assert!(serde_json::from_value::<BoundedQuery>(wire).is_err());
    let mut wire =
        serde_json::to_value(request(Investigation::Callees { caller: label("a") })).unwrap();
    wire["query"]["relationship_kind"] = "all".into();
    assert!(serde_json::from_value::<BoundedQuery>(wire).is_err());
}

#[test]
fn cli_and_application_execute_the_same_request() {
    let snapshot = snapshot();
    let query = request(Investigation::Callees { caller: label("a") });
    let directory = std::env::temp_dir().join(format!("gloom-bounded-cli-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let stored = directory.join("snapshot.json");
    let request_path = directory.join("query.json");
    std::fs::write(
        &stored,
        Application.export_snapshot_json(&snapshot).unwrap(),
    )
    .unwrap();
    std::fs::write(&request_path, serde_json::to_vec(&query).unwrap()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_gloom"))
        .arg("investigate")
        .arg(&stored)
        .arg("--request")
        .arg(&request_path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::to_value(run(&snapshot, &query)).unwrap()
    );
    std::fs::write(&request_path, r#"{"query":{"name":"traverse"}}"#).unwrap();
    let rejected = std::process::Command::new(env!("CARGO_BIN_EXE_gloom"))
        .arg("investigate")
        .arg(&stored)
        .arg("--request")
        .arg(&request_path)
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(rejected.stdout.is_empty());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn runtime_evidence_keeps_its_context_and_never_becomes_a_static_or_invocation_count() {
    let snapshot = snapshot();
    let mut query = request(Investigation::Callees { caller: label("b") });
    query.bounds.max_depth = 1;
    let static_result = run(&snapshot, &query);
    assert_eq!(relationships(&static_result).len(), 1);
    assert!(static_result.items.iter().any(|item| matches!(
        item,
        InvestigationItem::CallSite {
            targets_omitted_by_scope: true,
            ..
        }
    )));
    let b_id = relationships(&static_result)[0].caller_entity_id.clone();
    query
        .scope
        .observation_context_ids
        .push(runtime_context().id);
    query.query = Investigation::Callees {
        caller: CallableSelector::by_entity_id(b_id),
    };
    let mixed = run(&snapshot, &query);
    assert_eq!(relationships(&mixed).len(), 2);
    assert_eq!(mixed.returned_static_call_site_cardinality, 1);
    let runtime_target = relationships(&mixed)
        .into_iter()
        .find(|r| r.target_observation_context_id == runtime_context().id)
        .unwrap();
    assert!(
        Application
            .explain_snapshot(&snapshot, &runtime_target.explanation_handle)
            .unwrap()
            .evidence_records
            .iter()
            .any(|r| r.scope == EvidenceScope::Runtime)
    );
    query.scope.observation_context_ids = vec![runtime_context().id];
    query.query = Investigation::Callees { caller: label("b") };
    let runtime = run(&snapshot, &query);
    assert_eq!(relationships(&runtime).len(), 1);
    assert_eq!(runtime.returned_static_call_site_cardinality, 0);
    assert_eq!(runtime.runtime_invocation_measure, None);
    assert_eq!(
        runtime.observation_contexts[0].runtime_workload.as_deref(),
        Some("fixture workload")
    );
}
