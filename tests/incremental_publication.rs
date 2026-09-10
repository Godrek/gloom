use gloom::app::{Application, NamedQuery};
use gloom::publication::{IndexingProgress, IndexingStatus};
use gloom::{CallableSelector, PublishedSnapshot};
use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};

struct Build {
    directory: PathBuf,
    declaration: Value,
}

impl Build {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "gloom-incremental-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("main.ll"),
            "declare void @worker()\ndefine void @main() {\n call void @worker()\n ret void\n}\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("worker.ll"),
            "define void @worker() {\n ret void\n}\n",
        )
        .unwrap();
        let compilations: Vec<_> = ["main", "worker"]
            .iter()
            .map(|name| {
                json!({
                    "id": name, "working_directory": ".", "source_input": format!("{name}.c"),
                    "compiler_arguments": ["clang", "-S", "-emit-llvm", format!("{name}.c")],
                    "evidence_artifact": format!("{name}.ll"), "generated_inputs": []
                })
            })
            .collect();
        Self {
            directory,
            declaration: json!({
                "schema_version": "1", "program_snapshot_id": "one",
                "build_configuration": "debug", "toolchain": "Clang 18.1.8 / LLVM 18",
                "analysis_stage": "per-translation-unit LLVM IR before linking",
                "compilations": compilations,
                "targets": [{"name": "server", "compilation_ids": ["main", "worker"]}]
            }),
        }
    }
    fn manifest(&mut self, id: &str) -> PathBuf {
        self.declaration["program_snapshot_id"] = json!(id);
        let path = self.directory.join("manifest.json");
        std::fs::write(&path, serde_json::to_vec(&self.declaration).unwrap()).unwrap();
        path
    }
    fn change_worker(&self) {
        std::fs::write(
            self.directory.join("worker.ll"),
            "declare void @helper()\ndefine void @worker() {\n call void @helper()\n ret void\n}\n",
        )
        .unwrap();
    }
}
impl Drop for Build {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

fn has_helper(snapshot: &PublishedSnapshot) -> bool {
    !Application
        .query_snapshot(
            snapshot,
            NamedQuery::CallableSearch {
                label: "helper".into(),
            },
        )
        .unwrap()
        .callable_search()
        .unwrap()
        .callables
        .is_empty()
}

fn explain_main(snapshot: &PublishedSnapshot) {
    let result = Application
        .query_snapshot(
            snapshot,
            NamedQuery::Callees {
                caller: CallableSelector::by_label("main"),
            },
        )
        .unwrap();
    let relationships = &result.call_relationships().unwrap().relationships;
    assert_eq!(relationships.len(), 1);
    let explanation = Application
        .explain_snapshot(snapshot, &relationships[0].explanation_handle)
        .unwrap();
    assert!(!explanation.evidence_records.is_empty());
    let roundtrip = Application
        .load_snapshot_json(&Application.export_snapshot_json(snapshot).unwrap())
        .unwrap();
    assert_eq!(&roundtrip, snapshot);
}

#[test]
fn readers_pin_old_generation_until_complete_replacement_is_published() {
    let mut build = Build::new();
    let session = Arc::new(Application.publication_session());
    assert!(session.current().is_none());
    assert_eq!(session.status(), IndexingStatus::Idle);
    let old = session
        .reindex_declared_build(&build.manifest("one"), "server", |_| {})
        .unwrap();
    let old_json = Application.export_snapshot_json(&old).unwrap();
    assert!(!has_helper(&old));
    build.change_worker();
    let manifest = build.manifest("two");
    let (ready_tx, ready_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let writer_session = Arc::clone(&session);
    let writer = std::thread::spawn(move || {
        writer_session
            .reindex_declared_build(&manifest, "server", |status| {
                if matches!(status, IndexingStatus::Publishing(_)) {
                    ready_tx.send(()).unwrap();
                    continue_rx.recv().unwrap();
                }
            })
            .unwrap()
    });
    ready_rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap();
    assert_eq!(
        session.status(),
        IndexingStatus::Publishing(IndexingProgress {
            total: 2,
            reused: 1,
            analyzed: 1
        })
    );
    for _ in 0..20 {
        let pinned = session.current().unwrap();
        assert!(Arc::ptr_eq(&pinned, &old));
        assert!(!has_helper(&pinned));
        explain_main(&pinned);
    }
    assert!(
        session
            .reindex_declared_build(&build.manifest("three"), "server", |_| {})
            .unwrap_err()
            .contains("already in progress")
    );
    continue_tx.send(()).unwrap();
    let new = writer.join().unwrap();
    assert!(Arc::ptr_eq(&new, &session.current().unwrap()));
    assert!(has_helper(&new));
    assert_eq!(new.program_snapshot().id.as_str(), "two");
    assert_ne!(
        old.observation_contexts()[0].id,
        new.observation_contexts()[0].id
    );
    assert_eq!(Application.export_snapshot_json(&old).unwrap(), old_json);
    explain_main(&old);
    explain_main(&new);
    assert_eq!(
        session.status(),
        IndexingStatus::Published(IndexingProgress {
            total: 2,
            reused: 1,
            analyzed: 1
        })
    );
}

#[test]
fn failures_preserve_current_and_successful_cache_and_ids_cannot_be_republished() {
    let mut build = Build::new();
    let session = Application.publication_session();
    let old = session
        .reindex_declared_build(&build.manifest("one"), "server", |_| {})
        .unwrap();
    build.change_worker();
    // Failing after some acquisition must not commit a partial cache or snapshot.
    build.declaration["targets"][0]["compilation_ids"] = json!(["worker", "main"]);
    std::fs::remove_file(build.directory.join("main.ll")).unwrap();
    assert!(
        session
            .reindex_declared_build(&build.manifest("two"), "server", |_| {})
            .is_err()
    );
    assert!(matches!(session.status(), IndexingStatus::Failed { .. }));
    assert!(Arc::ptr_eq(&old, &session.current().unwrap()));
    explain_main(&old);
    std::fs::write(
        build.directory.join("main.ll"),
        "declare void @worker()\ndefine void @main() {\n call void @worker()\n ret void\n}\n",
    )
    .unwrap();
    session
        .reindex_declared_build(&build.manifest("two"), "server", |_| {})
        .unwrap();
    assert_eq!(
        session.status(),
        IndexingStatus::Published(IndexingProgress {
            total: 2,
            reused: 1,
            analyzed: 1
        })
    );
    assert!(
        session
            .reindex_declared_build(&build.manifest("one"), "server", |_| {})
            .unwrap_err()
            .contains("already published")
    );
    assert_eq!(
        session.current().unwrap().program_snapshot().id.as_str(),
        "two"
    );
}

#[test]
fn unchanged_bytes_reuse_analysis_but_changed_declarations_invalidate_it() {
    let mut build = Build::new();
    let session = Application.publication_session();
    session
        .reindex_declared_build(&build.manifest("one"), "server", |_| {})
        .unwrap();
    session
        .reindex_declared_build(&build.manifest("two"), "server", |_| {})
        .unwrap();
    assert_eq!(
        session.status(),
        IndexingStatus::Published(IndexingProgress {
            total: 2,
            reused: 2,
            analyzed: 0
        })
    );
    build.declaration["compilations"][0]["compiler_arguments"] = json!(["clang", "-O2", "main.c"]);
    session
        .reindex_declared_build(&build.manifest("three"), "server", |_| {})
        .unwrap();
    assert_eq!(
        session.status(),
        IndexingStatus::Published(IndexingProgress {
            total: 2,
            reused: 1,
            analyzed: 1
        })
    );
    build.declaration["toolchain"] = json!("Clang 19 / LLVM 19");
    session
        .reindex_declared_build(&build.manifest("four"), "server", |_| {})
        .unwrap();
    assert_eq!(
        session.status(),
        IndexingStatus::Published(IndexingProgress {
            total: 2,
            reused: 0,
            analyzed: 2
        })
    );
    // Target membership is rebuilt; removed entities cannot survive in an index.
    build.declaration["targets"][0]["compilation_ids"] = json!(["worker"]);
    let reduced = session
        .reindex_declared_build(&build.manifest("five"), "server", |_| {})
        .unwrap();
    assert_eq!(reduced.acquired_inputs().len(), 1);
    assert!(
        Application
            .query_snapshot(
                &reduced,
                NamedQuery::Callees {
                    caller: CallableSelector::by_label("main")
                }
            )
            .is_err()
    );
}

#[test]
fn progress_callback_can_read_and_failures_before_initial_publication_are_separate() {
    let mut build = Build::new();
    let session = Application.publication_session();
    assert!(
        session
            .reindex_declared_build(&build.manifest("one"), "missing", |_| {})
            .is_err()
    );
    assert!(session.current().is_none());
    assert!(matches!(session.status(), IndexingStatus::Failed { .. }));
    let mut statuses = Vec::new();
    session
        .reindex_declared_build(&build.manifest("one"), "server", |status| {
            assert_eq!(&session.status(), status);
            if matches!(status, IndexingStatus::Published(_)) {
                assert!(session.current().is_some());
            } else {
                assert!(session.current().is_none());
            }
            statuses.push(status.clone());
        })
        .unwrap();
    assert!(matches!(
        statuses.last(),
        Some(IndexingStatus::Published(_))
    ));
}
