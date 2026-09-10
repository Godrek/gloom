//! Run `cargo run --example incremental -- server manifest-v1.json manifest-v2.json`.
use gloom::app::{Application, NamedQuery};
use std::path::Path;

fn main() -> Result<(), String> {
    let mut arguments = std::env::args().skip(1);
    let target = arguments.next().ok_or("expected TARGET MANIFEST...")?;
    let manifests: Vec<_> = arguments.collect();
    if manifests.is_empty() {
        return Err("expected TARGET MANIFEST...".into());
    }
    let session = Application.publication_session();
    // Retain handles to demonstrate that later publications do not change them.
    let mut generations = Vec::new();
    for manifest in manifests {
        generations.push(session.reindex_declared_build(
            Path::new(&manifest),
            &target,
            |status| {
                eprintln!("{status:?}");
            },
        )?);
    }
    for snapshot in generations {
        let result = Application.query_snapshot(
            &snapshot,
            NamedQuery::CallableSearch {
                label: String::new(),
            },
        )?;
        println!(
            "{}",
            serde_json::to_string(&result).map_err(|error| error.to_string())?
        );
    }
    Ok(())
}
