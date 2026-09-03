use gloom::app::Application;
use gloom::{
    CONTRIBUTOR_IDENTITY_CORRESPONDENCE_RULE, ContributedCallKind, ContributedCallSite,
    ContributedCallable, ContributedEvidence, ContributedInput, ContributedTargetClaim,
    ContributorIdentity, EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability,
    EvidenceContribution, EvidenceContributor, EvidenceScope, EvidenceSupport, Manifestation,
    ObservationContext, ProgramEntityKind, PublishedSnapshot, Resolution,
};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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

/// A contributor that observes one callable identity, `shared_target`, in three
/// observation contexts of one acquired input, but asserts that identity, by
/// emitting contributor-identity evidence, in only the first
/// `identity_evidenced_contexts` of them. The remaining manifestations exist
/// because target evidence named them, so nothing but resolution and target
/// evidence mentions them.
struct SharedTargetFixture {
    identity_evidenced_contexts: usize,
}

struct FixtureContexts {
    stat: ObservationContext,
    first_runtime: ObservationContext,
    second_runtime: ObservationContext,
}

fn fixture_contexts(context: &ObservationContext) -> FixtureContexts {
    let runtime = |workload: &str| {
        ObservationContext::runtime_analysis(
            context.program_snapshot_id.as_str(),
            context.build_target.clone(),
            context.build_configuration.clone(),
            context.toolchain.clone(),
            context.extraction_method.clone(),
            context.extraction_version.clone(),
            "runtime target tracing",
            workload,
        )
    };
    FixtureContexts {
        stat: context.clone(),
        first_runtime: runtime("first workload"),
        second_runtime: runtime("second workload"),
    }
}

fn representation(index: usize) -> &'static str {
    [
        "static-callable",
        "first-runtime-callable",
        "second-runtime-callable",
    ][index]
}

fn scope(index: usize) -> EvidenceScope {
    if index == 0 {
        EvidenceScope::Static
    } else {
        EvidenceScope::Runtime
    }
}

