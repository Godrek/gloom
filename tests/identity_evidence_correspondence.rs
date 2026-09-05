use gloom::app::Application;
use gloom::{
    CONTRIBUTOR_IDENTITY_CORRESPONDENCE_RULE, CallableIdentityScope, ContributedCallKind,
    ContributedCallSite, ContributedCallable, ContributedEvidence, ContributedEvidenceLocation,
    ContributedInput, ContributedTargetClaim, ContributorCallSiteId, ContributorCallableIdentity,
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

fn callable_identity(value: &str) -> ContributorCallableIdentity {
    ContributorCallableIdentity::new(value, CallableIdentityScope::LinkageNamespace).unwrap()
}

/// One observation context of the fixture input, with the representation and
/// evidence scope the contributor uses there.
struct ObservedContext {
    context: ObservationContext,
    representation: &'static str,
    scope: EvidenceScope,
}

impl ObservedContext {
    fn identity_evidence(&self, artifact: &str, line: usize) -> ContributedEvidence {
        evidence(
            "contributed-callable-identity",
            self.scope,
            EvidenceSupport::ContributorIdentity,
            artifact,
            line,
        )
    }

    fn target_evidence(&self, artifact: &str, line: usize) -> ContributedEvidence {
        evidence(
            "observed-target",
            self.scope,
            EvidenceSupport::TargetClaim,
            artifact,
            line,
        )
    }
}

/// The three observation contexts every fixture in this file observes: the
/// publication context and two workload-qualified runtime contexts.
fn observed_contexts(publication: &ObservationContext) -> Vec<ObservedContext> {
    let runtime = |workload: &str| {
        ObservationContext::runtime_analysis(
            publication.program_snapshot_id.as_str(),
            publication.build_target.clone(),
            publication.build_configuration.clone(),
            publication.toolchain.clone(),
            publication.extraction_method.clone(),
            publication.extraction_version.clone(),
            "runtime target tracing",
            workload,
        )
    };
    vec![
        ObservedContext {
            context: publication.clone(),
            representation: "static-callable",
            scope: EvidenceScope::Static,
        },
        ObservedContext {
            context: runtime("first workload"),
            representation: "first-runtime-callable",
            scope: EvidenceScope::Runtime,
        },
        ObservedContext {
            context: runtime("second workload"),
            representation: "second-runtime-callable",
            scope: EvidenceScope::Runtime,
        },
    ]
}

fn shared_target(observed: &ObservedContext, line: usize, artifact: &str) -> ContributedCallable {
    ContributedCallable {
        callable_identity: callable_identity("shared_target"),
        display_name: "shared_target".into(),
        defined: true,
        representation: observed.representation.into(),
        observation_context_id: observed.context.id.clone(),
        line,
        identity_evidence: observed.identity_evidence(artifact, line),
    }
}

fn fixture_input(input: &Path, fingerprint: &str) -> ContributedInput {
    ContributedInput {
        path: input.display().to_string(),
        evidence_artifact: input.display().to_string(),
        media_type: "application/x-gloom-fixture".into(),
        acquisition_method: "semantic-fixture".into(),
        content_fingerprint: fingerprint.into(),
    }
}

fn dispatch(observed: &ObservedContext, artifact: &str) -> ContributedCallable {
    ContributedCallable {
        callable_identity: callable_identity("dispatch"),
        display_name: "dispatch".into(),
        defined: true,
        representation: observed.representation.into(),
        observation_context_id: observed.context.id.clone(),
        line: 1,
        identity_evidence: observed.identity_evidence(artifact, 1),
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
        let observed = observed_contexts(context);
        let artifact = input.display().to_string();
        let mut callables = vec![dispatch(&observed[0], &artifact)];
        callables.extend(
            observed
                .iter()
                .take(self.identity_evidenced_contexts)
                .enumerate()
                .map(|(index, observed)| shared_target(observed, index + 2, &artifact)),
        );
        Ok(EvidenceContribution {
            input: fixture_input(input, "fixture:shared-target"),
            observation_contexts: observed
                .iter()
                .map(|observed| observed.context.clone())
                .collect(),
            callables,
            call_sites: vec![ContributedCallSite {
                contributor_call_site_id: ContributorCallSiteId::new("dispatch:9").unwrap(),
                kind: ContributedCallKind::Indirect,
                caller_callable_identity: callable_identity("dispatch"),
                line: 9,
                observation_context_id: observed[0].context.id.clone(),
                resolution: Resolution::Partial,
                evidence: evidence(
                    "static-indirect-call",
                    EvidenceScope::Static,
                    EvidenceSupport::CallSiteResolution,
                    &artifact,
                    9,
                ),
                target_claims: observed
                    .iter()
                    .map(|observed| ContributedTargetClaim {
                        target_callable_identity: callable_identity("shared_target"),
                        callee_display_name: "shared_target".into(),
                        target_representation: observed.representation.into(),
                        observation_context_id: observed.context.id.clone(),
                        evidence: vec![observed.target_evidence(&artifact, 9)],
                    })
                    .collect(),
            }],
            call_site_attachments: Vec::new(),
        })
    }
}

/// A contributor that declares `shared_target` twice in its publication context
/// and once in each runtime context. Coalescing those duplicates would publish
/// three manifestations asserted by four identity evidence records.
struct DuplicateCallableFixture;

impl EvidenceContributor for DuplicateCallableFixture {
    fn identity(&self) -> ContributorIdentity {
        ContributorIdentity {
            name: "fixture.duplicate-callable".into(),
            version: "1".into(),
            contract_version: EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION.into(),
            capabilities: vec![EvidenceCapability::CallableManifestations],
        }
    }

    fn contribute(
        &self,
        input: &Path,
        context: &ObservationContext,
    ) -> Result<EvidenceContribution, String> {
        let observed = observed_contexts(context);
        let artifact = input.display().to_string();
        let mut callables = vec![
            shared_target(&observed[0], 2, &artifact),
            shared_target(&observed[0], 3, &artifact),
        ];
        callables.extend(
            observed
                .iter()
                .skip(1)
                .enumerate()
                .map(|(index, observed)| shared_target(observed, index + 4, &artifact)),
        );
        Ok(EvidenceContribution {
            input: fixture_input(input, "fixture:duplicate-callable"),
            observation_contexts: observed
                .iter()
                .map(|observed| observed.context.clone())
                .collect(),
            callables,
            call_sites: Vec::new(),
            call_site_attachments: Vec::new(),
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
        .filter(|manifestation| {
            manifestation.contributor_callable_identity.as_str() == "shared_target"
        })
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
    assert_eq!(
        claim.contributor_callable_identity.as_str(),
        "shared_target"
    );
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
    // One linkage-namespace identity is aggregated into one program entity only
    // within one observation context: the namespace an identity is joined in is
    // the context's, which names one build target. Across contexts the
    // manifestations stay separate entities and are related by a correspondence
    // claim, as ADR 0002 requires.
    let entities = manifestations
        .iter()
        .map(|manifestation| manifestation.entity_id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        entities.len(),
        manifestations.len(),
        "one identity must not aggregate across observation contexts"
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

#[test]
fn duplicate_contributed_callable_identities_are_rejected() {
    let context = ObservationContext::static_analysis(
        "snapshot:duplicate-callable-fixture",
        "duplicate-callable-fixture",
        "debug fixture",
        "semantic fixture",
        "fixture.duplicate-callable",
        "1",
        "target analysis",
    );
    let error = Application
        .publish_snapshot(
            &[PathBuf::from("duplicate-callable.fixture")],
            context,
            &DuplicateCallableFixture,
        )
        .unwrap_err();

    assert!(
        error.contains("duplicate contributions in observation context"),
        "unexpected error: {error}"
    );
}

#[test]
fn snapshots_reject_more_than_one_identity_record_for_one_manifestation() {
    let application = Application;
    let snapshot = publish(3);
    let exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();

    let mut duplicated_record = exported["evidence_records"]
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["support"] == serde_json::json!("contributor-identity"))
        .unwrap()
        .clone();
    let duplicate_id = format!("{}:duplicate", duplicated_record["id"].as_str().unwrap());
    duplicated_record["id"] = serde_json::json!(duplicate_id);
    let mut duplicated = exported;
    duplicated["evidence_records"]
        .as_array_mut()
        .unwrap()
        .push(duplicated_record);

    let error = application
        .load_snapshot_json(&serde_json::to_string(&duplicated).unwrap())
        .unwrap_err();
    assert!(
        error.contains("more than one contributor-identity evidence record"),
        "unexpected error: {error}"
    );
}

fn identity_evidence_id_for(snapshot: &PublishedSnapshot, contributor_callable_id: &str) -> String {
    snapshot
        .evidence_records()
        .iter()
        .find(|record| {
            record.support == EvidenceSupport::ContributorIdentity
                && record.related_manifestation_ids.iter().any(|id| {
                    snapshot.manifestations().iter().any(|manifestation| {
                        manifestation.id == *id
                            && manifestation.contributor_callable_identity.as_str()
                                == contributor_callable_id
                    })
                })
        })
        .unwrap()
        .id
        .to_string()
}

#[test]
fn correspondence_claims_reject_identity_evidence_for_another_callable_on_load() {
    let application = Application;
    let snapshot = publish(3);
    let mut exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let foreign_evidence_id = identity_evidence_id_for(&snapshot, "dispatch");
    assert!(
        !snapshot.correspondence_claims()[0]
            .evidence_ids
            .iter()
            .any(|id| id.as_str() == foreign_evidence_id)
    );

    exported["correspondence_claims"][0]["evidence_ids"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!(foreign_evidence_id));

    let error = application
        .load_snapshot_json(&serde_json::to_string(&exported).unwrap())
        .unwrap_err();
    assert!(
        error.contains("does not cite exactly the contributor-identity evidence"),
        "unexpected error: {error}"
    );
}

#[test]
fn correspondence_claims_reject_a_manifestation_dropped_from_standing_identity_evidence_on_load() {
    let application = Application;
    let snapshot = publish(3);
    let mut exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    let dropped = snapshot
        .manifestations()
        .iter()
        .find(|manifestation| manifestation.representation == "second-runtime-callable")
        .unwrap();
    let dropped_evidence_id = snapshot
        .evidence_records()
        .iter()
        .find(|record| {
            record.support == EvidenceSupport::ContributorIdentity
                && record.related_manifestation_ids.contains(&dropped.id)
        })
        .unwrap()
        .id
        .clone();

    let claim = &mut exported["correspondence_claims"][0];
    claim["manifestation_ids"]
        .as_array_mut()
        .unwrap()
        .retain(|id| *id != serde_json::json!(dropped.id));
    claim["evidence_ids"]
        .as_array_mut()
        .unwrap()
        .retain(|id| *id != serde_json::json!(dropped_evidence_id));

    let error = application
        .load_snapshot_json(&serde_json::to_string(&exported).unwrap())
        .unwrap_err();
    assert!(
        error.contains(
            "does not identify every manifestation its contributor-identity evidence asserts"
        ),
        "unexpected error: {error}"
    );
}

#[test]
fn a_deleted_correspondence_claim_its_identity_evidence_still_supports_is_rejected_on_load() {
    let application = Application;
    let snapshot = publish(3);
    let mut exported: serde_json::Value =
        serde_json::from_str(&application.export_snapshot_json(&snapshot).unwrap()).unwrap();
    exported["correspondence_claims"] = serde_json::json!([]);

    let error = application
        .load_snapshot_json(&serde_json::to_string(&exported).unwrap())
        .unwrap_err();
    assert!(
        error.contains("but no correspondence claim"),
        "unexpected error: {error}"
    );
}
