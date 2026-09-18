//! Capturing a real Linux/Clang build and publishing one executable target.
//!
//! These tests run the fixture build for real. When no supported Clang is
//! installed there is no build to capture, so each test says so and stops
//! rather than asserting against a toolchain that is not there.
use gloom::PublishedSnapshot;
use gloom::app::Application;
use gloom::capture::{
    BuildCaptureRequest, CapturedBuild, CapturedInputRole, SUPPORTED_CLANG_MAJOR_VERSIONS,
};
use gloom::queries::{
    BoundedQuery, Investigation, InvestigationItem, QueryBounds, QueryScope, ResolutionPolicy,
    WorldPolicy,
};
use gloom::{CallableSelector, ObservationContext};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const SNAPSHOT_ID: &str = "build-capture-fixture-v1";

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/build-capture")
}

/// The Clang the fixture build will invoke, when one in the supported range is
/// installed. Capture refuses anything else, so there is nothing to test with.
fn supported_clang() -> Option<String> {
    let output = Command::new("clang").arg("--version").output().ok()?;
    let version = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    let major: u32 = version
        .split("clang version ")
        .nth(1)?
        .split(['.', '-', ' '])
        .next()?
        .parse()
        .ok()?;
    SUPPORTED_CLANG_MAJOR_VERSIONS
        .contains(&major)
        .then_some(version)
}

macro_rules! require_clang {
    () => {
        match supported_clang() {
            Some(version) => version,
            None => {
                eprintln!(
                    "no Clang {}-{} is installed, so there is no build to capture",
                    SUPPORTED_CLANG_MAJOR_VERSIONS.start(),
                    SUPPORTED_CLANG_MAJOR_VERSIONS.end()
                );
                return;
            }
        }
    };
}

/// A temporary directory holding project copies and capture directories, so a
/// captured build never writes into the checked-in fixture.
struct Workspace(PathBuf);

impl Workspace {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "gloom-capture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn directory(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    /// A copy of the fixture project whose build script is `script`.
    fn project_with(&self, name: &str, script: &str) -> PathBuf {
        let path = self.directory(name);
        for source in ["main.c", "support.c", "other.c"] {
            std::fs::copy(fixture().join(source), path.join(source)).unwrap();
        }
        std::fs::write(path.join("build.sh"), script).unwrap();
        path
    }

    fn project(&self, name: &str) -> PathBuf {
        self.project_with(
            name,
            &std::fs::read_to_string(fixture().join("build.sh")).unwrap(),
        )
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn request(project: &Path, capture_directory: &Path, target: &str) -> BuildCaptureRequest {
    BuildCaptureRequest {
        project_root: project.to_path_buf(),
        build_command: vec!["sh".into(), "build.sh".into()],
        target: target.into(),
        program_snapshot_id: SNAPSHOT_ID.into(),
        compiler: "clang".into(),
        capture_directory: capture_directory.to_path_buf(),
        recorder: PathBuf::from(env!("CARGO_BIN_EXE_gloom")),
    }
}

fn capture(workspace: &Workspace, name: &str, target: &str) -> Result<PublishedSnapshot, String> {
    let project = workspace.project(name);
    let capture_directory = workspace.0.join(format!("{name}-capture"));
    Application.capture_build(&request(&project, &capture_directory, target))
}

fn capture_script(
    workspace: &Workspace,
    name: &str,
    script: &str,
    target: &str,
) -> Result<PublishedSnapshot, String> {
    let project = workspace.project_with(name, script);
    let capture_directory = workspace.0.join(format!("{name}-capture"));
    Application.capture_build(&request(&project, &capture_directory, target))
}

fn investigation(snapshot: &PublishedSnapshot, target: &str, query: Investigation) -> BoundedQuery {
    BoundedQuery {
        scope: QueryScope {
            build_target: target.into(),
            observation_context_ids: vec![snapshot.observation_contexts()[0].id.clone()],
        },
        resolution_policy: ResolutionPolicy::IncludePossible,
        world: WorldPolicy::Open {},
        bounds: QueryBounds {
            max_depth: 4,
            max_results: 100,
            max_steps: 100_000,
        },
        query,
    }
}

fn callable_names(snapshot: &PublishedSnapshot, target: &str) -> Vec<String> {
    let result = Application
        .investigate_snapshot(
            snapshot,
            &investigation(
                snapshot,
                target,
                Investigation::CallableSearch {
                    label: String::new(),
                },
            ),
        )
        .unwrap();
    let mut names: Vec<String> = result
        .items
        .iter()
        .filter_map(|item| match item {
            InvestigationItem::Callable { display_name, .. } => Some(display_name.clone()),
            _ => None,
        })
        .collect();
    names.sort();
    names.dedup();
    names
}

/// A compilation is identified by the resolved object it produced, so a test
/// names one by the object's file name.
fn compilation<'a>(
    capture: &'a CapturedBuild,
    object: &str,
) -> &'a gloom::capture::CapturedCompilation {
    let suffix = format!("/{object}");
    capture
        .compilations
        .iter()
        .find(|compilation| compilation.id.ends_with(&suffix))
        .unwrap_or_else(|| panic!("captured compilation producing {object}"))
}

