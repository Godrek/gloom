mod analysis;
mod llvm;
mod model;
mod viewer;

use clap::{Parser, Subcommand};
use model::{Document, Graph};
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
}

fn load(path: &Path) -> Result<Document, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let document: Document =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if document.schema_version != "1.0" {
        return Err(format!(
            "unsupported graph schema {:?}",
            document.schema_version
        ));
    }
    Ok(document)
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    text.push('\n');
    fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

fn run() -> Result<(), String> {
    match Cli::parse().command {
        Commands::Build {
            inputs,
            output,
            html,
            clang,
            clang_flags,
        } => {
            let mut graph = Graph::default();
            for input in inputs {
                graph.merge(llvm::graph_from_path(&input, &clang, &clang_flags)?);
            }
            let document = Document::from_graph(&graph, analysis::summary(&graph));
            write_json(&output, &document)?;
            if let Some(path) = html {
                fs::write(&path, viewer::render_html(&document)?)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
            }
            println!(
                "Wrote {} nodes and {} edges to {}",
                graph.nodes.len(),
                graph.edges.len(),
                output.display()
            );
        }
        Commands::View { graph, output } => {
            let document = load(&graph)?;
            fs::write(&output, viewer::render_html(&document)?)
                .map_err(|e| format!("{}: {e}", output.display()))?;
            println!("Wrote {}", output.display());
        }
        Commands::Analyze {
            graph,
            cycles,
            reachable,
            path,
        } => {
            let graph = load(&graph)?.into_graph();
            let value = if cycles {
                serde_json::to_value(analysis::cycles(&graph))
            } else if let Some(start) = reachable {
                serde_json::to_value(analysis::reachable(&graph, &start)?)
            } else if let Some(points) = path {
                serde_json::to_value(analysis::shortest_path(&graph, &points[0], &points[1])?)
            } else {
                serde_json::to_value(analysis::summary(&graph))
            }
            .map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
            );
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
