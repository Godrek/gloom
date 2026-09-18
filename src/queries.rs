//! Bounded investigations over declared call relationships, shared by local adapters.
use crate::{
    CallRelationship, CallableSelector, EvidenceScope, EvidenceSupport, ExplanationHandle,
    ObservationContext, ObservationContextId, ProgramEntityId, ProgramSnapshotId,
    PublishedSnapshot, Resolution, SearchedCallableManifestation,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryScope {
    pub build_target: String,
    pub observation_context_ids: Vec<ObservationContextId>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolutionPolicy {
    IncludePossible,
    CompleteOnly,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorldPolicy {
    // An empty struct variant, not a unit variant: internally tagged unit
    // variants bypass `deny_unknown_fields`, silently accepting extra fields.
    Open {},
    /// Completeness applies only to these explicitly enumerated, recorded sites.
    ClosedCallSites {
        call_site_ids: Vec<ProgramEntityId>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryBounds {
    pub max_depth: usize,
    pub max_results: usize,
    pub max_steps: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Investigation {
    CallableSearch {
        label: String,
    },
    Callers {
        callee: CallableSelector,
    },
    Callees {
        caller: CallableSelector,
    },
    CallPath {
        start: CallableSelector,
        end: CallableSelector,
    },
    RecursiveCycles {
        start: CallableSelector,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoundedQuery {
    pub scope: QueryScope,
    pub resolution_policy: ResolutionPolicy,
    pub world: WorldPolicy,
    pub bounds: QueryBounds,
    pub query: Investigation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CycleClassification {
    DefiniteRecursiveCycle,
    PotentialRecursiveCycle,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InvestigationItem {
    Callable {
        entity_id: ProgramEntityId,
        display_name: String,
        manifestation: SearchedCallableManifestation,
    },
    Relationship {
        relationship: CallRelationship,
    },
    CallSite {
        call_site_id: ProgramEntityId,
        caller_entity_id: ProgramEntityId,
        observation_context_id: ObservationContextId,
        resolution: Resolution,
        /// Resolution describes the original site; scope filtering never upgrades it.
        targets_omitted_by_scope: bool,
        /// Incoming unknown targets cannot be attributed to the selected callee.
        unattributed: bool,
        explanation_handle: ExplanationHandle,
    },
    Path {
        relationships: Vec<CallRelationship>,
    },
    Cycle {
        classification: CycleClassification,
        relationships: Vec<CallRelationship>,
        /// Explicit per-cycle closure, justified by each site's resolution evidence.
        closed_call_site_scope: Option<Vec<ProgramEntityId>>,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct BoundedQueryResult {
    pub program_snapshot_id: ProgramSnapshotId,
    pub query_name: String,
    pub projection: String,
    pub relationship_kind: String,
    pub direction: String,
    pub observation_contexts: Vec<ObservationContext>,
    pub resolution_policy: ResolutionPolicy,
    pub world: WorldPolicy,
    pub closed_scope_explanation_handles: Vec<ExplanationHandle>,
    pub bounds: QueryBounds,
    pub items: Vec<InvestigationItem>,
    pub truncation: Vec<String>,
    pub steps: usize,
    /// Distinct static call-site entities referenced by the returned items only.
    pub returned_static_call_site_cardinality: usize,
    /// No result in this API measures runtime invocation frequency.
    pub runtime_invocation_measure: Option<u64>,
}

struct Evaluation<'a> {
    snapshot: &'a PublishedSnapshot,
    request: &'a BoundedQuery,
    contexts: BTreeSet<ObservationContextId>,
    sites: Option<BTreeSet<ProgramEntityId>>,
    result: BoundedQueryResult,
}

impl Evaluation<'_> {
    fn truncated(&mut self, reason: &str) {
        if !self.result.truncation.iter().any(|item| item == reason) {
            self.result.truncation.push(reason.into());
        }
    }

    fn step(&mut self) -> bool {
        if self.result.steps == self.request.bounds.max_steps {
            self.truncated("max-steps");
            false
        } else {
            self.result.steps += 1;
            true
        }
    }

    fn emit(&mut self, item: InvestigationItem) -> bool {
        if self.result.items.len() == self.request.bounds.max_results {
            self.truncated("max-results");
            false
        } else {
            self.result.items.push(item);
            true
        }
    }

    fn site_selected(&self, id: &ProgramEntityId, context: &ObservationContextId) -> bool {
        self.contexts.contains(context)
            && self.sites.as_ref().is_none_or(|sites| sites.contains(id))
    }

    fn allows(&self, resolution: Resolution) -> bool {
        self.request.resolution_policy == ResolutionPolicy::IncludePossible
            || resolution == Resolution::Complete
    }

    fn relationship_selected(&self, relationship: &CallRelationship) -> bool {
        self.site_selected(
            &relationship.call_site_id,
            &relationship.resolution_observation_context_id,
        ) && self
            .contexts
            .contains(&relationship.target_observation_context_id)
            && self.allows(relationship.resolution)
    }

    fn select(&self, selector: &CallableSelector) -> Result<ProgramEntityId, String> {
        if selector.entity_id.is_none() && selector.label.is_none() {
            return Err("select a callable by entity ID or exact label".into());
        }
        let mut candidates = self.snapshot.program_entities().iter().filter(|entity| {
            entity.kind == crate::ProgramEntityKind::Callable
                && selector
                    .entity_id
                    .as_ref()
                    .is_none_or(|id| *id == entity.id)
                && selector
                    .label
                    .as_ref()
                    .is_none_or(|label| *label == entity.display_name)
                && self.snapshot.manifestations().iter().any(|manifestation| {
                    manifestation.entity_id == entity.id
                        && self
                            .contexts
                            .contains(&manifestation.observation_context_id)
                })
        });
        let first = candidates
            .next()
            .ok_or("callable selection has no match in the selected observation contexts")?;
        if candidates.next().is_some() {
            return Err("ambiguous callable label in selected contexts; use bounded callable-search and select an entity ID".into());
        }
        Ok(first.id.clone())
    }

    fn search(&mut self, label: &str) {
        for entity in self.snapshot.program_entities() {
            if !self.step() {
                return;
            }
            if entity.kind != crate::ProgramEntityKind::Callable
                || !entity.display_name.contains(label)
            {
                continue;
            }
            // One manifestation per result keeps duplicate labels and contexts
            // distinguishable without an unbounded nested manifestation list.
            for manifestation in self.snapshot.manifestations() {
                if !self.step() {
                    return;
                }
                if manifestation.entity_id != entity.id
                    || !self
                        .contexts
                        .contains(&manifestation.observation_context_id)
                {
                    continue;
                }
                if !self.emit(InvestigationItem::Callable {
                    entity_id: entity.id.clone(),
                    display_name: entity.display_name.clone(),
                    manifestation: self.snapshot.searched_manifestation(manifestation),
                }) {
                    return;
                }
            }
        }
    }

    fn emit_site(&mut self, site: &crate::ProjectedCallSite, unattributed: bool) -> bool {
        let Some(targets_omitted_by_scope) = self.targets_omitted_by_scope(site, true) else {
            return false;
        };
        self.emit(InvestigationItem::CallSite {
            call_site_id: site.call_site_id.clone(),
            caller_entity_id: site.caller_entity_id.clone(),
            observation_context_id: site.resolution_observation_context_id.clone(),
            resolution: site.resolution,
            targets_omitted_by_scope,
            unattributed,
            explanation_handle: site.explanation_handle.clone(),
        })
    }

    fn targets_omitted_by_scope(
        &mut self,
        site: &crate::ProjectedCallSite,
        metered: bool,
    ) -> Option<bool> {
        for target in &site.targets {
            if metered && !self.step() {
                return None;
            }
            if !self
                .contexts
                .contains(&target.target_observation_context_id)
            {
                return Some(true);
            }
        }
        Some(false)
    }

    fn expand(&mut self, start: ProgramEntityId, incoming: bool) {
        let mut queue = VecDeque::from([(start.clone(), 0)]);
        let mut seen = BTreeSet::from([start]);
        let mut emitted_sites = BTreeSet::new();
        while let Some((entity, depth)) = queue.pop_front() {
            for site in &self.snapshot.call_graph_projection().call_sites {
                if !self.step() {
                    return;
                }
                if !self.site_selected(&site.call_site_id, &site.resolution_observation_context_id)
                    || !self.allows(site.resolution)
                {
                    continue;
                }
                let relevant = if incoming {
                    let mut relevant = false;
                    for target in &site.targets {
                        if !self.step() {
                            return;
                        }
                        if target.callee_entity_id == entity
                            && self
                                .contexts
                                .contains(&target.target_observation_context_id)
                        {
                            relevant = true;
                            break;
                        }
                    }
                    relevant
                } else {
                    site.caller_entity_id == entity
                };
                if relevant {
                    if depth == self.request.bounds.max_depth {
                        self.truncated("max-depth");
                        continue;
                    }
                    if emitted_sites.insert(site.call_site_id.clone())
                        && !self.emit_site(site, false)
                    {
                        return;
                    }
                }
            }
            for relationship in &self.snapshot.call_graph_projection().relationships {
                if !self.step() {
                    return;
                }
                if !self.relationship_selected(relationship) {
                    continue;
                }
                let (from, to) = if incoming {
                    (
                        &relationship.callee_entity_id,
                        &relationship.caller_entity_id,
                    )
                } else {
                    (
                        &relationship.caller_entity_id,
                        &relationship.callee_entity_id,
                    )
                };
                if *from != entity {
                    continue;
                }
                if depth == self.request.bounds.max_depth {
                    self.truncated("max-depth");
                    continue;
                }
                if !self.emit(InvestigationItem::Relationship {
                    relationship: relationship.clone(),
                }) {
                    return;
                }
                if seen.insert(to.clone()) {
                    queue.push_back((to.clone(), depth + 1));
                }
            }
        }
        // Unknown incoming targets are scoped uncertainty, never invented edges
        // to the selected callee. Partial sites with known targets are included too.
        if incoming && self.request.resolution_policy == ResolutionPolicy::IncludePossible {
            for site in &self.snapshot.call_graph_projection().call_sites {
                if !self.step() {
                    return;
                }
                if self.site_selected(&site.call_site_id, &site.resolution_observation_context_id)
                    && site.resolution != Resolution::Complete
                    && emitted_sites.insert(site.call_site_id.clone())
                    && !self.emit_site(site, true)
                {
                    return;
                }
            }
        }
    }

    fn path(&mut self, start: ProgramEntityId, end: ProgramEntityId) {
        let mut queue = VecDeque::from([(start.clone(), 0)]);
        let mut seen = BTreeSet::from([start.clone()]);
        let mut predecessors = BTreeMap::new();
        while let Some((entity, depth)) = queue.pop_front() {
            if entity == end {
                let mut path = Vec::new();
                let mut cursor = end;
                while cursor != start {
                    let relationship: &CallRelationship = predecessors[&cursor];
                    path.push(relationship.clone());
                    cursor = relationship.caller_entity_id.clone();
                }
                path.reverse();
                self.emit(InvestigationItem::Path {
                    relationships: path,
                });
                return;
            }
            for relationship in &self.snapshot.call_graph_projection().relationships {
                if !self.step() {
                    return;
                }
                if !self.relationship_selected(relationship)
                    || relationship.caller_entity_id != entity
                {
                    continue;
                }
                if depth == self.request.bounds.max_depth {
                    self.truncated("max-depth");
                    continue;
                }
                if seen.insert(relationship.callee_entity_id.clone()) {
                    predecessors.insert(relationship.callee_entity_id.clone(), relationship);
                    queue.push_back((relationship.callee_entity_id.clone(), depth + 1));
                }
            }
        }
    }

    fn cycles(&mut self, start: ProgramEntityId) {
        // Iterative depth-first enumeration keeps only one active simple path.
        // Dense branching consumes steps, without materializing sibling paths.
        let relationships = &self.snapshot.call_graph_projection().relationships;
        let mut frames = vec![(start.clone(), 0)];
        let mut path = Vec::<&CallRelationship>::new();
        let mut visited = BTreeSet::from([start.clone()]);
        while let Some((entity, next_index)) = frames.last_mut() {
            if *next_index == relationships.len() {
                let (entity, _) = frames.pop().expect("active frame");
                visited.remove(&entity);
                path.pop();
                continue;
            }
            let relationship = &relationships[*next_index];
            *next_index += 1;
            if !self.step() {
                return;
            }
            if !self.relationship_selected(relationship) || relationship.caller_entity_id != *entity
            {
                continue;
            }
            if path.len() == self.request.bounds.max_depth {
                self.truncated("max-depth");
                continue;
            }
            if relationship.callee_entity_id == start {
                let cycle: Vec<_> = path
                    .iter()
                    .copied()
                    .chain([relationship])
                    .cloned()
                    .collect();
                let mut closed = true;
                for relationship in &cycle {
                    match self.closed_site(&relationship.call_site_id, true) {
                        Some(true) => {}
                        Some(false) => {
                            closed = false;
                            break;
                        }
                        None => return,
                    }
                }
                let scope = closed.then(|| cycle.iter().map(|r| r.call_site_id.clone()).collect());
                if !self.emit(InvestigationItem::Cycle {
                    classification: if closed {
                        CycleClassification::DefiniteRecursiveCycle
                    } else {
                        CycleClassification::PotentialRecursiveCycle
                    },
                    relationships: cycle,
                    closed_call_site_scope: scope,
                }) {
                    return;
                }
            } else if visited.insert(relationship.callee_entity_id.clone()) {
                path.push(relationship);
                frames.push((relationship.callee_entity_id.clone(), 0));
            }
        }
    }

    fn closed_site(&mut self, id: &ProgramEntityId, metered: bool) -> Option<bool> {
        for site in &self.snapshot.call_graph_projection().call_sites {
            if metered && !self.step() {
                return None;
            }
            if site.call_site_id != *id {
                continue;
            }
            if !self
                .contexts
                .contains(&site.resolution_observation_context_id)
                || site.resolution != Resolution::Complete
            {
                return Some(false);
            }
            return self
                .targets_omitted_by_scope(site, metered)
                .map(|omitted| !omitted);
        }
        Some(false)
    }
}

pub(crate) fn execute(
    snapshot: &PublishedSnapshot,
    request: &BoundedQuery,
) -> Result<BoundedQueryResult, String> {
    let bounds = &request.bounds;
    if !(1..=100).contains(&bounds.max_depth)
        || !(1..=10_000).contains(&bounds.max_results)
        || !(1..=1_000_000).contains(&bounds.max_steps)
    {
        return Err(
            "bounds require max_depth 1..=100, max_results 1..=10000, and max_steps 1..=1000000"
                .into(),
        );
    }
    let contexts: BTreeSet<_> = request
        .scope
        .observation_context_ids
        .iter()
        .cloned()
        .collect();
    if contexts.is_empty()
        || contexts.len() > 100
        || contexts.len() != request.scope.observation_context_ids.len()
    {
        return Err("select 1..=100 nonrepeated observation context IDs".into());
    }
    let mut qualified = Vec::new();
    for id in &contexts {
        let context = snapshot
            .observation_contexts()
            .iter()
            .find(|context| context.id == *id)
            .ok_or_else(|| format!("unknown observation context '{id}'"))?;
        if context.build_target != request.scope.build_target {
            return Err(format!(
                "observation context '{id}' does not belong to target '{}'",
                request.scope.build_target
            ));
        }
        qualified.push(context.clone());
    }
    let (name, direction) = match request.query {
        Investigation::CallableSearch { .. } => ("callable-search", "none"),
        Investigation::Callers { .. } => ("callers", "incoming"),
        Investigation::Callees { .. } => ("callees", "outgoing"),
        Investigation::CallPath { .. } => ("call-path", "outgoing"),
        Investigation::RecursiveCycles { .. } => ("recursive-cycles", "outgoing"),
    };
    let mut evaluation = Evaluation {
        snapshot,
        request,
        contexts,
        sites: None,
        result: BoundedQueryResult {
            program_snapshot_id: snapshot.program_snapshot().id.clone(),
            query_name: name.into(),
            projection: "call-graph".into(),
            relationship_kind: "declared-call-target-claim".into(),
            direction: direction.into(),
            observation_contexts: qualified,
            resolution_policy: request.resolution_policy,
            world: request.world.clone(),
            closed_scope_explanation_handles: Vec::new(),
            bounds: bounds.clone(),
            items: Vec::new(),
            truncation: Vec::new(),
            steps: 0,
            returned_static_call_site_cardinality: 0,
            runtime_invocation_measure: None,
        },
    };
    if let WorldPolicy::ClosedCallSites { call_site_ids } = &request.world {
        let sites: BTreeSet<_> = call_site_ids.iter().cloned().collect();
        if sites.is_empty()
            || sites.len() > request.bounds.max_results
            || sites.len() != call_site_ids.len()
            || sites
                .iter()
                .any(|id| evaluation.closed_site(id, false) != Some(true))
        {
            return Err("closed-call-sites requires 1..=max_results nonrepeated recorded sites with complete resolution and all target contexts selected; completeness bases are available through their explanations".into());
        }
        if matches!(request.query, Investigation::CallableSearch { .. }) {
            return Err("call-site closure cannot justify closed-world callable search".into());
        }
        evaluation.result.closed_scope_explanation_handles = snapshot
            .call_graph_projection()
            .call_sites
            .iter()
            .filter(|site| sites.contains(&site.call_site_id))
            .map(|site| site.explanation_handle.clone())
            .collect();
        evaluation.sites = Some(sites);
    }
    match &request.query {
        Investigation::CallableSearch { label } => evaluation.search(label),
        Investigation::Callers { callee } => {
            let start = evaluation.select(callee)?;
            evaluation.expand(start, true);
        }
        Investigation::Callees { caller } => {
            let start = evaluation.select(caller)?;
            evaluation.expand(start, false);
        }
        Investigation::CallPath { start, end } => {
            let start = evaluation.select(start)?;
            let end = evaluation.select(end)?;
            evaluation.path(start, end);
        }
        Investigation::RecursiveCycles { start } => {
            let start = evaluation.select(start)?;
            evaluation.cycles(start);
        }
    }
    let mut returned_sites = BTreeSet::new();
    for item in &evaluation.result.items {
        match item {
            InvestigationItem::CallSite { call_site_id, .. } => {
                returned_sites.insert(call_site_id);
            }
            InvestigationItem::Relationship { relationship } => {
                returned_sites.insert(&relationship.call_site_id);
            }
            InvestigationItem::Path { relationships }
            | InvestigationItem::Cycle { relationships, .. } => {
                returned_sites.extend(relationships.iter().map(|r| &r.call_site_id));
            }
            InvestigationItem::Callable { .. } => {}
        }
    }
    evaluation.result.returned_static_call_site_cardinality = returned_sites
        .iter()
        .filter(|id| {
            snapshot.evidence_records().iter().any(|record| {
                record.subject_entity_id == ***id
                    && record.support == EvidenceSupport::CallSiteResolution
                    && record.scope == EvidenceScope::Static
            })
        })
        .count();
    Ok(evaluation.result)
}
