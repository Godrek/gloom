//! Incremental declared-build indexing with an atomic, in-process publication boundary.
use crate::acquisition::{self, DeclaredCompilation};
use crate::llvm::ParsedLlvmArtifact;
use crate::{ObservationContext, PublishedSnapshot};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

/// Work completed in the current indexing attempt, separate from query results.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct IndexingProgress {
    pub total: usize,
    pub reused: usize,
    pub analyzed: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IndexingStatus {
    Idle,
    Indexing(IndexingProgress),
    Publishing(IndexingProgress),
    Published(IndexingProgress),
    Failed {
        progress: IndexingProgress,
        error: String,
    },
}

struct CachedCompilation {
    declaration: DeclaredCompilation,
    artifact: Arc<ParsedLlvmArtifact>,
}

#[derive(Default)]
struct Indexer {
    context: Option<ObservationContext>,
    compilations: BTreeMap<String, CachedCompilation>,
    published_ids: BTreeSet<String>,
}

/// A single writer prepares replacements while readers pin immutable snapshots.
///
/// Cloning the returned `Arc` pins a generation for an entire investigation,
/// including explanation expansion. Readers hold no lock while executing queries.
/// The cache lives only in this session; loading a JSON export does not restore it.
/// Separate sessions have independent publication boundaries.
pub struct PublicationSession {
    current: RwLock<Option<Arc<PublishedSnapshot>>>,
    status: RwLock<IndexingStatus>,
    indexer: Mutex<Indexer>,
}

impl Default for PublicationSession {
    fn default() -> Self {
        Self {
            current: RwLock::new(None),
            status: RwLock::new(IndexingStatus::Idle),
            indexer: Mutex::new(Indexer::default()),
        }
    }
}

impl PublicationSession {
    pub fn current(&self) -> Option<Arc<PublishedSnapshot>> {
        self.current
            .read()
            .expect("publication lock poisoned")
            .clone()
    }

    pub fn status(&self) -> IndexingStatus {
        self.status.read().expect("status lock poisoned").clone()
    }

    fn report(&self, status: IndexingStatus, progress: &mut impl FnMut(&IndexingStatus)) {
        *self.status.write().expect("status lock poisoned") = status.clone();
        progress(&status);
    }

    /// Read a fresh declaration and its selected IR artifacts, reuse unchanged
    /// analysis, then publish only after complete snapshot validation succeeds.
    ///
    /// Every attempt must name a snapshot ID not previously published by this
    /// session. Concurrent/reentrant writers are rejected; query readers continue.
    /// The progress callback runs without reader/status locks and may query the
    /// session. It should not panic. Build producers must supply coherent, finished
    /// artifacts, as with one-shot declared-build ingestion.
    pub fn reindex_declared_build(
        &self,
        manifest: &Path,
        target: &str,
        mut progress: impl FnMut(&IndexingStatus),
    ) -> Result<Arc<PublishedSnapshot>, String> {
        let mut indexer = self.indexer.try_lock().map_err(|error| match error {
            std::sync::TryLockError::WouldBlock => "indexing already in progress".to_string(),
            std::sync::TryLockError::Poisoned(_) => {
                "indexing session poisoned by a panic".to_string()
            }
        })?;
        let mut counts = IndexingProgress::default();
        self.report(IndexingStatus::Indexing(counts.clone()), &mut progress);
        let result = (|| {
            let selected = acquisition::select(manifest, target)?;
            let snapshot_id = selected.declaration.program_snapshot_id.clone();
            if indexer.published_ids.contains(&snapshot_id) {
                return Err(format!(
                    "program snapshot '{snapshot_id}' was already published; supply a new snapshot ID"
                ));
            }
            counts.total = selected.declaration.compilations.len();
            self.report(IndexingStatus::Indexing(counts.clone()), &mut progress);
            let reusable = indexer.context.as_ref().is_some_and(|old| {
                old.build_target == selected.context.build_target
                    && old.build_configuration == selected.context.build_configuration
                    && old.toolchain == selected.context.toolchain
                    && old.extraction_method == selected.context.extraction_method
                    && old.extraction_version == selected.context.extraction_version
                    && old.analysis_stage == selected.context.analysis_stage
                    && old.runtime_workload == selected.context.runtime_workload
            });
            let mut next_cache = BTreeMap::new();
            let mut contributions = Vec::new();
            for compilation in &selected.declaration.compilations {
                // Read once: both comparison and parsing use these exact bytes.
                // Neither file timestamps nor the exported fingerprint prove equality.
                let text = std::fs::read_to_string(&compilation.evidence_artifact)
                    .map_err(|error| format!("compilation '{}': {error}", compilation.id))?;
                let cached = indexer.compilations.get(&compilation.id).filter(|cached| {
                    reusable && cached.declaration == *compilation && cached.artifact.matches(&text)
                });
                let artifact = if let Some(cached) = cached {
                    counts.reused += 1;
                    Arc::clone(&cached.artifact)
                } else {
                    let parsed = ParsedLlvmArtifact::parse(text)
                        .map_err(|error| format!("compilation '{}': {error}", compilation.id))?;
                    counts.analyzed += 1;
                    Arc::new(parsed)
                };
                contributions.push(
                    artifact
                        .contribute(Path::new(&compilation.evidence_artifact), &selected.context)?,
                );
                next_cache.insert(
                    compilation.id.clone(),
                    CachedCompilation {
                        declaration: compilation.clone(),
                        artifact,
                    },
                );
                self.report(IndexingStatus::Indexing(counts.clone()), &mut progress);
            }
            let context = selected.context.clone();
            // Rebuild all snapshot-scoped identities, claims and projection indexes.
            // No published state or successful cache is modified if validation fails.
            let snapshot = Arc::new(selected.publish(contributions)?);
            self.report(IndexingStatus::Publishing(counts.clone()), &mut progress);
            *self.current.write().expect("publication lock poisoned") = Some(Arc::clone(&snapshot));
            indexer.context = Some(context);
            indexer.compilations = next_cache;
            indexer.published_ids.insert(snapshot_id);
            Ok(snapshot)
        })();
        match &result {
            Ok(_) => self.report(IndexingStatus::Published(counts), &mut progress),
            Err(error) => self.report(
                IndexingStatus::Failed {
                    progress: counts,
                    error: error.clone(),
                },
                &mut progress,
            ),
        }
        result
    }
}
