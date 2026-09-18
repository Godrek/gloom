//! Capture of a real Linux/Clang build, for builds whose declared artifacts do
//! not establish link membership.
//!
//! Where [`crate::acquisition`] trusts a build producer's manifest, capture
//! observes the build itself: every compiler invocation the build makes through
//! a generated wrapper is recorded with its verbatim argument vector and
//! working directory, and the link that produces one executable establishes
//! that target's membership. Gloom then replays each contributing compilation
//! with exactly those recorded arguments to obtain its LLVM IR, and publishes a
//! snapshot scoped to that target.
//!
//! Nothing here reconstructs configuration Gloom did not observe. A build whose
//! compilations were not captured, whose target links an object no captured
//! compilation produced, or whose link reaches membership Gloom cannot observe
//! (an archive, a shared library, a `-l` search) fails with a diagnostic naming
//! the gap instead of being completed by inference.
use crate::snapshot::{AcquiredInputId, ObservationContext, PublishedSnapshot};
use crate::{EvidenceContribution, LlvmTextContributor};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Version of the captured-build record an export carries.
pub const CAPTURE_SCHEMA_VERSION: &str = "1";

/// How an acquired input obtained by replaying a captured compilation is
/// distinguished from a producer's declared artifact.
pub const CAPTURED_ACQUISITION_METHOD: &str = "captured-build";

/// The stage capture observes. Capture reads per-translation-unit IR replayed
/// from the recorded compilations, never the linked image, so the stage is
/// derived rather than asked for.
pub const CAPTURED_ANALYSIS_STAGE: &str = "per-translation-unit LLVM IR before linking";

/// The initial supported capture range: Linux, a Clang the wrapper can re-run,
/// and a major version this extractor has been exercised against.
pub const SUPPORTED_CLANG_MAJOR_VERSIONS: std::ops::RangeInclusive<u32> = 14..=20;

/// The name a captured build must invoke its C compiler as. Capture wraps this
/// name on `PATH`; a build that invokes another driver, or one by absolute
/// path, is not observed and its compilations are reported missing.
pub const WRAPPED_COMPILER_NAME: &str = "clang";

const UNSUPPORTED_PLATFORM: &str =
    "build capture is supported only on Linux with Clang (see docs/build-capture.md)";

/// One compiler invocation the wrapper observed, exactly as the build made it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedInvocation {
    pub working_directory: String,
    /// The argument vector as executed, including the resolved compiler.
    pub arguments: Vec<String>,
    /// Absent when the compiler was terminated by a signal.
    pub exit_code: Option<i32>,
}

/// The compiler capture observed, identified by what it reports about itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedToolchain {
    pub compiler: String,
    pub version: String,
    pub target_triple: String,
}

impl CapturedToolchain {
    /// The toolchain identity an observation context is qualified by.
    pub fn identity(&self) -> String {
        format!("{} ({})", self.version, self.target_triple)
    }
}

/// What a build-produced file was to one captured compilation. Both roles are
/// observed: a file is either the translation unit the compilation names or a
/// file the compilation read while compiling it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapturedInputRole {
    TranslationUnitSource,
    IncludedFile,
}

/// An input a captured compilation read that the build itself produced.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedGeneratedInput {
    pub path: String,
    pub role: CapturedInputRole,
}

/// One compilation the build performed, and the IR replaying it produced.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedCompilation {
    /// The object output the link names this compilation by.
    pub id: String,
    pub working_directory: String,
    pub source_input: String,
    /// The recorded argument vector, including the compiler. Replayed verbatim
    /// apart from the extraction adjustments below.
    pub compiler_arguments: Vec<String>,
    pub object_output: String,
    /// The argument vector Gloom ran to obtain IR from this compilation.
    pub extraction_arguments: Vec<String>,
    pub evidence_artifact: String,
    /// Inputs this compilation read that did not exist before the build ran.
    /// Explicitly empty when the compilation read no build-produced file.
    pub generated_inputs: Vec<CapturedGeneratedInput>,
}

/// The link that established one executable target's membership.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedTarget {
    pub name: String,
    pub image_path: String,
    pub working_directory: String,
    pub link_arguments: Vec<String>,
    /// Membership in link order; every entry names a captured compilation.
    pub compilation_ids: Vec<String>,
}

/// Everything capture observed about one selected target.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedBuild {
    pub schema_version: String,
    pub program_snapshot_id: String,
    pub build_configuration: String,
    pub analysis_stage: String,
    pub toolchain: CapturedToolchain,
    pub build_command: Vec<String>,
    pub project_root: String,
    pub capture_directory: String,
    pub compilations: Vec<CapturedCompilation>,
    pub target: CapturedTarget,
}