fn member_objects(capture: &CapturedBuild) -> Vec<String> {
    capture
        .target
        .compilation_ids
        .iter()
        .map(|id| {
            Path::new(id)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn captures_compilation_and_link_membership_for_one_executable_target() {
    let version = require_clang!();
    let workspace = Workspace::new();
    let snapshot = capture(&workspace, "server", "server").unwrap();

    // The published target holds exactly the translation units the captured
    // link named, and nothing the build compiled for its other target.
    assert_eq!(
        callable_names(&snapshot, "server"),
        ["helper", "main", "prepare", "worker"]
    );
    assert_eq!(snapshot.acquired_inputs().len(), 3);
    let acquisition = snapshot.captured_build().unwrap();
    let capture = &acquisition.capture;
    assert_eq!(member_objects(capture), ["main.o", "worker.o", "support.o"]);
    for id in &capture.target.compilation_ids {
        assert!(Path::new(id).is_absolute(), "{id}");
    }
    assert_eq!(capture.target.name, "server");
    assert!(capture.target.image_path.ends_with("/build/server"));
    assert!(capture.target.link_arguments.contains(&"main.o".to_owned()));
    assert!(capture.target.working_directory.ends_with("/build"));

    // Captured compiler arguments, working directories, and generated inputs.
    let main = compilation(capture, "main.o");
    assert_eq!(main.compiler_arguments[1], "-c");
    assert!(
        main.compiler_arguments
            .contains(&"-DCAPTURED_BUILD=1".to_owned())
    );
    assert!(main.compiler_arguments.contains(&"-Igenerated".to_owned()));
    assert!(main.working_directory.ends_with("/build"));
    assert!(main.source_input.ends_with("/main.c"));
    assert_eq!(main.generated_inputs.len(), 1);
    assert!(
        main.generated_inputs[0]
            .path
            .ends_with("/build/generated/config.h")
    );
    assert_eq!(
        main.generated_inputs[0].role,
        CapturedInputRole::IncludedFile
    );
    let worker = compilation(capture, "worker.o");
    assert_eq!(
        worker.generated_inputs[0].role,
        CapturedInputRole::TranslationUnitSource
    );
    assert!(
        worker.generated_inputs[0]
            .path
            .ends_with("/build/generated/worker.c")
    );
    assert!(
        compilation(capture, "support.o")
            .generated_inputs
            .is_empty()
    );

    // Toolchain identity is what the captured compiler reported about itself,
    // and it qualifies the observation context the evidence is bound to.
    assert_eq!(capture.toolchain.version, version.trim());
    assert!(capture.toolchain.target_triple.contains("linux"));
    let context = &snapshot.observation_contexts()[0];
    assert_eq!(context.build_target, "server");
    assert_eq!(context.toolchain, capture.toolchain.identity());
    assert_eq!(context.build_configuration, "captured build: sh build.sh");
    assert_eq!(context.analysis_stage, capture.analysis_stage);

    // Every acquired input is the IR of one captured compilation, cited by the
    // evidence a query explains itself with.
    for (input, compilation) in snapshot.acquired_inputs().iter().zip(&capture.compilations) {
        assert_eq!(input.path, compilation.evidence_artifact);
        assert_eq!(input.acquisition_method, "captured-build");
    }
    let path = Application
        .investigate_snapshot(
            &snapshot,
            &investigation(
                &snapshot,
                "server",
                Investigation::CallPath {
                    start: CallableSelector::by_label("main"),
                    end: CallableSelector::by_label("helper"),
                },
            ),
        )
        .unwrap();
    let InvestigationItem::Path { relationships } = &path.items[0] else {
        panic!("a captured path from main to helper");
    };
    assert_eq!(relationships.len(), 2);
    let explanation = Application
        .explain_snapshot(&snapshot, &relationships[1].explanation_handle)
        .unwrap();
    let worker_input = &acquisition.acquired_input_ids[member_objects(capture)
        .iter()
        .position(|object| object == "worker.o")
        .unwrap()];
    assert!(
        explanation
            .evidence_records
            .iter()
            .any(|record| &record.acquired_input_id == worker_input)
    );

    let exported = Application.export_snapshot_json(&snapshot).unwrap();
    assert_eq!(Application.load_snapshot_json(&exported).unwrap(), snapshot);
}

#[test]
fn publishes_only_the_selected_target_from_the_same_captured_build() {
    require_clang!();
    let workspace = Workspace::new();
    let other = capture(&workspace, "other", "other-tool").unwrap();

    assert_eq!(callable_names(&other, "other-tool"), ["main", "other_only"]);
    assert_eq!(other.acquired_inputs().len(), 1);
    let capture = &other.captured_build().unwrap().capture;
    assert_eq!(member_objects(capture), ["other.o"]);
    assert!(
        compilation(capture, "other.o")
            .source_input
            .ends_with("/other.c")
    );
}

#[test]
fn the_recorded_toolchain_is_the_compiler_that_actually_ran() {
    let version = require_clang!();
    let workspace = Workspace::new();
    let snapshot = capture(&workspace, "toolchain", "server").unwrap();
    let capture = &snapshot.captured_build().unwrap().capture;

    // The requested compiler was a name; what is identified, wrapped into the
    // build, and recorded is the one executable that name resolved to.
    let compiler = Path::new(&capture.toolchain.compiler);
    assert!(compiler.is_absolute(), "{}", compiler.display());
    assert!(compiler.is_file(), "{}", compiler.display());
    let reported = Command::new(compiler).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&reported.stdout)
            .lines()
            .next()
            .unwrap(),
        capture.toolchain.version
    );
    assert_eq!(capture.toolchain.version, version.trim());
    assert_eq!(
        String::from_utf8_lossy(
            &Command::new(compiler)
                .arg("-dumpmachine")
                .output()
                .unwrap()
                .stdout
        )
        .trim(),
        capture.toolchain.target_triple
    );
    // Every published compilation ran that same executable.
    for compilation in &capture.compilations {
        assert_eq!(
            compilation.compiler_arguments[0],
            capture.toolchain.compiler
        );
    }

    // A compiler that reports something else is refused rather than identified
    // as the one that was asked for.
    let elsewhere = workspace.directory("elsewhere");
    let impostor = elsewhere.join("clang");
    std::fs::write(
        &impostor,
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo 'clang version 99.0.0'; else echo x86_64-pc-linux-gnu; fi\n",
    )
    .unwrap();
    Command::new("chmod")
        .arg("+x")
        .arg(&impostor)
        .status()
        .unwrap();
    let project = workspace.project("impostor");
    let mut substituted = request(&project, &workspace.0.join("impostor-capture"), "server");
    substituted.compiler = impostor.display().to_string();
    let error = Application.capture_build(&substituted).unwrap_err();
    assert!(error.contains("supports Clang 14-20"), "{error}");
    assert!(error.contains("99.0.0"), "{error}");
}

#[test]
fn identically_spelled_objects_of_different_compilations_stay_distinct() {
    require_clang!();
    let workspace = Workspace::new();
    // Two compilations name their output `unit.o` from different working
    // directories. They are two objects, and the target links both.
    let script = "set -eu\nmkdir -p a b\ncp support.c a/unit.c\nsed 's/helper/second/' support.c > b/unit.c\ncd a\nclang -c -O0 unit.c -o unit.o\ncd ../b\nclang -c -O0 unit.c -o unit.o\ncd ..\nclang -c -O0 other.c -o other.o\nclang a/unit.o b/unit.o other.o -o app\n";
    let snapshot = capture_script(&workspace, "colliding", script, "app").unwrap();

    let capture = &snapshot.captured_build().unwrap().capture;
    assert_eq!(member_objects(capture), ["unit.o", "unit.o", "other.o"]);
    assert_eq!(capture.compilations.len(), 3);
    let ids: std::collections::BTreeSet<&String> = capture.target.compilation_ids.iter().collect();
    assert_eq!(ids.len(), 3, "{:?}", capture.target.compilation_ids);
    assert_eq!(
        callable_names(&snapshot, "app"),
        ["helper", "main", "other_only", "second"]
    );
}

#[test]
fn reloading_a_captured_snapshot_revalidates_membership_and_context() {
    require_clang!();
    let workspace = Workspace::new();
    let snapshot = capture(&workspace, "reload", "server").unwrap();
    let value = serde_json::to_value(&snapshot).unwrap();

    let historical = ObservationContext::static_analysis(
        snapshot.observation_contexts()[0]
            .program_snapshot_id
            .as_str(),
        "server",
        &snapshot.observation_contexts()[0].build_configuration,
        &snapshot.observation_contexts()[0].toolchain,
        &snapshot.observation_contexts()[0].extraction_method,
        "0.0.1",
        &snapshot.observation_contexts()[0].analysis_stage,
    );
    let mut aged: Value = serde_json::from_str(&serde_json::to_string(&value).unwrap().replace(
        &serde_json::to_string(&snapshot.observation_contexts()[0].id).unwrap(),
        &serde_json::to_string(&historical.id).unwrap(),
    ))
    .unwrap();
    aged["observation_contexts"][0] = serde_json::to_value(&historical).unwrap();
    let loaded = Application.load_snapshot_json(&aged.to_string()).unwrap();
    assert_eq!(loaded.observation_contexts(), &[historical]);
    assert_eq!(
        callable_names(&loaded, "server"),
        callable_names(&snapshot, "server")
    );

    for field in [
        "program_snapshot_id",
        "build_configuration",
        "analysis_stage",
    ] {
        let mut corrupted = value.clone();
        corrupted["captured_build"]["capture"][field] = json!("another");
        let error = Application
            .load_snapshot_json(&corrupted.to_string())
            .unwrap_err();
        assert!(
            error.contains("disagrees with observation context")
                || error.contains("analysis stage"),
            "{error}"
        );
    }
    // Membership, target, and compilations are reread from the recorded
    // argument vectors on load, so a record that publishes anything those
    // vectors do not describe never becomes a snapshot again.
    let members = value["captured_build"]["capture"]["target"]["compilation_ids"]
        .as_array()
        .unwrap()
        .clone();
    for (field, corruption, diagnostic) in [
        (
            "name",
            json!("another"),
            "records another image than its link produced",
        ),
        (
            "image_path",
            json!("/elsewhere/server"),
            "records another image than its link produced",
        ),
        (
            "compilation_ids",
            json!([members[0], members[1]]),
            "publishes membership its link does not name",
        ),
        (
            "compilation_ids",
            json!([members[1], members[0], members[2]]),
            "publishes membership its link does not name",
        ),
        (
            "link_arguments",
            json!(["/usr/bin/clang", "main.o", "-lm", "-o", "server"]),
            "capture does not model",
        ),
        (
            "link_arguments",
            json!(["/usr/bin/clang", "main.o", "-o", "server"]),
            "publishes membership its link does not name",
        ),
        (
            "working_directory",
            json!("/elsewhere"),
            "records another image than its link produced",
        ),
    ] {
        let mut corrupted = value.clone();
        corrupted["captured_build"]["capture"]["target"][field] = corruption;
        let error = Application
            .load_snapshot_json(&corrupted.to_string())
            .unwrap_err();
        assert!(error.contains(diagnostic), "{field}: {error}");
    }
    for (field, corruption, diagnostic) in [
        (
            "compiler_arguments",
            json!(["/usr/bin/clang", "-c", "../other.c", "-o", "main.o"]),
            "records another source than it compiled",
        ),
        (
            "compiler_arguments",
            json!(["/usr/bin/clang", "-c", "../main.c", "-o", "elsewhere.o"]),
            "records another object than it produced",
        ),
        (
            "compiler_arguments",
            json!(["/usr/bin/clang", "../main.c", "-o", "main.o"]),
            "does not record a compilation",
        ),
        (
            "object_output",
            json!("/elsewhere/main.o"),
            "records another object than it produced",
        ),
        (
            "source_input",
            json!("/elsewhere/main.c"),
            "records another source than it compiled",
        ),
    ] {
        let mut corrupted = value.clone();
        corrupted["captured_build"]["capture"]["compilations"][0][field] = corruption;
        let error = Application
            .load_snapshot_json(&corrupted.to_string())
            .unwrap_err();
        assert!(error.contains(diagnostic), "{field}: {error}");
    }
    let mut corrupted = value.clone();
    corrupted["captured_build"]["capture"]["compilations"][0]["compiler_arguments"][0] =
        json!("/elsewhere/clang");
    assert!(
        Application
            .load_snapshot_json(&corrupted.to_string())
            .unwrap_err()
            .contains("did not run the identified compiler")
    );
    let mut corrupted = value;
    corrupted["captured_build"]["acquired_input_ids"][0] = json!("unknown");
    assert!(
        Application
            .load_snapshot_json(&corrupted.to_string())
            .is_err()
    );
}

/// The declaration a build producer would write for the build capture just
/// observed, naming the same compilations, membership, and artifacts.
fn declared_manifest(capture: &CapturedBuild, path: &Path) -> PathBuf {
    let compilations: Vec<Value> = capture
        .compilations
        .iter()
        .map(|compilation| {
            json!({
                "id": compilation.id,
                "working_directory": compilation.working_directory,
                "source_input": compilation.source_input,
                "compiler_arguments": compilation.compiler_arguments,
                "evidence_artifact": compilation.evidence_artifact,
                "generated_inputs": compilation
                    .generated_inputs
                    .iter()
                    .map(|input| json!({
                        "path": input.path,
                        "kind": match input.role {
                            CapturedInputRole::TranslationUnitSource => "generated",
                            CapturedInputRole::IncludedFile => "configured",
                        },
                    }))
                    .collect::<Vec<_>>(),
            })
        })
        .collect();
    let manifest = json!({
        "schema_version": "1",
        "program_snapshot_id": capture.program_snapshot_id,
        "build_configuration": capture.build_configuration,
        "toolchain": capture.toolchain.identity(),
        "analysis_stage": capture.analysis_stage,
        "compilations": compilations,
        "targets": [{
            "name": capture.target.name,
            "compilation_ids": capture.target.compilation_ids,
        }],
    });
    std::fs::write(path, serde_json::to_string_pretty(&manifest).unwrap()).unwrap();
    path.to_path_buf()
}

#[test]
fn captured_and_declared_acquisition_answer_bounded_named_queries_identically() {
    require_clang!();
    let workspace = Workspace::new();
    let captured = capture(&workspace, "parity", "server").unwrap();
    let manifest = declared_manifest(
        &captured.captured_build().unwrap().capture,
        &workspace.directory("declared").join("manifest.json"),
    );
    let declared = Application
        .publish_declared_build(&manifest, "server")
        .unwrap();

    // The same build, described two ways, is the same observation context and
    // the same acquired evidence; only how it was acquired differs.
    assert_eq!(
        captured.observation_contexts(),
        declared.observation_contexts()
    );
    for (captured_input, declared_input) in captured
        .acquired_inputs()
        .iter()
        .zip(declared.acquired_inputs())
    {
        assert_eq!(captured_input.path, declared_input.path);
        assert_eq!(
            captured_input.content_fingerprint,
            declared_input.content_fingerprint
        );
        assert_eq!(captured_input.acquisition_method, "captured-build");
        assert_eq!(declared_input.acquisition_method, "declared-artifact");
    }

    // The two accounts stay separate: a document claiming both would leave a
    // reader no way to tell which acquisition the evidence answers to.
    let mut both = serde_json::to_value(&captured).unwrap();
    both["declared_build"] = serde_json::to_value(&declared).unwrap()["declared_build"].clone();
    assert!(
        Application
            .load_snapshot_json(&both.to_string())
            .unwrap_err()
            .contains("one build acquisition"),
    );

    for query in [
        Investigation::CallableSearch {
            label: String::new(),
        },
        Investigation::Callees {
            caller: CallableSelector::by_label("main"),
        },
        Investigation::Callers {
            callee: CallableSelector::by_label("helper"),
        },
        Investigation::CallPath {
            start: CallableSelector::by_label("main"),
            end: CallableSelector::by_label("helper"),
        },
        Investigation::RecursiveCycles {
            start: CallableSelector::by_label("main"),
        },
    ] {
        let request = investigation(&captured, "server", query);
        let from_capture = Application
            .investigate_snapshot(&captured, &request)
            .unwrap();
        let from_declaration = Application
            .investigate_snapshot(&declared, &request)
            .unwrap();
        assert_eq!(
            serde_json::to_value(&from_capture).unwrap(),
            serde_json::to_value(&from_declaration).unwrap(),
            "{} answered differently",
            from_capture.query_name
        );
    }
}

#[test]
fn a_replay_runs_under_the_environment_its_compilation_ran_under() {
    require_clang!();
    let workspace = Workspace::new();
    // The build finds `pick.h` only through CPATH, and that header decides
    // which callable the translation unit calls. A replay that inherited
    // Gloom's environment instead would compile something the build never did.
    let script = "set -eu\nmkdir -p build/config\ncat > build/config/pick.h <<'HEADER'\nvoid right(void);\n#define PICKED right\nHEADER\ncat > build/unit.c <<'SOURCE'\n#include \"pick.h\"\nvoid right(void) {}\nvoid wrong(void) {}\nint main(void) { PICKED(); return 0; }\nSOURCE\ncd build\nCPATH=config clang -c -O0 unit.c -o unit.o\nclang unit.o -o picked\n";
    let snapshot = capture_script(&workspace, "environment", script, "picked").unwrap();

    let result = Application
        .investigate_snapshot(
            &snapshot,
            &investigation(
                &snapshot,
                "picked",
                Investigation::Callees {
                    caller: CallableSelector::by_label("main"),
                },
            ),
        )
        .unwrap();
    let callees: Vec<String> = result
        .items
        .iter()
        .filter_map(|item| match item {
            InvestigationItem::Relationship { relationship } => {
                Some(relationship.callee_display_name.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(callees, ["right"]);
    let capture = &snapshot.captured_build().unwrap().capture;
    assert_eq!(
        compilation(capture, "unit.o").environment.get("CPATH"),
        Some(&"config".to_owned())
    );
}

#[test]
fn content_that_changed_after_its_compilation_is_never_published() {
    require_clang!();
    let workspace = Workspace::new();
    // The build compiles a translation unit and then rewrites its source, so
    // the content on disk at replay is not the content that was compiled.
    let rewritten = "set -eu\nmkdir -p build\ncd build\ncp ../support.c unit.c\nclang -c -O0 unit.c -o unit.o\nclang -c -O0 ../other.c -o other.o\nsed 's/helper/rewritten/' ../support.c > unit.c\nclang unit.o other.o -o app\n";
    let error = capture_script(&workspace, "rewritten-source", rewritten, "app").unwrap_err();
    assert!(error.contains("changed since the compilation"), "{error}");
    assert!(error.contains("unit.c"), "{error}");

    // The same holds for a header the compilation read.
    let header = "set -eu\nmkdir -p build\ncd build\ncat > pick.h <<'HEADER'\nvoid helper(void);\nHEADER\ncp ../support.c unit.c\nclang -c -O0 -include pick.h unit.c -o unit.o\nclang -c -O0 ../other.c -o other.o\necho '/* rewritten */' >> pick.h\nclang unit.o other.o -o app\n";
    let error = capture_script(&workspace, "rewritten-header", header, "app").unwrap_err();
    assert!(error.contains("changed since the compilation"), "{error}");
    assert!(error.contains("pick.h"), "{error}");

    // An object rewritten after the link that consumed it is refused too: the
    // evidence would not be the evidence that was linked.
    let object = "set -eu\nmkdir -p build\ncd build\ncp ../support.c unit.c\nclang -c -O0 unit.c -o unit.o\nclang -c -O0 ../other.c -o other.o\nclang unit.o other.o -o app\nsed 's/helper/relinked/' ../support.c > unit.c\nclang -c -O0 unit.c -o unit.o\n";
    let error = capture_script(&workspace, "rewritten-object", object, "app").unwrap_err();
    assert!(
        error.contains("changed after the compilation that produced it"),
        "{error}"
    );

    // And an object produced only after the link cannot be that link's member.
    let late = "set -eu\nmkdir -p build\ncd build\nclang -c -O0 ../other.c -o other.o\ncp other.o spare.o\nclang spare.o -o app\nclang -c -O0 ../support.c -o spare.o\n";
    let error = capture_script(&workspace, "late-object", late, "app").unwrap_err();
    assert!(
        error.contains("no captured compilation produced before that link"),
        "{error}"
    );
}

#[test]
fn a_target_linked_again_is_refused_rather_than_published_from_the_first_link() {
    require_clang!();
    let workspace = Workspace::new();
    // The published image is the second link's, and capture does not model
    // that link, so neither link's membership may be published as the target.
    let relinked = "set -eu\nmkdir -p build\ncd build\nclang -c -O0 ../other.c -o other.o\nclang other.o -o app\nclang other.o -lm -o app\n";
    let error = capture_script(&workspace, "relinked", relinked, "app").unwrap_err();
    assert!(error.contains("linked 'app' 2 times"), "{error}");

    // The same refusal holds when both links are ones capture does model.
    let twice = "set -eu\nmkdir -p build\ncd build\nclang -c -O0 ../other.c -o other.o\nclang -c -O0 ../support.c -o support.o\nclang other.o -o app\nclang other.o support.o -o app\n";
    let error = capture_script(&workspace, "linked-twice", twice, "app").unwrap_err();
    assert!(error.contains("linked 'app' 2 times"), "{error}");
}

#[test]
fn link_membership_capture_cannot_read_is_refused_rather_than_partly_modelled() {
    require_clang!();
    let workspace = Workspace::new();
    let real_clang = resolved_clang();

    // An object forwarded straight to the linker is membership no reading of
    // the driver's inputs would report, so the link is refused outright.
    for (name, link, expected) in [
        (
            "forwarded-object",
            "clang other.o -Wl,hidden.o -o app",
            "-Wl,",
        ),
        (
            "linker-argument",
            "clang other.o -Xlinker hidden.o -o app",
            "-Xlinker",
        ),
        ("separated-library", "clang other.o -l m -o app", "-l"),
        (
            // A forwarded `-r` produces a relocatable object, not the
            // executable image a target's membership describes.
            "relocatable",
            "clang -Wl,-r -nostdlib -no-pie other.o -o app",
            "-Wl,-r",
        ),
        ("library-path", "clang other.o -L /usr/lib -o app", "-L"),
    ] {
        let script = format!(
            "set -eu\nmkdir -p build\ncd build\nclang -c -O0 ../other.c -o other.o\n{real_clang} -c -O0 ../support.c -o hidden.o\n{link}\n"
        );
        let error = capture_script(&workspace, name, &script, "app").unwrap_err();
        assert!(
            error.contains("does not support the link"),
            "{name}: {error}"
        );
        assert!(error.contains(expected), "{name}: {error}");
    }
}

/// The absolute path the `clang` on PATH resolves to, for builds that must
/// reach a compiler capture is not wrapping.
fn resolved_clang() -> String {
    String::from_utf8_lossy(
        &Command::new("sh")
            .args(["-c", "command -v clang"])
            .output()
            .unwrap()
            .stdout,
    )
    .trim()
    .to_owned()
}

#[test]
fn unsupported_or_incomplete_capture_fails_without_reconstructing_configuration() {
    require_clang!();
    let workspace = Workspace::new();
    let real_clang = resolved_clang();

    // A target the captured build never linked.
    let error = capture(&workspace, "unknown-target", "daemon").unwrap_err();
    assert!(
        error.contains("unknown captured build target 'daemon'"),
        "{error}"
    );
    assert!(error.contains("server"), "{error}");

    // An object no captured compilation produced: the build compiled it with a
    // compiler capture never wrapped.
    let error = capture_script(
        &workspace,
        "uncaptured-member",
        &format!(
            "set -eu\nmkdir -p build\ncd build\n{real_clang} -c -O0 ../other.c -o other.o\nclang -c -O0 ../support.c -o support.o\nclang other.o support.o -o mixed\n"
        ),
        "mixed",
    )
    .unwrap_err();
    assert!(
        error.contains("no captured compilation produced"),
        "{error}"
    );
    assert!(error.contains("other.o"), "{error}");

    // Membership reached through a library search is not membership capture
    // observed.
    let error = capture_script(
        &workspace,
        "library-search",
        "set -eu\nmkdir -p build\ncd build\nclang -c -O0 ../other.c -o other.o\nclang other.o -lm -o linked\n",
        "linked",
    )
    .unwrap_err();
    assert!(error.contains("does not support the link"), "{error}");
    assert!(error.contains("-lm"), "{error}");

    // A build that never invoked the wrapped compiler.
    let error = capture_script(
        &workspace,
        "no-compilation",
        "set -eu\nmkdir -p build\necho built > build/marker\n",
        "server",
    )
    .unwrap_err();
    assert!(error.contains("no captured compilation"), "{error}");

    // A failing build publishes nothing.
    let error = capture_script(&workspace, "failing", "set -eu\nexit 3\n", "server").unwrap_err();
    assert!(error.contains("failed"), "{error}");
    assert!(error.contains("nothing was published"), "{error}");

    // A compiler outside the supported Linux/Clang range.
    let project = workspace.project("unsupported-toolchain");
    let mut unsupported = request(
        &project,
        &workspace.0.join("unsupported-toolchain-capture"),
        "server",
    );
    unsupported.compiler = "/bin/true".into();
    let error = Application.capture_build(&unsupported).unwrap_err();
    assert!(error.contains("not Clang"), "{error}");
    assert!(error.contains("Linux with Clang"), "{error}");

    // A capture directory that already holds a capture is never appended to.
    let project = workspace.project("reused");
    let directory = workspace.0.join("reused-capture");
    Application
        .capture_build(&request(&project, &directory, "server"))
        .unwrap();
    let error = Application
        .capture_build(&request(&project, &directory, "server"))
        .unwrap_err();
    assert!(error.contains("already holds a capture"), "{error}");
}

#[test]
fn cli_captures_a_build_and_queries_the_published_target() {
    require_clang!();
    let workspace = Workspace::new();
    let project = workspace.project("cli");
    let output = workspace.0.join("cli-snapshot.json");
    let cli = env!("CARGO_BIN_EXE_gloom");
    let captured = Command::new(cli)
        .arg("capture-build")
        .arg("--project-root")
        .arg(&project)
        .args(["--target", "server", "--snapshot-id", SNAPSHOT_ID])
        .arg("--capture-dir")
        .arg(workspace.0.join("cli-capture"))
        .arg("-o")
        .arg(&output)
        .args(["--", "sh", "build.sh"])
        .output()
        .unwrap();
    assert!(
        captured.status.success(),
        "{}",
        String::from_utf8_lossy(&captured.stderr)
    );
    assert!(
        String::from_utf8_lossy(&captured.stdout).contains("Captured target server"),
        "{}",
        String::from_utf8_lossy(&captured.stdout)
    );

    let queried = Command::new(cli)
        .arg("query-snapshot")
        .arg(&output)
        .args(["--callees", "main"])
        .output()
        .unwrap();
    assert!(queried.status.success());
    let result: Value = serde_json::from_slice(&queried.stdout).unwrap();
    let callees: Vec<&str> = result["relationships"]
        .as_array()
        .unwrap()
        .iter()
        .map(|relationship| relationship["callee_display_name"].as_str().unwrap())
        .collect();
    assert_eq!(callees, ["prepare", "worker"]);
}