impl EvidenceContributor for SharedTargetFixture {
    fn identity(&self) -> ContributorIdentity {
        ContributorIdentity {
            name: "fixture.shared-target".into(),
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
        let contexts = fixture_contexts(context);
        let ordered = [
            &contexts.stat,
            &contexts.first_runtime,
            &contexts.second_runtime,
        ];
        let mut callables = vec![ContributedCallable {
            contributor_callable_id: "dispatch".into(),
            display_name: "dispatch".into(),
            defined: true,
            representation: "static-callable".into(),
            observation_context_id: contexts.stat.id.clone(),
            line: 1,
            identity_evidence: evidence(
                "static-callable-identity",
                EvidenceScope::Static,
                EvidenceSupport::ContributorIdentity,
            ),
        }];
        callables.extend(
            ordered
                .iter()
                .take(self.identity_evidenced_contexts)
                .enumerate()
                .map(|(index, observed)| ContributedCallable {
                    contributor_callable_id: "shared_target".into(),
                    display_name: "shared_target".into(),
                    defined: true,
                    representation: representation(index).into(),
                    observation_context_id: observed.id.clone(),
                    line: index + 2,
                    identity_evidence: evidence(
                        "contributed-callable-identity",
                        scope(index),
                        EvidenceSupport::ContributorIdentity,
                    ),
                }),
        );
        Ok(EvidenceContribution {
            input: ContributedInput {
                path: input.display().to_string(),
                evidence_artifact: input.display().to_string(),
                media_type: "application/x-gloom-fixture".into(),
                acquisition_method: "semantic-fixture".into(),
                content_fingerprint: "fixture:shared-target".into(),
            },
            observation_contexts: ordered.iter().map(|observed| (*observed).clone()).collect(),
            callables,
            call_sites: vec![ContributedCallSite {
                kind: ContributedCallKind::Indirect,
                caller_callable_id: "dispatch".into(),
                line: 9,
                observation_context_id: contexts.stat.id.clone(),
                resolution: Resolution::Partial,
                evidence: evidence(
                    "static-indirect-call",
                    EvidenceScope::Static,
                    EvidenceSupport::CallSiteResolution,
                ),
                target_claims: ordered
                    .iter()
                    .enumerate()
                    .map(|(index, observed)| ContributedTargetClaim {
                        target_callable_id: "shared_target".into(),
                        callee_display_name: "shared_target".into(),
                        target_representation: representation(index).into(),
                        observation_context_id: observed.id.clone(),
                        evidence: vec![evidence(
                            "observed-target",
                            scope(index),
                            EvidenceSupport::TargetClaim,
                        )],
                    })
                    .collect(),
            }],
        })
    }
}

fn publish(identity_evidenced_contexts: usize) -> PublishedSnapshot {
    let context = ObservationContext::static_analysis(
        "snapshot:shared-target-fixture",
        "shared-target-fixture",
        "debug fixture",
        "semantic fixture",
        "fixture.shared-target",
        "1",
        "target analysis",
    );
    Application
        .publish_snapshot(
            &[PathBuf::from("shared-target.fixture")],
            context,
            &SharedTargetFixture {
                identity_evidenced_contexts,
            },
        )
        .unwrap()
}

fn shared_target_manifestations(snapshot: &PublishedSnapshot) -> Vec<&Manifestation> {
    snapshot
        .manifestations()
        .iter()
        .filter(|manifestation| manifestation.contributor_callable_id == "shared_target")
        .collect()
}

#[test]
fn correspondence_claims_cite_only_contributor_identity_evidence() {
    let snapshot = publish(3);
    let manifestations = shared_target_manifestations(&snapshot);
    assert_eq!(manifestations.len(), 3);

    assert_eq!(snapshot.correspondence_claims().len(), 1);
    let claim = &snapshot.correspondence_claims()[0];
    assert_eq!(claim.rule, CONTRIBUTOR_IDENTITY_CORRESPONDENCE_RULE);
    assert_eq!(claim.contributor_callable_id, "shared_target");
    assert_eq!(
        claim.manifestation_ids.iter().collect::<BTreeSet<_>>(),
        manifestations
            .iter()
            .map(|manifestation| &manifestation.id)
            .collect::<BTreeSet<_>>()
    );

    let cited = claim
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
    assert_eq!(cited.len(), 3);
    for record in &cited {
        assert_eq!(record.support, EvidenceSupport::ContributorIdentity);
        assert_eq!(record.evidence_type, "contributed-callable-identity");
        let subject = snapshot
            .program_entities()
            .iter()
            .find(|entity| entity.id == record.subject_entity_id)
            .unwrap();
        assert_eq!(subject.kind, ProgramEntityKind::Callable);
        assert_eq!(subject.display_name, "shared_target");
        let related = snapshot
            .manifestations()
            .iter()
            .find(|manifestation| record.related_manifestation_ids.contains(&manifestation.id))
            .unwrap();
        assert_eq!(record.related_manifestation_ids.len(), 1);
        assert_eq!(related.entity_id, subject.id);
        assert_eq!(
            related.observation_context_id,
            record.observation_context_id
        );
    }
    assert_eq!(
        cited
            .iter()
            .flat_map(|record| &record.related_manifestation_ids)
            .collect::<BTreeSet<_>>(),
        claim.manifestation_ids.iter().collect::<BTreeSet<_>>()
    );
}

#[test]
fn target_and_resolution_evidence_are_never_cited_by_a_correspondence_claim() {
    let snapshot = publish(3);
    let claim = &snapshot.correspondence_claims()[0];
    let cited = claim.evidence_ids.iter().collect::<BTreeSet<_>>();

    let target_and_resolution_evidence = snapshot
        .target_claims()
        .iter()
        .flat_map(|target| &target.evidence_ids)
        .chain(
            snapshot
                .call_site_resolutions()
                .iter()
                .flat_map(|resolution| &resolution.evidence_ids),
        )
        .collect::<BTreeSet<_>>();
    assert_eq!(target_and_resolution_evidence.len(), 4);
    assert!(
        target_and_resolution_evidence
            .iter()
            .all(|id| !cited.contains(*id))
    );
    assert!(snapshot.evidence_records().iter().any(|record| {
        record.support == EvidenceSupport::TargetClaim
            && record
                .related_manifestation_ids
                .iter()
                .any(|id| claim.manifestation_ids.contains(id))
    }));
}

#[test]
fn removing_identity_evidence_drops_a_manifestation_from_the_correspondence_claim() {
    let snapshot = publish(2);
    let manifestations = shared_target_manifestations(&snapshot);
    assert_eq!(manifestations.len(), 3);
    let unevidenced = manifestations
        .iter()
        .find(|manifestation| manifestation.representation == "second-runtime-callable")
        .unwrap();

    assert_eq!(snapshot.correspondence_claims().len(), 1);
    let claim = &snapshot.correspondence_claims()[0];
    assert_eq!(claim.manifestation_ids.len(), 2);
    assert!(!claim.manifestation_ids.contains(&unevidenced.id));
    assert_eq!(claim.evidence_ids.len(), 2);
    assert!(
        !snapshot
            .evidence_records()
            .iter()
            .any(
                |record| record.support == EvidenceSupport::ContributorIdentity
                    && record.related_manifestation_ids.contains(&unevidenced.id)
            )
    );

    let projected = &snapshot.call_graph_projection().call_sites[0];
    let dropped_target = projected
        .targets
        .iter()
        .find(|target| {
            snapshot
                .target_claims()
                .iter()
                .find(|claim| claim.id == target.target_claim_id)
                .unwrap()
                .target_manifestation_id
                == unevidenced.id
        })
        .unwrap();
    assert!(dropped_target.correspondence_claim_ids.is_empty());
    assert!(
        projected
            .targets
            .iter()
            .any(|target| target.correspondence_claim_ids == std::slice::from_ref(&claim.id))
    );
}

#[test]
fn correspondence_disappears_below_two_identity_evidenced_manifestations() {
    let snapshot = publish(1);
    let manifestations = shared_target_manifestations(&snapshot);
    assert_eq!(manifestations.len(), 3);
    assert_eq!(
        manifestations
            .iter()
            .map(|manifestation| &manifestation.observation_context_id)
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );

    assert!(snapshot.correspondence_claims().is_empty());
    assert!(
        snapshot
            .call_graph_projection()
            .call_sites
            .iter()
            .flat_map(|site| &site.targets)
            .all(|target| target.correspondence_claim_ids.is_empty())
    );
    assert!(
        snapshot
            .call_graph_projection()
            .relationships
            .iter()
            .all(|relationship| relationship.correspondence_claim_ids.is_empty())
    );
}

#[test]
fn hand_edited_correspondence_evidence_without_identity_support_is_rejected_on_load() {
    let application = Application;
    let snapshot = publish(3);
    let exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();

    let target_evidence_id = &snapshot.target_claims()[0].evidence_ids[0];
    let mut target_evidence_export = exported.clone();
    target_evidence_export["correspondence_claims"][0]["evidence_ids"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(target_evidence_id));
    let error = application
        .load_snapshot_json(&serde_json::to_string(&target_evidence_export).unwrap())
        .unwrap_err();
    assert!(
        error.contains("instead of contributor-identity support"),
        "unexpected error: {error}"
    );

    let resolution_evidence_id = &snapshot.call_site_resolutions()[0].evidence_ids[0];
    let mut resolution_evidence_export = exported.clone();
    resolution_evidence_export["correspondence_claims"][0]["evidence_ids"] =
        serde_json::json!([resolution_evidence_id]);
    let error = application
        .load_snapshot_json(&serde_json::to_string(&resolution_evidence_export).unwrap())
        .unwrap_err();
    assert!(
        error.contains("instead of contributor-identity support"),
        "unexpected error: {error}"
    );

    let mut dropped_identity_export = exported.clone();
    dropped_identity_export["correspondence_claims"][0]["evidence_ids"]
        .as_array_mut()
        .unwrap()
        .pop();
    let error = application
        .load_snapshot_json(&serde_json::to_string(&dropped_identity_export).unwrap())
        .unwrap_err();
    assert!(
        error.contains("lacks evidence for every manifestation"),
        "unexpected error: {error}"
    );
}

#[test]
fn evidence_subjects_follow_their_support() {
    let application = Application;
    let snapshot = publish(3);
    let exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let identity_evidence_id = snapshot
        .evidence_records()
        .iter()
        .find(|record| record.support == EvidenceSupport::ContributorIdentity)
        .unwrap()
        .id
        .clone();
    let call_site_id = snapshot
        .program_entities()
        .iter()
        .find(|entity| entity.kind == ProgramEntityKind::CallSite)
        .unwrap()
        .id
        .clone();
    let edit =
        |value: &serde_json::Value, id: &str, field: &str, replacement: serde_json::Value| {
            let mut edited = value.clone();
            *edited["evidence_records"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|record| record["id"] == serde_json::json!(id))
                .unwrap()
                .get_mut(field)
                .unwrap() = replacement;
            application
                .load_snapshot_json(&serde_json::to_string(&edited).unwrap())
                .unwrap_err()
        };

    let error = edit(
        &exported,
        identity_evidence_id.as_str(),
        "subject_entity_id",
        serde_json::json!(call_site_id),
    );
    assert!(
        error.contains("does not identify a callable"),
        "unexpected error: {error}"
    );

    let error = edit(
        &exported,
        identity_evidence_id.as_str(),
        "support",
        serde_json::json!("call-site-resolution"),
    );
    assert!(
        error.contains("does not identify a call site"),
        "unexpected error: {error}"
    );

    let resolution_evidence_id = &snapshot.call_site_resolutions()[0].evidence_ids[0];
    let error = edit(
        &exported,
        resolution_evidence_id.as_str(),
        "support",
        serde_json::json!("contributor-identity"),
    );
    assert!(
        error.contains("does not identify a callable"),
        "unexpected error: {error}"
    );
}