/// The captured build retained by a published snapshot, with each contributing
/// compilation bound to the acquired input its evidence was read from.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedBuildAcquisition {
    pub capture: CapturedBuild,
    pub acquired_input_ids: Vec<AcquiredInputId>,
}

/// What a caller must supply to capture a build. Everything else — the build
/// configuration, the toolchain identity, the analysis stage, and target
/// membership — is observed rather than declared.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BuildCaptureRequest {
    /// The directory the build command runs in, and the tree whose
    /// build-produced files are observed.
    pub project_root: PathBuf,
    /// The build command, as argv. Executed once, under the capture wrapper.
    pub build_command: Vec<String>,
    /// The executable target to publish, named by the link output's file name.
    pub target: String,
    /// The program content identity the observed build is a build of. Gloom
    /// cannot observe program identity, so the caller asserts it.
    pub program_snapshot_id: String,
    /// The Clang the wrapper re-runs, by name on `PATH` or by path.
    pub compiler: String,
    /// Where the wrapper, the invocation log, and retained IR are written.
    pub capture_directory: PathBuf,
    /// The `gloom` executable the generated wrapper re-enters to record each
    /// invocation. A caller that is not the CLI (a test, an embedding) has a
    /// different current executable, so this is explicit rather than guessed.
    pub recorder: PathBuf,
}

fn required(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("build-capture gap: {field} cannot be empty"))
    } else {
        Ok(())
    }
}

impl CapturedBuild {
    fn validate(&self) -> Result<(), String> {
        if self.schema_version != CAPTURE_SCHEMA_VERSION {
            return Err(format!(
                "unsupported captured-build schema '{}'",
                self.schema_version
            ));
        }
        for (field, value) in [
            ("program_snapshot_id", self.program_snapshot_id.as_str()),
            ("build_configuration", self.build_configuration.as_str()),
            ("project_root", self.project_root.as_str()),
            ("capture_directory", self.capture_directory.as_str()),
            ("toolchain compiler", self.toolchain.compiler.as_str()),
            ("toolchain version", self.toolchain.version.as_str()),
            (
                "toolchain target triple",
                self.toolchain.target_triple.as_str(),
            ),
        ] {
            required(value, field)?;
        }
        if self.analysis_stage != CAPTURED_ANALYSIS_STAGE {
            return Err(format!(
                "captured build reports analysis stage '{}' rather than the stage capture observes",
                self.analysis_stage
            ));
        }
        if self.build_command.is_empty() {
            return Err("build-capture gap: build_command cannot be empty".into());
        }
        let mut ids = BTreeSet::new();
        for compilation in &self.compilations {
            for (field, value) in [
                ("compilation id", compilation.id.as_str()),
                ("working_directory", compilation.working_directory.as_str()),
                ("source_input", compilation.source_input.as_str()),
                ("object_output", compilation.object_output.as_str()),
                ("evidence_artifact", compilation.evidence_artifact.as_str()),
            ] {
                required(value, field)?;
            }
            if !ids.insert(compilation.id.as_str()) {
                return Err(format!(
                    "captured build records compilation '{}' more than once",
                    compilation.id
                ));
            }
            if compilation.compiler_arguments.len() < 2 {
                return Err(
                    "build-capture gap: compiler_arguments must include the compiler and its arguments"
                        .into(),
                );
            }
            if compilation.extraction_arguments.len() < 2 {
                return Err("build-capture gap: extraction_arguments cannot be empty".into());
            }
            if Path::new(&compilation.evidence_artifact)
                .extension()
                .and_then(|extension| extension.to_str())
                != Some("ll")
            {
                return Err("captured compilations are retained as .ll evidence artifacts".into());
            }
            for input in &compilation.generated_inputs {
                required(&input.path, "generated input path")?;
            }
        }
        required(&self.target.name, "target name")?;
        for (field, value) in [
            ("target image_path", self.target.image_path.as_str()),
            (
                "target working_directory",
                self.target.working_directory.as_str(),
            ),
        ] {
            required(value, field)?;
        }
        if self.target.link_arguments.len() < 2 {
            return Err("build-capture gap: the captured link arguments are incomplete".into());
        }
        if self.target.compilation_ids.is_empty() {
            return Err(format!(
                "build-capture gap: target '{}' has no captured membership",
                self.target.name
            ));
        }
        let mut members = BTreeSet::new();
        for id in &self.target.compilation_ids {
            if !ids.contains(id.as_str()) {
                return Err(format!(
                    "build-capture gap: target '{}' names uncaptured compilation '{id}'",
                    self.target.name
                ));
            }
            if !members.insert(id) {
                return Err(format!(
                    "target '{}' repeats compilation '{id}'",
                    self.target.name
                ));
            }
        }
        Ok(())
    }
}

