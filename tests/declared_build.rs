use gloom::app::{Application, NamedQuery};
use gloom::{CallableSelector, PublishedSnapshot};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/declared-build/manifest.json")
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "gloom-declared-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn manifest(&self, mutate: impl FnOnce(&mut Value)) -> PathBuf {
        let mut value: Value =
            serde_json::from_str(&std::fs::read_to_string(fixture()).unwrap()).unwrap();
        for compilation in value["compilations"].as_array_mut().unwrap() {
            compilation["working_directory"] = json!(fixture().parent().unwrap().join("build"));
        }
        mutate(&mut value);
        let path = self.0.join("manifest.json");
        std::fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();
        path
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn names(snapshot: &PublishedSnapshot) -> Vec<String> {
    Application
        .query_snapshot(
            snapshot,
            NamedQuery::CallableSearch {
                label: String::new(),
            },
        )
        .unwrap()
        .callable_search()
        .unwrap()
        .callables
        .iter()
        .map(|item| item.display_name.clone())
        .collect()
}

#[test]
fn selects_membership_before_publishing_and_retains_build_evidence() {
    let targets = Application.declared_build_targets(&fixture()).unwrap();
    assert_eq!(
        targets
            .iter()
            .map(|target| target.name.as_str())
            .collect::<Vec<_>>(),
        ["server", "other-tool"]
    );
    let server = Application
        .publish_declared_build(&fixture(), "server")
        .unwrap();
    let other = Application
        .publish_declared_build(&fixture(), "other-tool")
        .unwrap();
    let mut server_names = names(&server);
    server_names.sort();
    assert_eq!(server_names, ["helper", "server", "worker"]);
    let mut other_names = names(&other);
    other_names.sort();
    assert_eq!(other_names, ["helper", "other_tool"]);
    assert!(
        Application
            .query_snapshot(
                &server,
                NamedQuery::Callees {
                    caller: CallableSelector::by_label("other_tool")
                }
            )
            .is_err()
    );
    let context = &server.observation_contexts()[0];
    assert_eq!(context.build_target, "server");
    assert_eq!(context.build_configuration, "debug; FEATURE=1");
    assert_eq!(context.toolchain, "Clang 18.1.8 / LLVM 18");
    assert_ne!(context.id, other.observation_contexts()[0].id);
    let result = Application
        .query_snapshot(
            &server,
            NamedQuery::CallPath {
                start: CallableSelector::by_label("server"),
                end: CallableSelector::by_label("helper"),
                max_relationships: 2,
            },
        )
        .unwrap();
    let path = result.call_path().unwrap().path.as_ref().unwrap();
    assert_eq!(path.len(), 2);
    let explanation = Application
        .explain_snapshot(&server, &path[1].explanation_handle)
        .unwrap();
    let acquisition = server.declared_build().unwrap();
    let generated_index = acquisition
        .declaration
        .compilations
        .iter()
        .position(|compilation| compilation.id == "generated-worker")
        .unwrap();
    assert!(
        explanation
            .evidence_records
            .iter()
            .any(|record| record.acquired_input_id
                == acquisition.acquired_input_ids[generated_index])
    );
    assert!(
        acquisition.declaration.compilations[generated_index]
            .source_input
            .ends_with("build/generated/worker.c")
    );
    assert!(
        acquisition.declaration.compilations[0].generated_inputs[0]
            .path
            .ends_with("build/generated/config.h")
    );
    assert_eq!(
        acquisition.declaration.compilations[0].compiler_arguments[5],
        "-Igenerated"
    );
    let exported = Application.export_snapshot_json(&server).unwrap();
    assert_eq!(Application.load_snapshot_json(&exported).unwrap(), server);
}

