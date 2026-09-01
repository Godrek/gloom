use clap::{Parser, Subcommand};
use gloom::app::{Application, NamedQuery, Query};
use gloom::{LlvmTextContributor, ObservationContext};
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
        #[arg(long, conflicts_with = "explain", required_unless_present = "explain")]
        callees: Option<String>,
        #[arg(long, conflicts_with = "callees", required_unless_present = "callees")]
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
                "Published {} entities and {} call relationships to {}",
                snapshot.program_entities().len(),
                snapshot.call_graph_projection().relationships.len(),
                output.display()
            );
        }
        Commands::QuerySnapshot {
            snapshot,
            callees,
            explain,
        } => {
            let published = application
                .load_snapshot_json(&read(&snapshot)?)
                .map_err(|error| format!("{}: {error}", snapshot.display()))?;
            let value = if let Some(caller_name) = callees {
                serde_json::to_value(
                    application.query_snapshot(&published, NamedQuery::Callees { caller_name })?,
                )
            } else if let Some(handle) = explain {
                let explanation_handle = published
                    .call_graph_projection()
                    .relationships
                    .iter()
                    .find(|relationship| relationship.explanation_handle.as_str() == handle)
                    .map(|relationship| &relationship.explanation_handle)
                    .ok_or_else(|| format!("unknown explanation handle '{handle}'"))?;
                serde_json::to_value(application.explain_snapshot(&published, explanation_handle)?)
            } else {
                unreachable!("clap requires either --callees or --explain")
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