impl CapturedBuildAcquisition {
    /// Revalidates a captured build against the snapshot it is retained in,
    /// without reopening any build file: the observation context must be the
    /// one capture observed, and membership must cover the acquired inputs
    /// one-to-one, in link order.
    pub(crate) fn validate(&self, snapshot: &PublishedSnapshot) -> Result<(), String> {
        self.capture.validate()?;
        let capture = &self.capture;
        if snapshot.observation_contexts().len() != 1 {
            return Err("build capture publishes exactly one selected target and context".into());
        }
        let context = &snapshot.observation_contexts()[0];
        let contributor = LlvmTextContributor::new(&capture.toolchain.compiler, &[]).identity();
        if context
            != &ObservationContext::static_analysis(
                &capture.program_snapshot_id,
                &capture.target.name,
                &capture.build_configuration,
                capture.toolchain.identity(),
                contributor.name,
                &context.extraction_version,
                &capture.analysis_stage,
            )
        {
            return Err("captured build disagrees with observation context".into());
        }
        let members = &capture.target.compilation_ids;
        if capture.compilations.len() != snapshot.acquired_inputs().len()
            || self.acquired_input_ids.len() != capture.compilations.len()
            || members.len() != capture.compilations.len()
        {
            return Err("captured membership does not cover acquired inputs".into());
        }
        for (((compilation, input), id), member) in capture
            .compilations
            .iter()
            .zip(snapshot.acquired_inputs())
            .zip(&self.acquired_input_ids)
            .zip(members)
        {
            if &compilation.id != member
                || &input.id != id
                || compilation.evidence_artifact != input.path
                || input.acquisition_method != CAPTURED_ACQUISITION_METHOD
                || input.media_type != "application/llvm-ir"
            {
                return Err("captured compilation does not match its acquired input".into());
            }
        }
        Ok(())
    }
}

/// Records one wrapped compiler invocation and runs the real compiler.
///
/// The wrapper re-enters `gloom` rather than quoting argv from a shell, so the
/// recorded arguments are the bytes the build passed. The invocation is
/// recorded after it finishes, with its exit status, so a probe the build
/// expected to fail is visible as a failure rather than as a compilation.
pub fn record_compilation(
    log: &Path,
    compiler: &Path,
    arguments: &[String],
) -> Result<i32, String> {
    let working_directory = std::env::current_dir().map_err(|error| error.to_string())?;
    let status = Command::new(compiler)
        .args(arguments)
        .status()
        .map_err(|error| format!("{}: {error}", compiler.display()))?;
    let mut argv = vec![compiler.display().to_string()];
    argv.extend(arguments.iter().cloned());
    let record = RecordedInvocation {
        working_directory: working_directory.display().to_string(),
        arguments: argv,
        exit_code: status.code(),
    };
    let mut line = serde_json::to_string(&record).map_err(|error| error.to_string())?;
    line.push('\n');
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log)
        .map_err(|error| format!("{}: {error}", log.display()))?;
    file.write_all(line.as_bytes())
        .map_err(|error| format!("{}: {error}", log.display()))?;
    Ok(status.code().unwrap_or(1))
}

/// Flags whose value is a separate argument, so that the token after them is
/// not mistaken for an input file.
const SEPARATED_VALUE_FLAGS: &[&str] = &[
    "-o",
    "-I",
    "-D",
    "-U",
    "-L",
    "-l",
    "-x",
    "-u",
    "-z",
    "-B",
    "-arch",
    "-target",
    "-include",
    "-imacros",
    "-isystem",
    "-iquote",
    "-idirafter",
    "-isysroot",
    "-MF",
    "-MT",
    "-MQ",
    "-Xclang",
    "-Xlinker",
    "-Xpreprocessor",
];

/// Dependency and output flags dropped when a compilation is replayed, so that
/// extraction writes its own IR and dependency files instead of overwriting the
/// build's.
const REPLACED_OUTPUT_FLAGS: &[&str] = &[
    "-c",
    "-S",
    "-emit-llvm",
    "-M",
    "-MM",
    "-MD",
    "-MMD",
    "-MG",
    "-MP",
];

struct ParsedInvocation<'a> {
    arguments: &'a [String],
    inputs: Vec<String>,
    output: Option<String>,
}

