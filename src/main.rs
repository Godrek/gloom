use clap::{Parser, Subcommand};
use gloom::app::{Application, NamedQuery, Query};
use gloom::{LlvmTextContributor, ObservationContext, ProgramEntityId, PublishedSnapshot};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "gloom",
    version,
    about = "Build and explore LLVM-oriented program graphs"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a graph from C or textual LLVM IR.
    Build {
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        #[arg(short, long, default_value = "graph.json")]
        output: PathBuf,
        #[arg(long)]
        html: Option<PathBuf>,
        #[arg(long, default_value = "clang")]
        clang: String,
        #[arg(long = "clang-flag", allow_hyphen_values = true)]
        clang_flags: Vec<String>,
    },
    /// Render graph JSON as a self-contained HTML viewer.
    View {
        graph: PathBuf,
        #[arg(short, long, default_value = "graph.html")]
        output: PathBuf,
    },
    /// Query an existing graph.
    Analyze {
        graph: PathBuf,
        #[arg(long, conflicts_with_all = ["reachable", "path"])]
        cycles: bool,
        #[arg(long, conflicts_with = "path")]
        reachable: Option<String>,
        #[arg(long, num_args = 2, value_names = ["FROM", "TO"])]
        path: Option<Vec<String>>,
    },
    /// Publish an evidence-backed program snapshot from C or textual LLVM IR.
    Publish {
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        #[arg(short, long, default_value = "snapshot.json")]
        output: PathBuf,
        #[arg(long)]
        html: Option<PathBuf>,
        #[arg(long)]
        snapshot_id: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        build_configuration: String,
        #[arg(long)]
        toolchain: String,
        #[arg(long)]
        analysis_stage: String,
        #[arg(long, default_value = "clang")]
        clang: String,
        #[arg(long = "clang-flag", allow_hyphen_values = true)]
        clang_flags: Vec<String>,
    },
    /// Run a named query or expand an explanation from a published snapshot.
    QuerySnapshot {
        snapshot: PathBuf,
        /// Find callable entities whose display name contains LABEL, with the
        /// acquired input and declaration that tell same-named callables apart.
        #[arg(
            long,
            value_name = "LABEL",
            conflicts_with_all = ["callees", "callers", "call_path", "explain"],
            required_unless_present_any = ["callees", "callers", "call_path", "explain"]
        )]
        search_callables: Option<String>,
        #[arg(
            long,
            conflicts_with_all = ["callers", "call_path", "explain", "search_callables"],
            required_unless_present_any = ["callers", "call_path", "explain", "search_callables"]
        )]
        callees: Option<String>,
        #[arg(
            long,
            requires = "callees",
            conflicts_with_all = ["callers", "call_path", "explain", "search_callables"]
        )]
        caller_entity_id: Option<String>,
        #[arg(
            long,
            conflicts_with_all = ["callees", "call_path", "explain", "search_callables"],
            required_unless_present_any = ["callees", "call_path", "explain", "search_callables"]
        )]
        callers: Option<String>,
        #[arg(
            long,
            requires = "callers",
            conflicts_with_all = ["callees", "call_path", "explain", "search_callables"]
        )]
        callee_entity_id: Option<String>,
        #[arg(
            long = "call-path",
            num_args = 2,
            value_names = ["FROM", "TO"],
            conflicts_with_all = ["callees", "callers", "explain", "search_callables"],
            required_unless_present_any = ["callees", "callers", "explain", "search_callables"]
        )]
        call_path: Option<Vec<String>>,
        #[arg(long, requires = "call_path")]
        start_entity_id: Option<String>,
        #[arg(long, requires = "call_path")]
        end_entity_id: Option<String>,
        #[arg(long, requires = "call_path")]
        max_relationships: Option<usize>,
        #[arg(
            long,
            conflicts_with_all = ["callees", "callers", "call_path", "search_callables"],
            required_unless_present_any = ["callees", "callers", "call_path", "search_callables"]
        )]
        explain: Option<String>,
    },
    /// Render a published snapshot as a self-contained evidence viewer.
    ViewSnapshot {
        snapshot: PathBuf,
        #[arg(short, long, default_value = "snapshot.html")]
        output: PathBuf,
    },
}

fn read(path: &Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(text)
}

