use std::path::Path;

pub const EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION: &str = "1";

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
    pub display_name: String,
    pub defined: bool,
    pub representation: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContributedDirectCall {
    pub caller_display_name: String,
    pub callee_display_name: String,
    pub target_representation: String,
    pub line: usize,
    pub evidence_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceContribution {
    pub input: ContributedInput,
    pub callables: Vec<ContributedCallable>,
    pub direct_calls: Vec<ContributedDirectCall>,
}

/// A versioned seam between evidence-source adapters and Gloom's core model.
pub trait EvidenceContributor {
    fn identity(&self) -> ContributorIdentity;

    fn contribute(&self, input: &Path) -> Result<EvidenceContribution, String>;
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
        for required in [
            EvidenceCapability::CallableManifestations,
            EvidenceCapability::DirectCallEvidence,
        ] {
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
    pub(crate) fn validate(&self) -> Result<(), String> {
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
        for callable in &self.callables {
            if callable.display_name.is_empty() || callable.representation.is_empty() {
                return Err(
                    "contributed callable names and representations cannot be empty".into(),
                );
            }
        }
        for direct_call in &self.direct_calls {
            if direct_call.caller_display_name.is_empty()
                || direct_call.callee_display_name.is_empty()
                || direct_call.target_representation.is_empty()
                || direct_call.evidence_type.is_empty()
                || direct_call.line == 0
            {
                return Err("contributed direct-call evidence is not fully qualified".into());
            }
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
}