fn parse_invocation(arguments: &[String]) -> Result<ParsedInvocation<'_>, String> {
    let mut inputs = Vec::new();
    let mut output = None;
    let mut index = 1;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if let Some(path) = argument.strip_prefix("@") {
            return Err(format!(
                "build capture does not support the response file '@{path}'"
            ));
        }
        if argument.starts_with('-') {
            if argument == "-o" {
                output = arguments.get(index + 1).cloned();
                index += 2;
                continue;
            }
            if argument.len() > 2 && argument.starts_with("-o") {
                output = Some(argument["-o".len()..].to_owned());
                index += 1;
                continue;
            }
            if SEPARATED_VALUE_FLAGS.contains(&argument) {
                index += 2;
                continue;
            }
            index += 1;
            continue;
        }
        inputs.push(argument.to_owned());
        index += 1;
    }
    Ok(ParsedInvocation {
        arguments,
        inputs,
        output,
    })
}

impl ParsedInvocation<'_> {
    fn has(&self, flag: &str) -> bool {
        self.arguments.iter().any(|argument| argument == flag)
    }

    fn compiles(&self) -> bool {
        self.has("-c")
    }

    fn links(&self) -> bool {
        !self.compiles()
            && !self.has("-E")
            && !self.has("-S")
            && !self.has("-M")
            && !self.has("-MM")
            && !self.inputs.is_empty()
    }

    /// Why membership cannot be read from this link, or `None` when it can.
    fn unsupported_link(&self) -> Option<String> {
        if self.has("-shared") || self.has("-r") {
            return Some("it does not link an executable image".into());
        }
        if let Some(library) = self
            .arguments
            .iter()
            .find(|argument| argument.starts_with("-l") && argument.len() > 2)
        {
            return Some(format!(
                "its membership includes the library search '{library}', which capture does not observe"
            ));
        }
        self.inputs
            .iter()
            .find(|input| !input.ends_with(".o"))
            .map(|input| {
                format!(
                    "it links '{input}', which is not an object a captured compilation produced"
                )
            })
    }
}

fn absolute(working_directory: &str, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        Path::new(working_directory).join(candidate)
    }
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

struct Compilation {
    id: String,
    working_directory: String,
    source_input: String,
    arguments: Vec<String>,
    object_output: PathBuf,
}

struct Link {
    name: String,
    image_path: PathBuf,
    working_directory: String,
    arguments: Vec<String>,
    objects: Vec<PathBuf>,
}

/// Everything the wrapper observed, classified into compilations and links.
#[derive(Default)]
struct ClassifiedBuild {
    compilations: Vec<Compilation>,
    links: Vec<Link>,
    unsupported_links: BTreeMap<String, String>,
}

fn classify(invocations: Vec<RecordedInvocation>) -> Result<ClassifiedBuild, String> {
    let mut classified = ClassifiedBuild::default();
    let mut objects = BTreeSet::new();
    for invocation in invocations {
        if invocation.exit_code != Some(0) {
            continue;
        }
        let parsed = parse_invocation(&invocation.arguments)?;
        if parsed.compiles() {
            if parsed.inputs.len() != 1 {
                return Err(format!(
                    "build capture supports one source per compilation; captured {} in '{}'",
                    parsed.inputs.len(),
                    invocation.arguments.join(" ")
                ));
            }
            let source = parsed.inputs[0].clone();
            if !source.ends_with(".c") {
                return Err(format!(
                    "build capture supports C translation units; captured '{source}'"
                ));
            }
            let output = parsed.output.clone().ok_or_else(|| {
                format!(
                    "build-capture gap: the compilation of '{source}' names no object output, so its membership cannot be read"
                )
            })?;
            let object_output = canonical(&absolute(&invocation.working_directory, &output));
            if !objects.insert(object_output.clone()) {
                return Err(format!(
                    "build-capture gap: more than one captured compilation produced '{}'",
                    object_output.display()
                ));
            }
            classified.compilations.push(Compilation {
                id: output,
                working_directory: invocation.working_directory.clone(),
                source_input: absolute(&invocation.working_directory, &source)
                    .display()
                    .to_string(),
                arguments: invocation.arguments.clone(),
                object_output,
            });
        } else if parsed.links() {
            let image = parsed.output.clone().unwrap_or_else(|| "a.out".into());
            let image_path = absolute(&invocation.working_directory, &image);
            let name = image_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or(image);
            if let Some(reason) = parsed.unsupported_link() {
                classified.unsupported_links.insert(name, reason);
                continue;
            }
            classified.links.push(Link {
                name,
                image_path,
                working_directory: invocation.working_directory.clone(),
                arguments: invocation.arguments.clone(),
                objects: parsed
                    .inputs
                    .iter()
                    .map(|input| canonical(&absolute(&invocation.working_directory, input)))
                    .collect(),
            });
        }
    }
    Ok(classified)
}

