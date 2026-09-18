//! Capture of a real Linux/Clang build, for builds whose declared artifacts do
//! not establish link membership.
//!
//! Where [`crate::acquisition`] trusts a build producer's manifest, capture
//! observes the build itself: every compiler invocation the build makes through
//! a generated wrapper is recorded with its verbatim argument vector, working
//! directory, environment, and the fingerprints of the files it read and
//! produced, and the link that produces one executable establishes that
//! target's membership. Gloom then replays each contributing compilation under
//! exactly those conditions to obtain its LLVM IR, and publishes a snapshot
//! scoped to that target.
//!
//! Nothing here reconstructs what Gloom did not observe, and nothing is
//! modelled by omission. A link carrying an argument capture does not fully
//! model, an object no captured compilation produced before that link, content
//! that changed between the compilation and its replay, or a target linked more
//! than once all fail with a diagnostic naming the gap.
use crate::contributor::fingerprint_byte_parts;
use crate::snapshot::{AcquiredInputId, ObservationContext, PublishedSnapshot};
use crate::{EvidenceContribution, LlvmTextContributor};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
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

/// The environment variables a captured compilation's record retains.
///
/// A replay runs under the compilation's entire recorded environment; these are
/// the variables that change what a compilation reads or produces, so they are
/// the ones kept as evidence beside the argument vector rather than left only
/// in the capture log.
pub const RETAINED_ENVIRONMENT_VARIABLES: &[&str] = &[
    "CPATH",
    "C_INCLUDE_PATH",
    "CPLUS_INCLUDE_PATH",
    "OBJC_INCLUDE_PATH",
    "COMPILER_PATH",
    "LIBRARY_PATH",
    "SDKROOT",
    "SOURCE_DATE_EPOCH",
    "DEPENDENCIES_OUTPUT",
    "SUNPRO_DEPENDENCIES",
    "CCC_OVERRIDE_OPTIONS",
    "CLANG_CONFIG_FILE",
];

/// The only arguments capture models in a link.
///
/// A link's membership is read from its argument vector, so an argument that
/// could name an input, forward something to the linker, or change what the
/// link produces would make that reading wrong. Rather than enumerate the ways
/// an argument can do that, capture accepts the arguments it models — an
/// output, object inputs, and these flags, none of which can name an input —
/// and refuses every link carrying anything else.
const MODELLED_LINK_FLAGS: &[&str] = &[
    "-g", "-O0", "-O1", "-O2", "-O3", "-Og", "-Os", "-Oz", "-pipe", "-pie", "-no-pie", "-fpie",
    "-fPIE", "-fno-pie", "-m32", "-m64",
];

const UNSUPPORTED_PLATFORM: &str =
    "build capture is supported only on Linux with Clang (see docs/build-capture.md)";

/// One compiler invocation the wrapper observed, exactly as the build made it.
///
/// This is the capture log's record, not the published one: it holds the whole
/// environment and every input fingerprint, which is what a faithful replay
/// needs and more than a snapshot should carry.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedInvocation {
    pub working_directory: String,
    /// The argument vector as executed, including the resolved compiler.
    pub arguments: Vec<String>,
    /// Absent when the compiler was terminated by a signal.
    pub exit_code: Option<i32>,
    pub environment: BTreeMap<String, String>,
    /// False when a variable could not be recorded as text, so this
    /// compilation's conditions cannot be reproduced.
    pub environment_recorded: bool,
    /// Content fingerprints of the files this compilation read, by resolved
    /// path, taken while the build was running.
    pub inputs: BTreeMap<String, String>,
    /// False when those inputs could not be enumerated.
    pub inputs_recorded: bool,
    /// The fingerprint of the object this compilation produced, taken as soon
    /// as it was produced.
    pub output_fingerprint: Option<String>,
}

/// The compiler capture observed, identified by what it reports about itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedToolchain {
    /// The resolved executable that ran, not the name it was asked for.
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
    /// The content the compilation read, as fingerprinted while it ran.
    pub content_fingerprint: String,
}

