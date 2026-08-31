use clap::{Parser, Subcommand};
use gloom::app::{Application, Query};
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
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("gloom: error: {error}");
        std::process::exit(2);
    }
}