impl ClassifiedBuild {
    /// The one link that produced the requested target, or a diagnostic naming
    /// what was observed instead.
    fn select(&self, target: &str) -> Result<&Link, String> {
        let mut matches = self.links.iter().filter(|link| link.name == target);
        let Some(selected) = matches.next() else {
            if let Some(reason) = self.unsupported_links.get(target) {
                return Err(format!(
                    "build capture does not support the link for target '{target}': {reason}"
                ));
            }
            let observed: Vec<_> = self.links.iter().map(|link| link.name.as_str()).collect();
            return Err(if observed.is_empty() {
                format!(
                    "build-capture gap: the captured build linked no supported executable target, so '{target}' has no observed membership"
                )
            } else {
                format!(
                    "unknown captured build target '{target}'; the build linked {}",
                    observed.join(", ")
                )
            });
        };
        if matches.next().is_some() {
            return Err(format!(
                "build-capture gap: the captured build linked '{target}' more than once, so its membership is ambiguous"
            ));
        }
        Ok(selected)
    }
}

/// The files a build produced: present when it finished, absent before it ran.
fn inventory(root: &Path, excluded: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path == *excluded || path.file_name().is_some_and(|name| name == ".git") {
                continue;
            }
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => pending.push(path),
                Ok(kind) if kind.is_file() => {
                    found.insert(canonical(&path));
                }
                _ => {}
            }
        }
    }
    found
}

/// The inputs a replayed compilation reported reading, from its dependency file.
fn dependency_inputs(text: &str) -> Vec<String> {
    let Some(body) = text.split_once(':').map(|(_, rest)| rest) else {
        return Vec::new();
    };
    let mut inputs = Vec::new();
    let mut current = String::new();
    let mut characters = body.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => match characters.next() {
                Some('\n') | None => {}
                Some(escaped) => current.push(escaped),
            },
            character if character.is_whitespace() => {
                if !current.is_empty() {
                    inputs.push(std::mem::take(&mut current));
                }
            }
            character => current.push(character),
        }
    }
    if !current.is_empty() {
        inputs.push(current);
    }
    inputs
}

/// The argument vector that replays one captured compilation as IR.
///
/// Everything the build passed is kept — definitions, include paths, language
/// and optimization settings — because the IR must be the IR of the compilation
/// that really ran. Only the output is redirected, and value names are retained
/// so that manifestations carry the names the source gave them.
fn extraction_arguments(arguments: &[String], artifact: &Path, dependencies: &Path) -> Vec<String> {
    let mut replayed = Vec::new();
    let mut index = 1;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument == "-o" || argument == "-MF" || argument == "-MT" || argument == "-MQ" {
            index += 2;
            continue;
        }
        if REPLACED_OUTPUT_FLAGS.contains(&argument)
            || (argument.starts_with("-o") && argument.len() > 2)
        {
            index += 1;
            continue;
        }
        replayed.push(argument.to_owned());
        index += 1;
    }
    replayed.extend([
        "-S".into(),
        "-emit-llvm".into(),
        "-fno-discard-value-names".into(),
        "-MD".into(),
        "-MF".into(),
        dependencies.display().to_string(),
        "-o".into(),
        artifact.display().to_string(),
    ]);
    replayed
}