/// One compilation the build performed, and the IR replaying it produced.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedCompilation {
    /// The resolved object output the link names this compilation by.
    pub id: String,
    pub working_directory: String,
    pub source_input: String,
    /// The content of the translation unit this IR was produced from.
    pub source_fingerprint: String,
    /// The recorded argument vector, including the compiler. Replayed verbatim
    /// apart from the extraction adjustments below.
    pub compiler_arguments: Vec<String>,
    /// The variables in [`RETAINED_ENVIRONMENT_VARIABLES`] this compilation ran
    /// with. The replay ran under the whole recorded environment.
    pub environment: BTreeMap<String, String>,
    pub object_output: String,
    /// The object content the link consumed.
    pub object_fingerprint: String,
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
    /// The Clang the wrapper re-runs, by name on `PATH` or by path. It is
    /// resolved to one executable, and that executable is what is identified.
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
    /// Rereads the record's own argument vectors and checks that the
    /// membership, target, and compilations it publishes are the ones those
    /// vectors describe.
    ///
    /// This is what keeps a hand-edited export from renaming a target, adding a
    /// member, or pointing a compilation at another source: every published
    /// field is recomputed from the captured argument vectors, so a record that
    /// disagrees with what the build did never becomes a snapshot. No build
    /// file is reopened, so the check holds wherever the export is read.
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
            compilation.validate()?;
            if !ids.insert(compilation.id.as_str()) {
                return Err(format!(
                    "captured build records compilation '{}' more than once",
                    compilation.id
                ));
            }
            if compilation.compiler_arguments[0] != self.toolchain.compiler {
                return Err(format!(
                    "captured compilation '{}' did not run the identified compiler",
                    compilation.id
                ));
            }
        }
        self.target.validate()?;
        if self.target.compilation_ids.is_empty() {
            return Err(format!(
                "build-capture gap: target '{}' has no captured membership",
                self.target.name
            ));
        }
        for id in &self.target.compilation_ids {
            if !ids.contains(id.as_str()) {
                return Err(format!(
                    "build-capture gap: target '{}' names uncaptured compilation '{id}'",
                    self.target.name
                ));
            }
        }
        Ok(())
    }
}

impl CapturedCompilation {
    fn validate(&self) -> Result<(), String> {
        for (field, value) in [
            ("compilation id", self.id.as_str()),
            ("working_directory", self.working_directory.as_str()),
            ("source_input", self.source_input.as_str()),
            ("source_fingerprint", self.source_fingerprint.as_str()),
            ("object_output", self.object_output.as_str()),
            ("object_fingerprint", self.object_fingerprint.as_str()),
            ("evidence_artifact", self.evidence_artifact.as_str()),
        ] {
            required(value, field)?;
        }
        if self.extraction_arguments.len() < 2 {
            return Err("build-capture gap: extraction_arguments cannot be empty".into());
        }
        if Path::new(&self.evidence_artifact)
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("ll")
        {
            return Err("captured compilations are retained as .ll evidence artifacts".into());
        }
        for input in &self.generated_inputs {
            required(&input.path, "generated input path")?;
            required(&input.content_fingerprint, "generated input fingerprint")?;
        }
        // The published compilation must be the one its own argument vector
        // describes: the same translation unit, the same object, and a
        // compilation rather than something else the compiler was asked to do.
        let parsed = parse_invocation(&self.compiler_arguments)?;
        if !parsed.compiles() {
            return Err(format!(
                "captured compilation '{}' does not record a compilation",
                self.id
            ));
        }
        let [source] = parsed.inputs.as_slice() else {
            return Err(format!(
                "captured compilation '{}' does not record one translation unit",
                self.id
            ));
        };
        if resolved(&self.working_directory, source) != Path::new(&self.source_input) {
            return Err(format!(
                "captured compilation '{}' records another source than it compiled",
                self.id
            ));
        }
        let output = parsed.output.as_deref().ok_or_else(|| {
            format!(
                "captured compilation '{}' records no object output",
                self.id
            )
        })?;
        if resolved(&self.working_directory, output) != Path::new(&self.object_output)
            || self.id != self.object_output
        {
            return Err(format!(
                "captured compilation '{}' records another object than it produced",
                self.id
            ));
        }
        Ok(())
    }
}

