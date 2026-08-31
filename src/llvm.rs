use crate::model::{Graph, Node};
use regex::Regex;
use std::fs;
use std::path::Path;
use std::process::Command;

fn captured_name(captures: &regex::Captures<'_>, first: usize) -> String {
    captures
        .get(first)
        .or_else(|| captures.get(first + 1))
        .unwrap()
        .as_str()
        .to_owned()
}

pub fn parse_llvm_ir(text: &str, source: Option<&str>) -> Result<Graph, String> {
    let symbol = r#"@(?:\"((?:[^\"\\]|\\.)+)\"|([-a-zA-Z$._0-9]+))"#;
    let function = Regex::new(&format!(r"^\s*(define|declare)\b.*?{symbol}\s*\("))
        .map_err(|e| e.to_string())?;
    let call = Regex::new(&format!(r"\b(?:call|invoke)\b[^@\n]*?{symbol}\s*\("))
        .map_err(|e| e.to_string())?;
    let any_call = Regex::new(r"\b(?:call|invoke)\b").map_err(|e| e.to_string())?;
    let mut graph = Graph::default();
    if let Some(source) = source {
        graph.inputs.push(source.into());
    }
    let mut current: Option<String> = None;
    let mut brace_depth: isize = 0;

    for line in text.lines() {
        if let Some(found) = function.captures(line) {
            let name = captured_name(&found, 2);
            let defined = &found[1] == "define";
            graph.add_node(Node::function(&name, defined, source.map(str::to_owned)));
            if defined {
                current = Some(name);
                brace_depth =
                    line.matches('{').count() as isize - line.matches('}').count() as isize;
            }
            continue;
        }
        let Some(caller) = current.as_deref() else {
            continue;
        };
        brace_depth += line.matches('{').count() as isize - line.matches('}').count() as isize;
        if let Some(found) = call.captures(line) {
            let callee = captured_name(&found, 1);
            if !callee.starts_with("llvm.") {
                let caller = caller.to_owned();
                graph.add_node(Node::function(&callee, false, None));
                graph.add_edge(&caller, &callee, "direct-call");
            }
        } else if any_call.is_match(line) && !line.contains(" asm ") {
            let caller = caller.to_owned();
            graph.add_node(Node {
                id: "<indirect>".into(),
                label: "indirect call".into(),
                kind: "unknown".into(),
                defined: false,
                language: "llvm".into(),
                source: None,
            });
            graph.add_edge(&caller, "<indirect>", "indirect-call");
        }
        if brace_depth <= 0 && line.contains('}') {
            current = None;
        }
    }
    Ok(graph)
}

fn compile_c(path: &Path, clang: &str, flags: &[String]) -> Result<String, String> {
    let unique = format!(
        "gloom-{}-{}.ll",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let output = std::env::temp_dir().join(unique);
    let result = Command::new(clang)
        .args(["-S", "-emit-llvm", "-g", "-O0", "-fno-discard-value-names"])
        .args(flags).arg(path).arg("-o").arg(&output).output()
        .map_err(|error| if error.kind() == std::io::ErrorKind::NotFound { format!("'{clang}' was not found; install Clang, pass --clang PATH, or provide a .ll file") } else { error.to_string() })?;
    if !result.status.success() {
        return Err(format!(
            "Clang failed for {}:\n{}",
            path.display(),
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    let text = fs::read_to_string(&output).map_err(|e| e.to_string());
    let _ = fs::remove_file(output);
    text
}

pub fn graph_from_path(path: &Path, clang: &str, flags: &[String]) -> Result<Graph, String> {
    let extension = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or_default();
    let text = match extension {
        "ll" => fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
        "c" | "i" => compile_c(path, clang, flags)?,
        _ => {
            return Err(format!(
                "unsupported input '{}'; expected .c, .i, or .ll",
                path.display()
            ));
        }
    };
    parse_llvm_ir(&text, Some(&path.display().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const IR: &str = r#"
declare i32 @puts(ptr)
define i32 @main() {
  %x = call i32 @worker(i32 2)
  %y = call i32 @worker(i32 3)
  call void %callback()
  ret i32 0
}
define i32 @worker(i32 %n) {
  %x = call i32 @"odd.name"(i32 %n)
  ret i32 %x
}
define i32 @"odd.name"(i32 %n) {
  %x = call i32 @worker(i32 %n)
  %y = call i32 @puts(ptr null)
  ret i32 %x
}"#;

    #[test]
    fn extracts_and_coalesces_calls() {
        let graph = parse_llvm_ir(IR, Some("fixture.ll")).unwrap();
        assert!(graph.nodes["main"].defined);
        assert!(!graph.nodes["puts"].defined);
        assert_eq!(
            graph.edges[&("main".into(), "worker".into(), "direct-call".into())].call_count,
            2
        );
        assert!(graph.edges.contains_key(&(
            "main".into(),
            "<indirect>".into(),
            "indirect-call".into()
        )));
    }
}