#[test]
fn rejects_acquisition_gaps_and_ambiguous_membership() {
    for (field, value) in [
        ("targets", json!([])),
        ("toolchain", json!("")),
        ("build_configuration", json!("")),
        ("schema_version", json!("99")),
    ] {
        let temp = Temp::new();
        let path = temp.manifest(|manifest| manifest[field] = value);
        assert!(
            Application.publish_declared_build(&path, "server").is_err(),
            "{field}"
        );
    }
    for (field, value, diagnostic) in [
        ("compiler_arguments", json!([]), "compiler_arguments"),
        ("working_directory", json!(""), "working_directory"),
        ("source_input", json!(""), "source_input"),
        ("evidence_artifact", json!("missing.ll"), "server-main"),
        ("evidence_artifact", json!("../main.c"), "existing .ll"),
    ] {
        let temp = Temp::new();
        let path = temp.manifest(|manifest| manifest["compilations"][0][field] = value);
        let error = Application
            .publish_declared_build(&path, "server")
            .unwrap_err();
        assert!(error.contains(diagnostic), "{error}");
    }
    for members in [
        json!([]),
        json!(["unknown"]),
        json!(["server-main", "server-main"]),
    ] {
        let temp = Temp::new();
        let path = temp.manifest(|manifest| manifest["targets"][0]["compilation_ids"] = members);
        assert!(Application.publish_declared_build(&path, "server").is_err());
    }
    let temp = Temp::new();
    let path = temp.manifest(|manifest| {
        manifest["compilations"][0]
            .as_object_mut()
            .unwrap()
            .remove("generated_inputs");
    });
    assert!(
        Application
            .publish_declared_build(&path, "server")
            .unwrap_err()
            .contains("generated_inputs")
    );
    assert!(
        Application
            .publish_declared_build(&fixture(), "unknown")
            .unwrap_err()
            .contains("unknown declared build target")
    );
}

#[test]
fn ingestion_reads_only_selected_artifacts_and_never_executes_declared_commands() {
    let temp = Temp::new();
    let path = temp.manifest(|manifest| {
        manifest["compilations"][0]["compiler_arguments"] =
            json!(["/does/not/exist/compiler", "--network-is-unavailable"]);
        manifest["compilations"][2]["evidence_artifact"] = json!("unavailable-other-target.ll");
    });
    assert_eq!(
        Application
            .publish_declared_build(&path, "server")
            .unwrap()
            .acquired_inputs()
            .len(),
        2
    );
    assert!(
        Application
            .publish_declared_build(&path, "other-tool")
            .is_err()
    );
}

#[test]
fn exported_acquisition_cannot_disagree_with_published_membership() {
    let snapshot = Application
        .publish_declared_build(&fixture(), "server")
        .unwrap();
    let value = serde_json::to_value(snapshot).unwrap();
    for field in ["toolchain", "build_configuration", "program_snapshot_id"] {
        let mut corrupted = value.clone();
        corrupted["declared_build"]["declaration"][field] = json!("another");
        assert!(
            Application
                .load_snapshot_json(&corrupted.to_string())
                .is_err()
        );
    }
    let mut corrupted = value.clone();
    corrupted["declared_build"]["acquired_input_ids"][0] = json!("unknown");
    assert!(
        Application
            .load_snapshot_json(&corrupted.to_string())
            .is_err()
    );
    let mut corrupted = value;
    corrupted["declared_build"]["declaration"]["compilations"][0]["evidence_artifact"] =
        json!("other.ll");
    assert!(
        Application
            .load_snapshot_json(&corrupted.to_string())
            .is_err()
    );
}

#[test]
fn cli_ingests_from_an_unrelated_directory_and_queries_the_selected_target() {
    let temp = Temp::new();
    let output = temp.0.join("snapshot.json");
    let cli = env!("CARGO_BIN_EXE_gloom");
    let listed = Command::new(cli)
        .current_dir(&temp.0)
        .arg("build-targets")
        .arg(fixture())
        .output()
        .unwrap();
    assert!(listed.status.success());
    let targets: Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(targets[0]["name"], "server");
    let ingested = Command::new(cli)
        .current_dir(&temp.0)
        .arg("ingest-build")
        .arg(fixture())
        .args(["--target", "server", "-o"])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        ingested.status.success(),
        "{}",
        String::from_utf8_lossy(&ingested.stderr)
    );
    let queried = Command::new(cli)
        .arg("query-snapshot")
        .arg(output)
        .args(["--callees", "server"])
        .output()
        .unwrap();
    assert!(queried.status.success());
    let result: Value = serde_json::from_slice(&queried.stdout).unwrap();
    assert_eq!(result["relationships"].as_array().unwrap().len(), 1);
}
