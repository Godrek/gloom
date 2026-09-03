use crate::snapshot::{
    EvidenceScope, EvidenceSupport, ObservationContext, ObservationContextId, Resolution,
    complete_resolution_evidence_error,
};
use std::path::Path;

/// Version of the seam between evidence contributors and the core model.
///
/// History:
/// - `1`: callable manifestations plus direct calls keyed by display name, one
///   observation context per publication.
/// - `2`: contributors declare their observation contexts, assert a contributor
///   callable identity per manifestation, and contribute first-class call sites
///   with per-site target-set resolution, typed evidence (scope and support),
///   and zero or more target claims. Indirect-call evidence became a declared
///   capability.
pub const EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION: &str = "2";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContributorIdentity {
    pub name: String,
    pub version: String,
    pub contract_version: String,
    pub capabilities: Vec<EvidenceCapability>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceCapability {
    CallableManifestations,
    DirectCallEvidence,
    IndirectCallEvidence,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContributedInput {
    pub path: String,
    pub evidence_artifact: String,
    pub media_type: String,
    pub acquisition_method: String,
    pub content_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContributedCallable {
    pub contributor_callable_id: String,
    pub display_name: String,
    pub defined: bool,
    pub representation: String,
    pub observation_context_id: ObservationContextId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContributedCallKind {
    Direct,
    Indirect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContributedEvidence {
    pub evidence_type: String,
    pub scope: EvidenceScope,
    pub support: EvidenceSupport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContributedCallSite {
    pub kind: ContributedCallKind,
    pub caller_callable_id: String,
    pub line: usize,
    pub observation_context_id: ObservationContextId,
    pub resolution: Resolution,
    pub evidence: ContributedEvidence,
    pub target_claims: Vec<ContributedTargetClaim>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContributedTargetClaim {
    pub target_callable_id: String,
    pub callee_display_name: String,
    pub target_representation: String,
    pub observation_context_id: ObservationContextId,
    pub evidence: Vec<ContributedEvidence>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceContribution {
    pub input: ContributedInput,
    pub observation_contexts: Vec<ObservationContext>,
    pub callables: Vec<ContributedCallable>,
    pub call_sites: Vec<ContributedCallSite>,
}

/// A versioned seam between evidence-source adapters and Gloom's core model.
pub trait EvidenceContributor {
    fn identity(&self) -> ContributorIdentity;

    fn contribute(
        &self,
        input: &Path,
        context: &ObservationContext,
    ) -> Result<EvidenceContribution, String>;
}

impl ContributorIdentity {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.contract_version != EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION {
            return Err(format!(
                "unsupported evidence-contributor contract {:?}",
                self.contract_version
            ));
        }
        if self.name.trim().is_empty() || self.version.trim().is_empty() {
            return Err("evidence contributor identity and version cannot be empty".into());
        }
        for required in [EvidenceCapability::CallableManifestations] {
            if !self.capabilities.contains(&required) {
                return Err(format!(
                    "evidence contributor '{}' does not declare capability {required:?}",
                    self.name
                ));
            }
        }
        Ok(())
    }
}

impl EvidenceContribution {
    pub(crate) fn validate(
        &self,
        contributor: &ContributorIdentity,
        publication_context: &ObservationContext,
    ) -> Result<(), String> {
        for (field, value) in [
            ("path", self.input.path.as_str()),
            ("evidence artifact", self.input.evidence_artifact.as_str()),
            ("media type", self.input.media_type.as_str()),
            ("acquisition method", self.input.acquisition_method.as_str()),
            (
                "content fingerprint",
                self.input.content_fingerprint.as_str(),
            ),
        ] {
            if value.trim().is_empty() {
                return Err(format!("contributed input {field} cannot be empty"));
            }
        }
        let context_ids = self
            .observation_contexts
            .iter()
            .map(|context| context.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let contexts_by_id = self
            .observation_contexts
            .iter()
            .map(|context| (context.id.as_str(), context))
            .collect::<std::collections::BTreeMap<_, _>>();
        if context_ids.len() != self.observation_contexts.len() {
            return Err("contribution contains duplicate observation-context identities".into());
        }
        if !context_ids.contains(publication_context.id.as_str()) {
            return Err("contribution omits its publication observation context".into());
        }
        for context in &self.observation_contexts {
            context.validate()?;
            if context.program_snapshot_id != publication_context.program_snapshot_id {
                return Err(format!(
                    "contributed observation context '{}' belongs to another program snapshot",
                    context.id
                ));
            }
            if context.extraction_method != contributor.name
                || context.extraction_version != contributor.version
            {
                return Err(format!(
                    "evidence contributor '{}@{}' does not match contributed observation context '{}@{}'",
                    contributor.name,
                    contributor.version,
                    context.extraction_method,
                    context.extraction_version
                ));
            }
        }
        let mut callable_identities = std::collections::BTreeMap::new();
        for callable in &self.callables {
            if callable.contributor_callable_id.trim().is_empty()
                || callable.display_name.is_empty()
                || callable.representation.is_empty()
            {
                return Err(
                    "contributed callable identities, names, and representations cannot be empty"
                        .into(),
                );
            }
            if !context_ids.contains(callable.observation_context_id.as_str()) {
                return Err(format!(
                    "contributed callable references unknown observation context '{}'",
                    callable.observation_context_id
                ));
            }
            let identity = (
                callable.observation_context_id.as_str(),
                callable.contributor_callable_id.as_str(),
            );
            if let Some(existing) = callable_identities.insert(identity, callable) {
                if existing.display_name != callable.display_name
                    || existing.representation != callable.representation
                {
                    return Err(format!(
                        "contributed callable identity '{}' has conflicting labels or representations in observation context '{}'",
                        callable.contributor_callable_id, callable.observation_context_id
                    ));
                }
            }
        }
        for call_site in &self.call_sites {
            if call_site.caller_callable_id.trim().is_empty() || call_site.line == 0 {
                return Err("contributed call-site evidence is not fully qualified".into());
            }
            let required_capability = match call_site.kind {
                ContributedCallKind::Direct => EvidenceCapability::DirectCallEvidence,
                ContributedCallKind::Indirect => EvidenceCapability::IndirectCallEvidence,
            };
            if !contributor.capabilities.contains(&required_capability) {
                return Err(format!(
                    "evidence contributor '{}' does not declare capability {required_capability:?}",
                    contributor.name
                ));
            }
            if !context_ids.contains(call_site.observation_context_id.as_str()) {
                return Err(format!(
                    "contributed call site references unknown observation context '{}'",
                    call_site.observation_context_id
                ));
            }
            call_site.evidence.validate(
                contexts_by_id
                    .get(call_site.observation_context_id.as_str())
                    .expect("contributed call-site context must exist"),
                EvidenceSupport::CallSiteResolution,
            )?;
            if !callable_identities.contains_key(&(
                call_site.observation_context_id.as_str(),
                call_site.caller_callable_id.as_str(),
            )) {
                return Err(format!(
                    "contributed call site references unknown caller identity '{}' in observation context '{}'",
                    call_site.caller_callable_id, call_site.observation_context_id
                ));
            }
            let contextual_target_count = call_site
                .target_claims
                .iter()
                .filter(|target| target.observation_context_id == call_site.observation_context_id)
                .count();
            if !call_site
                .resolution
                .accepts_target_count(contextual_target_count)
            {
                return Err(format!(
                    "contributed call-site resolution {:?} is incompatible with {contextual_target_count} target claims in its observation context",
                    call_site.resolution,
                ));
            }
            if call_site.resolution.requires_static_resolution_evidence()
                && call_site.evidence.scope != EvidenceScope::Static
            {
                return Err(complete_resolution_evidence_error("contributed call site"));
            }
            if call_site.kind == ContributedCallKind::Direct
                && (call_site.resolution != Resolution::Complete || contextual_target_count != 1)
            {
                return Err(
                    "contributed direct call must have complete resolution and one target claim"
                        .into(),
                );
            }
            for target in &call_site.target_claims {
                if target.target_callable_id.trim().is_empty()
                    || target.callee_display_name.is_empty()
                    || target.target_representation.is_empty()
                    || target.evidence.is_empty()
                {
                    return Err("contributed target claim is not fully qualified".into());
                }
                if !context_ids.contains(target.observation_context_id.as_str()) {
                    return Err(format!(
                        "contributed target claim references unknown observation context '{}'",
                        target.observation_context_id
                    ));
                }
                let target_context = contexts_by_id
                    .get(target.observation_context_id.as_str())
                    .expect("contributed target context must exist");
                for evidence in &target.evidence {
                    evidence.validate(target_context, EvidenceSupport::TargetClaim)?;
                }
            }
        }
        Ok(())
    }
}

impl ContributedEvidence {
    fn validate(
        &self,
        context: &ObservationContext,
        expected_support: EvidenceSupport,
    ) -> Result<(), String> {
        if self.evidence_type.trim().is_empty() {
            return Err("contributed evidence type cannot be empty".into());
        }
        if self.support != expected_support {
            return Err(format!(
                "contributed evidence '{}' declares {:?} support where {:?} support is required",
                self.evidence_type, self.support, expected_support
            ));
        }
        if !self.scope.matches(context) {
            return Err(format!(
                "contributed {:?} evidence '{}' is incompatible with observation context '{}'",
                self.scope, self.evidence_type, context.id
            ));
        }
        Ok(())
    }
}

pub(crate) fn fingerprint_parts(parts: &[&str]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for part in parts {
        let length = u64::try_from(part.len()).expect("string length must fit in u64");
        for byte in length.to_le_bytes().iter().chain(part.as_bytes()) {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    format!("fnv1a64:{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_uses_fixed_width_length_prefixes() {
        assert_eq!(fingerprint_parts(&["a", "bc"]), "fnv1a64:ba1e1f0e0704d8ea");
    }

    fn direct_call_contribution(
        contributor: &ContributorIdentity,
    ) -> (EvidenceContribution, ObservationContext) {
        let context = ObservationContext::static_analysis(
            "snapshot:fixture",
            "fixture",
            "debug",
            "fixture toolchain",
            contributor.name.clone(),
            contributor.version.clone(),
            "fixture extraction",
        );
        let contribution = EvidenceContribution {
            input: ContributedInput {
                path: "fixture".into(),
                evidence_artifact: "fixture".into(),
                media_type: "application/x-fixture".into(),
                acquisition_method: "semantic-fixture".into(),
                content_fingerprint: "fixture".into(),
            },
            observation_contexts: vec![context.clone()],
            callables: vec![ContributedCallable {
                contributor_callable_id: "caller".into(),
                display_name: "caller".into(),
                defined: true,
                representation: "fixture-callable".into(),
                observation_context_id: context.id.clone(),
            }],
            call_sites: vec![ContributedCallSite {
                kind: ContributedCallKind::Direct,
                caller_callable_id: "caller".into(),
                line: 1,
                observation_context_id: context.id.clone(),
                resolution: Resolution::Complete,
                evidence: ContributedEvidence {
                    evidence_type: "static-call-site".into(),
                    scope: EvidenceScope::Static,
                    support: EvidenceSupport::CallSiteResolution,
                },
                target_claims: vec![ContributedTargetClaim {
                    target_callable_id: "callee".into(),
                    callee_display_name: "callee".into(),
                    target_representation: "fixture-callable".into(),
                    observation_context_id: context.id.clone(),
                    evidence: vec![ContributedEvidence {
                        evidence_type: "static-direct-call".into(),
                        scope: EvidenceScope::Static,
                        support: EvidenceSupport::TargetClaim,
                    }],
                }],
            }],
        };
        (contribution, context)
    }

    fn identity(name: &str, capabilities: Vec<EvidenceCapability>) -> ContributorIdentity {
        ContributorIdentity {
            name: name.into(),
            version: "1".into(),
            contract_version: EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION.into(),
            capabilities,
        }
    }

    #[test]
    fn contributor_capabilities_are_declarations_not_a_mandatory_checklist() {
        let direct_only = identity(
            "fixture.direct-only",
            vec![
                EvidenceCapability::CallableManifestations,
                EvidenceCapability::DirectCallEvidence,
            ],
        );
        direct_only.validate().unwrap();
        let (contribution, context) = direct_call_contribution(&direct_only);
        contribution.validate(&direct_only, &context).unwrap();

        let callable_only = identity(
            "fixture.callable-only",
            vec![EvidenceCapability::CallableManifestations],
        );
        callable_only.validate().unwrap();
        let (contribution, context) = direct_call_contribution(&callable_only);
        let error = contribution.validate(&callable_only, &context).unwrap_err();
        assert!(
            error.contains("does not declare capability DirectCallEvidence"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn contributed_contexts_must_name_the_validating_contributor() {
        let direct_only = identity(
            "fixture.direct-only",
            vec![
                EvidenceCapability::CallableManifestations,
                EvidenceCapability::DirectCallEvidence,
            ],
        );
        let (contribution, context) = direct_call_contribution(&direct_only);
        let other = identity("fixture.other", direct_only.capabilities.clone());
        let error = contribution.validate(&other, &context).unwrap_err();
        assert!(
            error.contains("does not match contributed observation context"),
            "unexpected error: {error}"
        );
    }
}