fn probe(compiler: &str, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new(compiler)
        .args(arguments)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("'{compiler}' was not found; {UNSUPPORTED_PLATFORM}")
            } else {
                format!("'{compiler}': {error}")
            }
        })?;
    if !output.status.success() {
        return Err(format!(
            "'{compiler} {}' failed:\n{}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// Identifies the compiler capture is about to wrap, and refuses anything
/// outside the supported range instead of capturing a build it cannot replay.
fn supported_toolchain(compiler: &str) -> Result<CapturedToolchain, String> {
    if !cfg!(target_os = "linux") {
        return Err(UNSUPPORTED_PLATFORM.into());
    }
    let version = probe(compiler, &["--version"])?
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if !version.contains("clang version") {
        return Err(format!(
            "'{compiler}' reports '{version}', which is not Clang; {UNSUPPORTED_PLATFORM}"
        ));
    }
    let major = version
        .split("clang version ")
        .nth(1)
        .and_then(|rest| rest.split(['.', '-', ' ']).next())
        .and_then(|major| major.parse::<u32>().ok())
        .ok_or_else(|| format!("no Clang version could be read from '{version}'"))?;
    if !SUPPORTED_CLANG_MAJOR_VERSIONS.contains(&major) {
        return Err(format!(
            "build capture supports Clang {}-{}; '{compiler}' reports '{version}'",
            SUPPORTED_CLANG_MAJOR_VERSIONS.start(),
            SUPPORTED_CLANG_MAJOR_VERSIONS.end()
        ));
    }
    let target_triple = probe(compiler, &["-dumpmachine"])?;
    if !target_triple.contains("linux") {
        return Err(format!(
            "build capture supports Linux targets; '{compiler}' targets '{target_triple}'"
        ));
    }
    let compiler = probe(compiler, &["-print-prog-name=clang"]).unwrap_or_default();
    let compiler = if compiler.is_empty() || !Path::new(&compiler).is_absolute() {
        which(WRAPPED_COMPILER_NAME)?
    } else {
        compiler
    };
    Ok(CapturedToolchain {
        compiler,
        version,
        target_triple,
    })
}

fn which(name: &str) -> Result<String, String> {
    let candidate = Path::new(name);
    if candidate.is_absolute() {
        return Ok(name.to_owned());
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| {
            std::env::split_paths(&paths)
                .map(|directory| directory.join(name))
                .collect::<Vec<_>>()
        })
        .find(|path| path.is_file())
        .map(|path| path.display().to_string())
        .ok_or_else(|| format!("'{name}' was not found on PATH; {UNSUPPORTED_PLATFORM}"))
}

fn shell_quoted(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<(), String> {
    Err(UNSUPPORTED_PLATFORM.into())
}

/// Writes the wrapper the build invokes as `clang`, which records the
/// invocation and then runs the real compiler with the same arguments.
fn write_wrapper(
    directory: &Path,
    recorder: &Path,
    log: &Path,
    compiler: &str,
) -> Result<(), String> {
    std::fs::create_dir_all(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    let script = format!(
        "#!/bin/sh\nexec {recorder} capture-compilation --log {log} --compiler {compiler} -- \"$@\"\n",
        recorder = shell_quoted(&recorder.display().to_string()),
        log = shell_quoted(&log.display().to_string()),
        compiler = shell_quoted(compiler),
    );
    let path = directory.join(WRAPPED_COMPILER_NAME);
    std::fs::write(&path, script).map_err(|error| format!("{}: {error}", path.display()))?;
    make_executable(&path)
}

fn read_invocations(log: &Path) -> Result<Vec<RecordedInvocation>, String> {
    let text = match std::fs::read_to_string(log) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}", log.display())),
    };
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .map_err(|error| format!("unreadable capture record: {error}"))
        })
        .collect()
}

/// Runs the build once, under the wrapper, and returns what it did.
fn observe(
    request: &BuildCaptureRequest,
    toolchain: &CapturedToolchain,
) -> Result<Observed, String> {
    let project_root = std::fs::canonicalize(&request.project_root)
        .map_err(|error| format!("{}: {error}", request.project_root.display()))?;
    std::fs::create_dir_all(&request.capture_directory)
        .map_err(|error| format!("{}: {error}", request.capture_directory.display()))?;
    let capture_directory = std::fs::canonicalize(&request.capture_directory)
        .map_err(|error| format!("{}: {error}", request.capture_directory.display()))?;
    let log = capture_directory.join("invocations.jsonl");
    if log.exists() {
        return Err(format!(
            "build-capture gap: '{}' already holds a capture; capture into a new directory",
            capture_directory.display()
        ));
    }
    let wrapper_directory = capture_directory.join("toolchain");
    write_wrapper(
        &wrapper_directory,
        &request.recorder,
        &log,
        &toolchain.compiler,
    )?;
    let before = inventory(&project_root, &capture_directory);
    let path = match std::env::var_os("PATH") {
        Some(existing) => {
            let mut directories = vec![wrapper_directory.clone()];
            directories.extend(std::env::split_paths(&existing));
            std::env::join_paths(directories).map_err(|error| error.to_string())?
        }
        None => wrapper_directory.clone().into_os_string(),
    };
    let (command, arguments) = request
        .build_command
        .split_first()
        .ok_or("build-capture gap: no build command was given")?;
    let status = Command::new(command)
        .args(arguments)
        .current_dir(&project_root)
        .env("PATH", path)
        .status()
        .map_err(|error| format!("'{command}': {error}"))?;
    if !status.success() {
        return Err(format!(
            "the captured build command '{}' failed with {status}; nothing was published",
            request.build_command.join(" ")
        ));
    }
    let produced: BTreeSet<_> = inventory(&project_root, &capture_directory)
        .difference(&before)
        .cloned()
        .collect();
    Ok(Observed {
        project_root,
        capture_directory,
        produced,
        classified: classify(read_invocations(&log)?)?,
    })
}

struct Observed {
    project_root: PathBuf,
    capture_directory: PathBuf,
    produced: BTreeSet<PathBuf>,
    classified: ClassifiedBuild,
}

fn sanitized(id: &str) -> String {
    id.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// Replays one captured compilation into retained IR and reports the
/// build-produced inputs it read.
fn extract(
    observed: &Observed,
    toolchain: &CapturedToolchain,
    compilation: &Compilation,
    index: usize,
) -> Result<CapturedCompilation, String> {
    let directory = observed.capture_directory.join("evidence");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    let stem = format!("{index:03}-{}", sanitized(&compilation.id));
    let artifact = directory.join(format!("{stem}.ll"));
    let dependencies = directory.join(format!("{stem}.d"));
    let arguments = extraction_arguments(&compilation.arguments, &artifact, &dependencies);
    let output = Command::new(&toolchain.compiler)
        .args(&arguments)
        .current_dir(&compilation.working_directory)
        .output()
        .map_err(|error| format!("'{}': {error}", toolchain.compiler))?;
    if !output.status.success() {
        return Err(format!(
            "replaying the captured compilation of '{}' failed:\n{}",
            compilation.source_input,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut generated_inputs = Vec::new();
    let source = canonical(Path::new(&compilation.source_input));
    if observed.produced.contains(&source) {
        generated_inputs.push(CapturedGeneratedInput {
            path: source.display().to_string(),
            role: CapturedInputRole::TranslationUnitSource,
        });
    }
    let dependency_text = std::fs::read_to_string(&dependencies)
        .map_err(|error| format!("{}: {error}", dependencies.display()))?;
    for input in dependency_inputs(&dependency_text) {
        let path = canonical(&absolute(&compilation.working_directory, &input));
        if path != source && observed.produced.contains(&path) {
            generated_inputs.push(CapturedGeneratedInput {
                path: path.display().to_string(),
                role: CapturedInputRole::IncludedFile,
            });
        }
    }
    let mut argv = vec![toolchain.compiler.clone()];
    argv.extend(arguments);
    Ok(CapturedCompilation {
        id: compilation.id.clone(),
        working_directory: compilation.working_directory.clone(),
        source_input: compilation.source_input.clone(),
        compiler_arguments: compilation.arguments.clone(),
        object_output: compilation.object_output.display().to_string(),
        extraction_arguments: argv,
        evidence_artifact: artifact.display().to_string(),
        generated_inputs,
    })
}

/// Captures one build and publishes the selected target.
///
/// The build runs exactly once. What it did decides what is published: the
/// membership comes from the link Gloom observed, the evidence from replaying
/// the compilations that link named, and the observation context from the
/// compiler that ran and the build command that configured it.
pub(crate) fn publish(request: &BuildCaptureRequest) -> Result<PublishedSnapshot, String> {
    required(&request.program_snapshot_id, "program_snapshot_id")?;
    required(&request.target, "target")?;
    let toolchain = supported_toolchain(&request.compiler)?;
    let observed = observe(request, &toolchain)?;
    if observed.classified.compilations.is_empty() {
        return Err(format!(
            "build-capture gap: the build made no captured compilation; it must invoke its C compiler as '{WRAPPED_COMPILER_NAME}' on PATH"
        ));
    }
    let link = observed.classified.select(&request.target)?;
    let mut compilations = Vec::new();
    for (index, object) in link.objects.iter().enumerate() {
        let compilation = observed
            .classified
            .compilations
            .iter()
            .find(|compilation| &compilation.object_output == object)
            .ok_or_else(|| {
                format!(
                    "build-capture gap: target '{}' links '{}', which no captured compilation produced",
                    link.name,
                    object.display()
                )
            })?;
        if compilations
            .iter()
            .any(|captured: &CapturedCompilation| captured.id == compilation.id)
        {
            return Err(format!(
                "build-capture gap: target '{}' links '{}' more than once",
                link.name,
                object.display()
            ));
        }
        compilations.push(extract(&observed, &toolchain, compilation, index)?);
    }
    let capture = CapturedBuild {
        schema_version: CAPTURE_SCHEMA_VERSION.into(),
        program_snapshot_id: request.program_snapshot_id.clone(),
        build_configuration: format!("captured build: {}", request.build_command.join(" ")),
        analysis_stage: CAPTURED_ANALYSIS_STAGE.into(),
        toolchain: toolchain.clone(),
        build_command: request.build_command.clone(),
        project_root: observed.project_root.display().to_string(),
        capture_directory: observed.capture_directory.display().to_string(),
        target: CapturedTarget {
            name: link.name.clone(),
            image_path: link.image_path.display().to_string(),
            working_directory: link.working_directory.clone(),
            link_arguments: link.arguments.clone(),
            compilation_ids: compilations
                .iter()
                .map(|compilation| compilation.id.clone())
                .collect(),
        },
        compilations,
    };
    capture.validate()?;
    let contributor = LlvmTextContributor::new(&toolchain.compiler, &[]);
    let identity = contributor.identity();
    let context = ObservationContext::static_analysis(
        &capture.program_snapshot_id,
        &capture.target.name,
        &capture.build_configuration,
        toolchain.identity(),
        &identity.name,
        &identity.version,
        &capture.analysis_stage,
    );
    let contributions = capture
        .compilations
        .iter()
        .map(|compilation| {
            contributor
                .contribute_captured_compilation(
                    Path::new(&compilation.evidence_artifact),
                    &context,
                )
                .map_err(|error| {
                    format!(
                        "build-capture gap for compilation '{}': {error}",
                        compilation.id
                    )
                })
        })
        .collect::<Result<Vec<EvidenceContribution>, String>>()?;
    let snapshot = crate::snapshot::publish(contributions, identity, context)?;
    let acquisition = CapturedBuildAcquisition {
        capture,
        acquired_input_ids: snapshot
            .acquired_inputs()
            .iter()
            .map(|input| input.id.clone())
            .collect(),
    };
    snapshot.with_captured_build(acquisition)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_inputs_and_outputs_from_a_recorded_argument_vector() {
        let arguments: Vec<String> = ["clang", "-c", "-I", "gen", "-DX=1", "a.c", "-o", "a.o"]
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect();
        let parsed = parse_invocation(&arguments).unwrap();
        assert_eq!(parsed.inputs, ["a.c"]);
        assert_eq!(parsed.output.as_deref(), Some("a.o"));
        assert!(parsed.compiles());
        assert!(!parsed.links());
    }

    #[test]
    fn reports_link_membership_capture_cannot_observe() {
        let link = |arguments: &[&str]| {
            arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect::<Vec<_>>()
        };
        for (arguments, expected) in [
            (link(&["clang", "a.o", "-lm", "-o", "app"]), "-lm"),
            (link(&["clang", "a.o", "lib.a", "-o", "app"]), "lib.a"),
            (
                link(&["clang", "-shared", "a.o", "-o", "app"]),
                "executable",
            ),
        ] {
            let parsed = parse_invocation(&arguments).unwrap();
            assert!(parsed.links());
            let reason = parsed.unsupported_link().expect("unsupported membership");
            assert!(reason.contains(expected), "{reason}");
        }
        let supported = link(&["clang", "a.o", "b.o", "-o", "app"]);
        assert!(
            parse_invocation(&supported)
                .unwrap()
                .unsupported_link()
                .is_none()
        );
    }

    #[test]
    fn replays_a_compilation_into_ir_without_its_build_outputs() {
        let arguments: Vec<String> = [
            "clang", "-c", "-O0", "-MD", "-MF", "a.d", "-DX=1", "a.c", "-o", "a.o",
        ]
        .iter()
        .map(|argument| (*argument).to_owned())
        .collect();
        let replayed =
            extraction_arguments(&arguments, Path::new("/ir/a.ll"), Path::new("/ir/a.d"));
        assert!(!replayed.contains(&"-c".to_owned()));
        assert!(!replayed.contains(&"a.o".to_owned()));
        assert!(!replayed.contains(&"a.d".to_owned()));
        assert!(replayed.contains(&"-DX=1".to_owned()));
        assert!(replayed.contains(&"-O0".to_owned()));
        assert_eq!(replayed[replayed.len() - 1], "/ir/a.ll");
    }

    #[test]
    fn reads_the_inputs_a_replayed_compilation_reported() {
        let inputs = dependency_inputs("/ir/a.ll: ../a.c \\\n  gen/config.h  odd\\ name.h\n");
        assert_eq!(inputs, ["../a.c", "gen/config.h", "odd name.h"]);
    }
}