impl CapturedTarget {
    fn validate(&self) -> Result<(), String> {
        for (field, value) in [
            ("target name", self.name.as_str()),
            ("target image_path", self.image_path.as_str()),
            ("target working_directory", self.working_directory.as_str()),
        ] {
            required(value, field)?;
        }
        // Membership is a reading of the captured link, so the published
        // membership must be exactly what rereading that link produces.
        let parsed = parse_invocation(&self.link_arguments)?;
        if !parsed.links() {
            return Err(format!(
                "captured target '{}' does not record a link",
                self.name
            ));
        }
        if let Some(reason) = parsed.unmodelled_link() {
            return Err(format!(
                "captured target '{}' records a link capture does not model: {reason}",
                self.name
            ));
        }
        let image = resolved(
            &self.working_directory,
            parsed.output.as_deref().unwrap_or("a.out"),
        );
        if image != Path::new(&self.image_path)
            || image.file_name().is_none_or(|name| name != &*self.name)
        {
            return Err(format!(
                "captured target '{}' records another image than its link produced",
                self.name
            ));
        }
        let membership: Vec<String> = parsed
            .inputs
            .iter()
            .map(|input| {
                resolved(&self.working_directory, input)
                    .display()
                    .to_string()
            })
            .collect();
        if membership != self.compilation_ids {
            return Err(format!(
                "captured target '{}' publishes membership its link does not name",
                self.name
            ));
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

fn fingerprint_of(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(fingerprint_byte_parts(&[&bytes]))
}

/// The environment of the process the wrapper is standing in for.
///
/// A variable that is not text cannot be replayed faithfully, so it is reported
/// rather than dropped quietly: the compilation it belongs to is refused.
fn current_environment() -> (BTreeMap<String, String>, bool) {
    let mut environment = BTreeMap::new();
    let mut complete = true;
    for (name, value) in std::env::vars_os() {
        match (name.into_string(), value.into_string()) {
            (Ok(name), Ok(value)) => {
                environment.insert(name, value);
            }
            _ => complete = false,
        }
    }
    (environment, complete)
}

/// Records one wrapped compiler invocation and runs the real compiler.
///
/// The wrapper re-enters `gloom` rather than quoting argv from a shell, so the
/// recorded arguments are the bytes the build passed. The invocation is
/// recorded after it finishes, with its exit status, so a probe the build
/// expected to fail is visible as a failure rather than as a compilation.
///
/// A successful compilation is also fingerprinted while the build is still
/// running: the files it read, found by asking the compiler for them, and the
/// object it produced. Those fingerprints are what let a later replay refuse to
/// read content that changed after the build compiled it.
pub fn record_compilation(
    log: &Path,
    compiler: &Path,
    arguments: &[String],
) -> Result<i32, String> {
    let working_directory = std::env::current_dir().map_err(|error| error.to_string())?;
    let (environment, environment_recorded) = current_environment();
    let status = Command::new(compiler)
        .args(arguments)
        .status()
        .map_err(|error| format!("{}: {error}", compiler.display()))?;
    let mut argv = vec![compiler.display().to_string()];
    argv.extend(arguments.iter().cloned());
    let mut record = RecordedInvocation {
        working_directory: working_directory.display().to_string(),
        arguments: argv,
        exit_code: status.code(),
        environment,
        environment_recorded,
        inputs: BTreeMap::new(),
        inputs_recorded: false,
        output_fingerprint: None,
    };
    if status.success() && parse_invocation(&record.arguments).is_ok_and(|parsed| parsed.compiles())
    {
        record.inputs_recorded = fingerprint_inputs(log, compiler, &record, &working_directory)
            .map(|inputs| record.inputs = inputs)
            .is_ok();
        record.output_fingerprint = parse_invocation(&record.arguments)
            .ok()
            .and_then(|parsed| parsed.output)
            .map(|output| resolved(&record.working_directory, &output))
            .and_then(|object| fingerprint_of(&object).ok());
    }
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

/// Asks the compiler which files the compilation just made reads, and
/// fingerprints each of them where the build left them.
fn fingerprint_inputs(
    log: &Path,
    compiler: &Path,
    record: &RecordedInvocation,
    working_directory: &Path,
) -> Result<BTreeMap<String, String>, String> {
    let scratch = log.with_file_name(format!(
        "inputs-{}-{}.d",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let mut arguments = without_output_arguments(&record.arguments);
    arguments.extend([
        "-M".into(),
        "-MF".into(),
        scratch.display().to_string(),
        "-o".into(),
        "/dev/null".into(),
    ]);
    let output = Command::new(compiler)
        .args(&arguments)
        .current_dir(working_directory)
        .output()
        .map_err(|error| format!("{}: {error}", compiler.display()))?;
    let text = std::fs::read_to_string(&scratch).map_err(|error| error.to_string());
    let _ = std::fs::remove_file(&scratch);
    if !output.status.success() {
        return Err(format!(
            "the compilation's inputs could not be enumerated:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let mut inputs = BTreeMap::new();
    for input in dependency_inputs(&text?) {
        let path = resolved(&record.working_directory, &input);
        let fingerprint = fingerprint_of(&path)?;
        inputs.insert(path.display().to_string(), fingerprint);
    }
    Ok(inputs)
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

/// Dependency and output flags dropped when a compilation is re-run, so that
/// Gloom writes its own outputs instead of overwriting the build's.
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
    if arguments.is_empty() {
        return Err("build-capture gap: an invocation records no compiler".into());
    }
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

    /// Whether this invocation asked for an image rather than for compilation
    /// or preprocessing. A link is recognized before it is judged, so that one
    /// capture cannot model membership is refused instead of overlooked.
    fn links(&self) -> bool {
        !self.compiles()
            && !self.has("-E")
            && !self.has("-S")
            && !self.has("-M")
            && !self.has("-MM")
            && (!self.inputs.is_empty() || self.output.is_some())
    }

    /// Why membership cannot be read from this link, or `None` when it can.
    ///
    /// Every argument must be one capture models. An unmodelled argument may
    /// name an input the membership reading would miss — `-Wl,extra.o`,
    /// `-Xlinker extra.o`, `-l m`, an archive — or change what the link
    /// produces, as a forwarded `-Wl,-r` does.
    fn unmodelled_link(&self) -> Option<String> {
        let mut index = 1;
        while index < self.arguments.len() {
            let argument = self.arguments[index].as_str();
            if argument == "-o" {
                index += 2;
                continue;
            }
            if !argument.starts_with('-') {
                if !argument.ends_with(".o") {
                    return Some(format!(
                        "it links '{argument}', which is not an object a captured compilation produced"
                    ));
                }
            } else if !(MODELLED_LINK_FLAGS.contains(&argument)
                || (argument.len() > 2 && argument.starts_with("-o")))
            {
                return Some(format!(
                    "it carries '{argument}', an argument capture does not model, which may name membership or change what the link produces"
                ));
            }
            index += 1;
        }
        if self.inputs.is_empty() {
            return Some("it links no captured object".into());
        }
        None
    }
}

/// An absolute path with `.` and `..` resolved by name.
///
/// Every path in a capture is compared against paths recorded by other
/// invocations of the same build and, later, recomputed when an export is read,
/// where no build file need still exist. So resolution is lexical: it gives the
/// same answer in all three places.
fn resolved(working_directory: &str, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    let absolute = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        Path::new(working_directory).join(candidate)
    };
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

struct Compilation {
    /// Where this compilation sits in the order the build ran.
    index: usize,
    working_directory: String,
    source_input: PathBuf,
    arguments: Vec<String>,
    environment: BTreeMap<String, String>,
    environment_recorded: bool,
    inputs: BTreeMap<String, String>,
    inputs_recorded: bool,
    object_output: PathBuf,
    output_fingerprint: Option<String>,
}

struct Link {
    index: usize,
    name: String,
    image_path: PathBuf,
    working_directory: String,
    arguments: Vec<String>,
    /// The objects this link named, or why capture does not model it.
    membership: Result<Vec<PathBuf>, String>,
}

/// Everything the wrapper observed, in the order the build ran it.
#[derive(Default)]
struct ClassifiedBuild {
    compilations: Vec<Compilation>,
    links: Vec<Link>,
}

fn classify(invocations: Vec<RecordedInvocation>) -> Result<ClassifiedBuild, String> {
    let mut classified = ClassifiedBuild::default();
    for (index, invocation) in invocations.into_iter().enumerate() {
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
            classified.compilations.push(Compilation {
                index,
                object_output: resolved(&invocation.working_directory, &output),
                source_input: resolved(&invocation.working_directory, &source),
                working_directory: invocation.working_directory,
                arguments: invocation.arguments,
                environment: invocation.environment,
                environment_recorded: invocation.environment_recorded,
                inputs: invocation.inputs,
                inputs_recorded: invocation.inputs_recorded,
                output_fingerprint: invocation.output_fingerprint,
            });
        } else if parsed.links() {
            let image_path = resolved(
                &invocation.working_directory,
                parsed.output.as_deref().unwrap_or("a.out"),
            );
            let Some(name) = image_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
            else {
                continue;
            };
            let membership = match parsed.unmodelled_link() {
                Some(reason) => Err(reason),
                None => Ok(parsed
                    .inputs
                    .iter()
                    .map(|input| resolved(&invocation.working_directory, input))
                    .collect()),
            };
            classified.links.push(Link {
                index,
                name,
                image_path,
                working_directory: invocation.working_directory,
                arguments: invocation.arguments,
                membership,
            });
        }
    }
    Ok(classified)
}

impl ClassifiedBuild {
    /// The one link that produced the requested target, or a diagnostic naming
    /// what was observed instead.
    ///
    /// Every link that produced the target counts, including ones capture does
    /// not model: a build that linked the target again produced a different
    /// image from the one a first link's membership would describe, so which
    /// one to publish is not Gloom's to choose.
    fn select(&self, target: &str) -> Result<&Link, String> {
        let produced: Vec<&Link> = self
            .links
            .iter()
            .filter(|link| link.name == target)
            .collect();
        match produced.as_slice() {
            [] => {
                let observed: Vec<_> = self.links.iter().map(|link| link.name.as_str()).collect();
                Err(if observed.is_empty() {
                    format!(
                        "build-capture gap: the captured build linked no executable target, so '{target}' has no observed membership"
                    )
                } else {
                    format!(
                        "unknown captured build target '{target}'; the build linked {}",
                        observed.join(", ")
                    )
                })
            }
            [link] => match &link.membership {
                Ok(_) => Ok(link),
                Err(reason) => Err(format!(
                    "build capture does not support the link for target '{target}': {reason}"
                )),
            },
            links => Err(format!(
                "build-capture gap: the captured build linked '{target}' {} times, so which link's membership the published image has is not observable",
                links.len()
            )),
        }
    }

    /// The compilation whose object a link consumed: the last one to produce
    /// that path before the link ran, whose object still holds the content it
    /// produced.
    fn member(&self, link: &Link, object: &Path) -> Result<&Compilation, String> {
        let compilation = self
            .compilations
            .iter()
            .rfind(|compilation| {
                compilation.object_output == object && compilation.index < link.index
            })
            .ok_or_else(|| {
                format!(
                    "build-capture gap: target '{}' links '{}', which no captured compilation produced before that link",
                    link.name,
                    object.display()
                )
            })?;
        let recorded = compilation.output_fingerprint.as_deref().ok_or_else(|| {
            format!(
                "build-capture gap: the object '{}' was not fingerprinted when it was produced",
                object.display()
            )
        })?;
        let current = fingerprint_of(object).map_err(|error| {
            format!("build-capture gap: the linked object cannot be read: {error}")
        })?;
        if current != recorded {
            return Err(format!(
                "build-capture gap: '{}' changed after the compilation that produced it, so the evidence for target '{}' would not be the evidence that was linked",
                object.display(),
                link.name
            ));
        }
        Ok(compilation)
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
                    found.insert(path);
                }
                _ => {}
            }
        }
    }
    found
}

/// The inputs a compilation reported reading, from its dependency file.
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

/// One invocation's arguments with its outputs and dependency files removed,
/// so that re-running it writes where Gloom says rather than where the build
/// did.
fn without_output_arguments(arguments: &[String]) -> Vec<String> {
    let mut kept = Vec::new();
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
        kept.push(argument.to_owned());
        index += 1;
    }
    kept
}

/// The argument vector that replays one captured compilation as IR.
///
/// Everything the build passed is kept — definitions, include paths, language
/// and optimization settings — because the IR must be the IR of the compilation
/// that really ran. Only the output is redirected, and value names are retained
/// so that manifestations carry the names the source gave them.
fn extraction_arguments(arguments: &[String], artifact: &Path, dependencies: &Path) -> Vec<String> {
    let mut replayed = without_output_arguments(arguments);
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
///
/// The requested compiler is resolved to one executable first, and it is that
/// executable which is asked what it is, wrapped into the build, and recorded.
/// Identifying one Clang and then running another would put a toolchain in the
/// observation context that never compiled anything.
fn supported_toolchain(compiler: &str) -> Result<CapturedToolchain, String> {
    if !cfg!(target_os = "linux") {
        return Err(UNSUPPORTED_PLATFORM.into());
    }
    let compiler = resolve_executable(compiler)?;
    let version = probe(&compiler, &["--version"])?
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
    let target_triple = probe(&compiler, &["-dumpmachine"])?;
    if !target_triple.contains("linux") {
        return Err(format!(
            "build capture supports Linux targets; '{compiler}' targets '{target_triple}'"
        ));
    }
    Ok(CapturedToolchain {
        compiler,
        version,
        target_triple,
    })
}

/// The one executable a requested compiler names, as an absolute path.
fn resolve_executable(compiler: &str) -> Result<String, String> {
    let candidate = Path::new(compiler);
    if candidate.components().count() > 1 || candidate.is_absolute() {
        let path = std::fs::canonicalize(candidate)
            .map_err(|error| format!("'{compiler}': {error}; {UNSUPPORTED_PLATFORM}"))?;
        if !path.is_file() {
            return Err(format!("'{compiler}' is not an executable file"));
        }
        return Ok(path.display().to_string());
    }
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|directory| directory.join(compiler))
        .find(|path| path.is_file())
        .map(|path| path.display().to_string())
        .ok_or_else(|| format!("'{compiler}' was not found on PATH; {UNSUPPORTED_PLATFORM}"))
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
/// invocation and then runs the identified compiler with the same arguments.
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

fn sanitized(name: &str) -> String {
    name.chars()
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
///
/// The replay runs under the compilation's own environment and working
/// directory, and only over content the build compiled: each input the wrapper
/// fingerprinted must still hold that content, and the replay must read exactly
/// those inputs. A build that rewrote a source after compiling it is refused
/// rather than published as the IR of content that was never linked.
fn extract(
    observed: &Observed,
    toolchain: &CapturedToolchain,
    compilation: &Compilation,
    index: usize,
) -> Result<CapturedCompilation, String> {
    let object = compilation.object_output.display().to_string();
    if !compilation.environment_recorded {
        return Err(format!(
            "build-capture gap: the environment of the compilation producing '{object}' could not be recorded, so it cannot be replayed"
        ));
    }
    if !compilation.inputs_recorded {
        return Err(format!(
            "build-capture gap: the inputs of the compilation producing '{object}' could not be fingerprinted, so replaying it could read other content"
        ));
    }
    for (path, fingerprint) in &compilation.inputs {
        let current = fingerprint_of(Path::new(path)).map_err(|error| {
            format!("build-capture gap: an input of the captured build cannot be read: {error}")
        })?;
        if &current != fingerprint {
            return Err(format!(
                "build-capture gap: '{path}' changed since the compilation that read it, so replaying that compilation would publish content the build never compiled"
            ));
        }
    }
    let directory = observed.capture_directory.join("evidence");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    let stem = format!(
        "{index:03}-{}",
        sanitized(
            &compilation
                .object_output
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        )
    );
    let artifact = directory.join(format!("{stem}.ll"));
    let dependencies = directory.join(format!("{stem}.d"));
    let arguments = extraction_arguments(&compilation.arguments, &artifact, &dependencies);
    let output = Command::new(&toolchain.compiler)
        .args(&arguments)
        .current_dir(&compilation.working_directory)
        .env_clear()
        .envs(&compilation.environment)
        .output()
        .map_err(|error| format!("'{}': {error}", toolchain.compiler))?;
    if !output.status.success() {
        return Err(format!(
            "replaying the captured compilation of '{}' failed:\n{}",
            compilation.source_input.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let dependency_text = std::fs::read_to_string(&dependencies)
        .map_err(|error| format!("{}: {error}", dependencies.display()))?;
    let replayed: BTreeSet<PathBuf> = dependency_inputs(&dependency_text)
        .iter()
        .map(|input| resolved(&compilation.working_directory, input))
        .collect();
    let recorded: BTreeSet<PathBuf> = compilation.inputs.keys().map(PathBuf::from).collect();
    if replayed != recorded {
        return Err(format!(
            "build-capture gap: replaying the compilation producing '{object}' read different inputs than the build did, so its IR would not be that compilation's"
        ));
    }
    let mut generated_inputs = Vec::new();
    for path in &replayed {
        if !observed.produced.contains(path) {
            continue;
        }
        let role = if path == &compilation.source_input {
            CapturedInputRole::TranslationUnitSource
        } else {
            CapturedInputRole::IncludedFile
        };
        generated_inputs.push(CapturedGeneratedInput {
            path: path.display().to_string(),
            role,
            content_fingerprint: compilation.inputs[&path.display().to_string()].clone(),
        });
    }
    let source_fingerprint = compilation
        .inputs
        .get(&compilation.source_input.display().to_string())
        .cloned()
        .ok_or_else(|| {
            format!("build-capture gap: the translation unit of '{object}' was not fingerprinted")
        })?;
    let mut argv = vec![toolchain.compiler.clone()];
    argv.extend(arguments);
    Ok(CapturedCompilation {
        id: object.clone(),
        working_directory: compilation.working_directory.clone(),
        source_input: compilation.source_input.display().to_string(),
        source_fingerprint,
        compiler_arguments: compilation.arguments.clone(),
        environment: RETAINED_ENVIRONMENT_VARIABLES
            .iter()
            .filter_map(|name| {
                compilation
                    .environment
                    .get(*name)
                    .map(|value| ((*name).to_owned(), value.clone()))
            })
            .collect(),
        object_output: object,
        object_fingerprint: compilation
            .output_fingerprint
            .clone()
            .expect("membership selection fingerprints every linked object"),
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
    let objects = link
        .membership
        .as_ref()
        .expect("a selected link has readable membership");
    let mut compilations = Vec::new();
    let mut members = BTreeSet::new();
    for (index, object) in objects.iter().enumerate() {
        if !members.insert(object) {
            return Err(format!(
                "build-capture gap: target '{}' links '{}' more than once",
                link.name,
                object.display()
            ));
        }
        let compilation = observed.classified.member(link, object)?;
        if compilation.arguments[0] != toolchain.compiler {
            return Err(format!(
                "build-capture gap: '{}' was produced by '{}' rather than by the identified compiler '{}'",
                object.display(),
                compilation.arguments[0],
                toolchain.compiler
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

    fn argv(arguments: &[&str]) -> Vec<String> {
        arguments
            .iter()
            .map(|argument| (*argument).to_owned())
            .collect()
    }

    #[test]
    fn reads_inputs_and_outputs_from_a_recorded_argument_vector() {
        let arguments = argv(&["clang", "-c", "-I", "gen", "-DX=1", "a.c", "-o", "a.o"]);
        let parsed = parse_invocation(&arguments).unwrap();
        assert_eq!(parsed.inputs, ["a.c"]);
        assert_eq!(parsed.output.as_deref(), Some("a.o"));
        assert!(parsed.compiles());
        assert!(!parsed.links());
    }

    #[test]
    fn refuses_every_link_argument_capture_does_not_model() {
        for (arguments, expected) in [
            (argv(&["clang", "a.o", "-lm", "-o", "app"]), "-lm"),
            (argv(&["clang", "a.o", "-l", "m", "-o", "app"]), "-l"),
            (argv(&["clang", "a.o", "-Wl,hidden.o", "-o", "app"]), "-Wl,"),
            (argv(&["clang", "a.o", "-Wl,-r", "-o", "app"]), "-Wl,-r"),
            (
                argv(&["clang", "-Xlinker", "hidden.o", "a.o", "-o", "app"]),
                "-Xlinker",
            ),
            (argv(&["clang", "-shared", "a.o", "-o", "app"]), "-shared"),
            (argv(&["clang", "-r", "a.o", "-o", "app"]), "-r"),
            (argv(&["clang", "a.o", "lib.a", "-o", "app"]), "lib.a"),
            (argv(&["clang", "a.c", "-o", "app"]), "a.c"),
            (argv(&["clang", "-L", "lib", "a.o", "-o", "app"]), "-L"),
            (argv(&["clang", "-o", "app"]), "links no captured object"),
        ] {
            let parsed = parse_invocation(&arguments).unwrap();
            assert!(parsed.links(), "{arguments:?}");
            let reason = parsed.unmodelled_link().expect("unmodelled membership");
            assert!(reason.contains(expected), "{reason}");
        }
        let modelled = argv(&["clang", "-g", "-O2", "-no-pie", "a.o", "b.o", "-o", "app"]);
        let parsed = parse_invocation(&modelled).unwrap();
        assert_eq!(parsed.unmodelled_link(), None);
        assert_eq!(parsed.inputs, ["a.o", "b.o"]);
    }

    #[test]
    fn replays_a_compilation_into_ir_without_its_build_outputs() {
        let arguments = argv(&[
            "clang", "-c", "-O0", "-MD", "-MF", "a.d", "-DX=1", "a.c", "-o", "a.o",
        ]);
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

    #[test]
    fn resolves_a_path_the_same_way_wherever_it_is_compared() {
        assert_eq!(
            resolved("/build/sub", "../unit.o"),
            PathBuf::from("/build/unit.o")
        );
        assert_eq!(
            resolved("/build", "./gen/../unit.o"),
            PathBuf::from("/build/unit.o")
        );
        assert_eq!(
            resolved("/build", "/other/unit.o"),
            PathBuf::from("/other/unit.o")
        );
        // Identically spelled outputs of different compilations stay distinct.
        assert_ne!(resolved("/a", "unit.o"), resolved("/b", "unit.o"));
    }
}
