//! Local ingestion of a build producer's explicit compilation and membership declarations.
use crate::contributor::EvidenceContributor;
use crate::{AcquiredInputId, LlvmTextContributor, ObservationContext, PublishedSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredBuild {
    pub schema_version: String,
    pub program_snapshot_id: String,
    pub build_configuration: String,
    pub toolchain: String,
    pub analysis_stage: String,
    pub compilations: Vec<DeclaredCompilation>,
    pub targets: Vec<DeclaredTarget>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredCompilation {
    pub id: String,
    pub working_directory: String,
    pub source_input: String,
    /// The original argv, including the compiler executable. Never executed.
    pub compiler_arguments: Vec<String>,
    pub evidence_artifact: String,
    /// Explicitly empty when this compilation has no generated/configured inputs.
    pub generated_inputs: Vec<DeclaredGeneratedInput>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredGeneratedInput {
    pub path: String,
    pub kind: GeneratedInputKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GeneratedInputKind {
    Generated,
    Configured,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredTarget {
    pub name: String,
    pub compilation_ids: Vec<String>,
}

/// Retained declaration for one selected target, with each compilation bound to
/// the acquired input cited by searchable declarations and explanation evidence.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredBuildAcquisition {
    pub manifest_path: String,
    pub declaration: DeclaredBuild,
    pub acquired_input_ids: Vec<AcquiredInputId>,
}

fn required(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!(
            "declared-build acquisition gap: {field} cannot be empty"
        ))
    } else {
        Ok(())
    }
}

impl DeclaredBuild {
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != "1" {
            return Err(format!(
                "unsupported declared-build schema '{}'",
                self.schema_version
            ));
        }
        for (field, value) in [
            ("program_snapshot_id", &self.program_snapshot_id),
            ("build_configuration", &self.build_configuration),
            ("toolchain", &self.toolchain),
            ("analysis_stage", &self.analysis_stage),
        ] {
            required(value, field)?;
        }
        let mut ids = BTreeSet::new();
        for compilation in &self.compilations {
            for (field, value) in [
                ("compilation id", &compilation.id),
                ("working_directory", &compilation.working_directory),
                ("source_input", &compilation.source_input),
                ("evidence_artifact", &compilation.evidence_artifact),
            ] {
                required(value, field)?;
            }
            if !ids.insert(&compilation.id) {
                return Err(format!("duplicate compilation id '{}'", compilation.id));
            }
            required(
                compilation
                    .compiler_arguments
                    .first()
                    .map_or("", String::as_str),
                "compiler_arguments (including executable)",
            )?;
            if compilation.compiler_arguments.len() < 2 {
                return Err("declared-build acquisition gap: compiler_arguments must include compilation arguments".into());
            }
            if Path::new(&compilation.evidence_artifact)
                .extension()
                .and_then(|ext| ext.to_str())
                != Some("ll")
            {
                return Err(
                    "declared-build acquisition requires existing .ll evidence artifacts".into(),
                );
            }
            for input in &compilation.generated_inputs {
                required(&input.path, "generated input path")?;
            }
        }
        if self.targets.is_empty() {
            return Err("declared-build acquisition gap: target membership is required".into());
        }
        let mut targets = BTreeSet::new();
        for target in &self.targets {
            required(&target.name, "target name")?;
            if !targets.insert(&target.name) {
                return Err(format!("duplicate build target '{}'", target.name));
            }
            if target.compilation_ids.is_empty() {
                return Err(format!(
                    "declared-build acquisition gap: target '{}' has no compilation membership",
                    target.name
                ));
            }
            let mut members = BTreeSet::new();
            for id in &target.compilation_ids {
                if !ids.contains(id) {
                    return Err(format!(
                        "declared-build acquisition gap: target '{}' references unknown compilation '{id}'",
                        target.name
                    ));
                }
                if !members.insert(id) {
                    return Err(format!(
                        "target '{}' repeats compilation '{id}'",
                        target.name
                    ));
                }
            }
        }
        Ok(())
    }
}

impl DeclaredBuildAcquisition {
    pub(crate) fn validate(&self, snapshot: &PublishedSnapshot) -> Result<(), String> {
        self.declaration.validate()?;
        required(&self.manifest_path, "manifest_path")?;
        let declaration = &self.declaration;
        if declaration.targets.len() != 1 || snapshot.observation_contexts().len() != 1 {
            return Err(
                "declared-build acquisition must qualify exactly one selected target and context"
                    .into(),
            );
        }
        let context = &snapshot.observation_contexts()[0];
        let contributor = LlvmTextContributor::new("clang", &[]).identity();
        if context
            != &ObservationContext::static_analysis(
                &declaration.program_snapshot_id,
                &declaration.targets[0].name,
                &declaration.build_configuration,
                &declaration.toolchain,
                contributor.name,
                &context.extraction_version,
                &declaration.analysis_stage,
            )
        {
            return Err("declared-build acquisition disagrees with observation context".into());
        }
        let members = &declaration.targets[0].compilation_ids;
        if declaration.compilations.len() != snapshot.acquired_inputs().len()
            || self.acquired_input_ids.len() != declaration.compilations.len()
            || members.len() != declaration.compilations.len()
        {
            return Err("declared-build membership does not cover acquired inputs".into());
        }
        for (((compilation, input), id), member) in declaration
            .compilations
            .iter()
            .zip(snapshot.acquired_inputs())
            .zip(&self.acquired_input_ids)
            .zip(members)
        {
            if &compilation.id != member
                || &input.id != id
                || compilation.evidence_artifact != input.path
                || input.acquisition_method != "declared-artifact"
                || input.media_type != "application/llvm-ir"
            {
                return Err("declared-build compilation does not match its acquired input".into());
            }
        }
        Ok(())
    }
}

pub(crate) fn load(path: &Path) -> Result<DeclaredBuild, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let declaration: DeclaredBuild = serde_json::from_str(&text).map_err(|error| {
        format!(
            "declared-build acquisition gap in {}: {error}",
            path.display()
        )
    })?;
    declaration.validate()?;
    Ok(declaration)
}