fn write(path: &Path, text: &str) -> Result<(), String> {
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn snapshot_entity_id(
    snapshot: &PublishedSnapshot,
    requested_id: Option<String>,
    role: &str,
) -> Result<Option<ProgramEntityId>, String> {
    requested_id
        .map(|requested_id| {
            snapshot
                .program_entities()
                .iter()
                .find(|entity| entity.id.as_str() == requested_id)
                .map(|entity| entity.id.clone())
                .ok_or_else(|| format!("unknown {role} entity '{requested_id}'"))
        })
        .transpose()
}

fn run() -> Result<(), String> {
    let application = Application;
    match Cli::parse().command {
        Commands::Build {
            inputs,
            output,
            html,
            clang,
            clang_flags,
        } => {
            let document = application.build(&inputs, &clang, &clang_flags)?;
            write(&output, &application.export_json(&document)?)?;
            if let Some(path) = html {
                write(&path, &application.render_viewer(&document)?)?;
            }
            println!(
                "Wrote {} nodes and {} edges to {}",
                document.nodes.len(),
                document.edges.len(),
                output.display()
            );
        }
        Commands::View { graph, output } => {
            let document = application
                .load_json(&read(&graph)?)
                .map_err(|error| format!("{}: {error}", graph.display()))?;
            write(&output, &application.render_viewer(&document)?)?;
            println!("Wrote {}", output.display());
        }
        Commands::Analyze {
            graph,
            cycles,
            reachable,
            path,
        } => {
            let document = application
                .load_json(&read(&graph)?)
                .map_err(|error| format!("{}: {error}", graph.display()))?;
            let query = if cycles {
                Query::PotentialRecursiveCycles
            } else if let Some(start) = reachable {
                Query::Reachable { start }
            } else if let Some(points) = path {
                Query::ShortestPath {
                    start: points[0].clone(),
                    end: points[1].clone(),
                }
            } else {
                Query::Summary
            };
            let value = application.query(document, query)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
            );
        }
        Commands::Publish {
            inputs,
            output,
            html,
            snapshot_id,
            target,
            build_configuration,
            toolchain,
            analysis_stage,
            clang,
            clang_flags,
        } => {
            let contributor = LlvmTextContributor::new(&clang, &clang_flags);
            let contributor_identity = contributor.identity();
            let context = ObservationContext::static_analysis(
                snapshot_id,
                target,
                build_configuration,
                toolchain,
                contributor_identity.name,
                contributor_identity.version,
                analysis_stage,
            );
            let snapshot = application.publish_snapshot(&inputs, context, &contributor)?;
            write(&output, &application.export_snapshot_json(&snapshot)?)?;
            if let Some(path) = html {
                write(&path, &application.render_snapshot_viewer(&snapshot)?)?;
            }
            println!(
                "Published {} entities and {} call sites to {}",
                snapshot.program_entities().len(),
                snapshot.call_graph_projection().call_sites.len(),
                output.display()
            );
        }
        Commands::QuerySnapshot {
            snapshot,
            search_callables,
            callees,
            caller_entity_id,
            callers,
            callee_entity_id,
            call_path,
            start_entity_id,
            end_entity_id,
            max_relationships,
            explain,
        } => {
            let published = application
                .load_snapshot_json(&read(&snapshot)?)
                .map_err(|error| format!("{}: {error}", snapshot.display()))?;
            let value = if let Some(label) = search_callables {
                serde_json::to_value(application.query_snapshot(
                    &published,
                    NamedQuery::CallableSearch { label },
                )?)
            } else if let Some(caller_name) = callees {
                let caller_entity_id =
                    snapshot_entity_id(&published, caller_entity_id, "caller")?;
                serde_json::to_value(application.query_snapshot(
                    &published,
                    NamedQuery::Callees {
                        caller_name,
                        caller_entity_id,
                    },
                )?)
            } else if let Some(callee_name) = callers {
                let callee_entity_id =
                    snapshot_entity_id(&published, callee_entity_id, "callee")?;
                serde_json::to_value(application.query_snapshot(
                    &published,
                    NamedQuery::Callers {
                        callee_name,
                        callee_entity_id,
                    },
                )?)
            } else if let Some(points) = call_path {
                let start_entity_id =
                    snapshot_entity_id(&published, start_entity_id, "start")?;
                let end_entity_id = snapshot_entity_id(&published, end_entity_id, "end")?;
                serde_json::to_value(application.query_snapshot(
                    &published,
                    NamedQuery::CallPath {
                        start_name: points[0].clone(),
                        start_entity_id,
                        end_name: points[1].clone(),
                        end_entity_id,
                        max_relationships: max_relationships.unwrap_or(8),
                    },
                )?)
            } else if let Some(handle) = explain {
                let explanation_handle = published
                    .call_graph_projection()
                    .call_sites
                    .iter()
                    .find(|call_site| call_site.explanation_handle.as_str() == handle)
                    .map(|call_site| &call_site.explanation_handle)
                    .ok_or_else(|| format!("unknown explanation handle '{handle}'"))?;
                serde_json::to_value(application.explain_snapshot(&published, explanation_handle)?)
            } else {
                unreachable!(
                    "clap requires --search-callables, --callees, --callers, --call-path, or --explain"
                )
            }
            .map_err(|error| error.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?
            );
        }
        Commands::ViewSnapshot { snapshot, output } => {
            let published = application
                .load_snapshot_json(&read(&snapshot)?)
                .map_err(|error| format!("{}: {error}", snapshot.display()))?;
            write(&output, &application.render_snapshot_viewer(&published)?)?;
            println!("Wrote {}", output.display());
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("gloom: error: {error}");
        std::process::exit(2);
    }
}