fn resolved(base: &Path, path: &str) -> String {
    base.join(path).to_string_lossy().into_owned()
}

pub(crate) struct SelectedBuild {
    pub manifest_path: String,
    pub declaration: DeclaredBuild,
    pub context: ObservationContext,
}

pub(crate) fn select(path: &Path, target: &str) -> Result<SelectedBuild, String> {
    let mut declaration = load(path)?;
    let selected = declaration
        .targets
        .iter()
        .find(|item| item.name == target)
        .cloned()
        .ok_or_else(|| format!("unknown declared build target '{target}'"))?;
    let manifest_path = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let base = manifest_path
        .parent()
        .ok_or("manifest has no parent directory")?;
    let mut compilations = Vec::new();
    for id in &selected.compilation_ids {
        let mut compilation = declaration
            .compilations
            .iter()
            .find(|item| &item.id == id)
            .expect("validated target membership")
            .clone();
        compilation.working_directory = resolved(base, &compilation.working_directory);
        let directory = PathBuf::from(&compilation.working_directory);
        compilation.source_input = resolved(&directory, &compilation.source_input);
        compilation.evidence_artifact = resolved(&directory, &compilation.evidence_artifact);
        for input in &mut compilation.generated_inputs {
            input.path = resolved(&directory, &input.path);
        }
        compilations.push(compilation);
    }
    declaration.compilations = compilations;
    declaration.targets = vec![selected];
    let contributor = LlvmTextContributor::new("clang", &[]);
    let identity = contributor.identity();
    let context = ObservationContext::static_analysis(
        &declaration.program_snapshot_id,
        target,
        &declaration.build_configuration,
        &declaration.toolchain,
        &identity.name,
        &identity.version,
        &declaration.analysis_stage,
    );
    Ok(SelectedBuild {
        manifest_path: manifest_path.to_string_lossy().into_owned(),
        declaration,
        context,
    })
}

impl SelectedBuild {
    pub(crate) fn publish(
        self,
        contributions: Vec<crate::EvidenceContribution>,
    ) -> Result<PublishedSnapshot, String> {
        let snapshot = crate::snapshot::publish(
            contributions,
            LlvmTextContributor::new("clang", &[]).identity(),
            self.context,
        )?;
        let acquisition = DeclaredBuildAcquisition {
            manifest_path: self.manifest_path,
            declaration: self.declaration,
            acquired_input_ids: snapshot
                .acquired_inputs()
                .iter()
                .map(|input| input.id.clone())
                .collect(),
        };
        snapshot.with_declared_build(acquisition)
    }
}

pub(crate) fn publish(path: &Path, target: &str) -> Result<PublishedSnapshot, String> {
    let selected = select(path, target)?;
    let contributor = LlvmTextContributor::new("clang", &[]);
    let contributions = selected
        .declaration
        .compilations
        .iter()
        .map(|compilation| {
            contributor
                .contribute(Path::new(&compilation.evidence_artifact), &selected.context)
                .map_err(|error| {
                    format!(
                        "declared-build acquisition gap for compilation '{}': {error}",
                        compilation.id
                    )
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    selected.publish(contributions)
}
