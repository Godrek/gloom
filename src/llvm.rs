use crate::contributor::{
    ContributedCallable, ContributedDirectCall, ContributedInput, ContributorIdentity,
    EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability, EvidenceContribution,
    EvidenceContributor, fingerprint_parts,
};
use crate::model::{Graph, Node};
use regex::Regex;
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Debug, Eq, PartialEq)]
enum AcquiredInputKind {
    TextualLlvmIr,
    CCompiledToLlvmIr,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AcquiredLlvmIr {
    pub text: String,
    pub kind: AcquiredInputKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedFunction {
    pub name: String,
    pub defined: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ObservedCallTarget {
    Direct(String),
    Indirect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedCall {
    pub caller: String,
    pub target: ObservedCallTarget,
    pub line: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LlvmObservations {
    pub functions: Vec<ObservedFunction>,
    pub calls: Vec<ObservedCall>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LlvmTextContributor {
    clang: String,
    clang_flags: Vec<String>,
}

impl LlvmTextContributor {
    pub fn new(clang: impl Into<String>, clang_flags: &[String]) -> Self {
        Self {
            clang: clang.into(),
            clang_flags: clang_flags.to_vec(),
        }
    }

    pub fn identity(&self) -> ContributorIdentity {
        ContributorIdentity {
            name: "gloom.llvm-text".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            contract_version: EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION.into(),
            capabilities: vec![
                EvidenceCapability::CallableManifestations,
                EvidenceCapability::DirectCallEvidence,
            ],
        }
    }
}

impl EvidenceContributor for LlvmTextContributor {
    fn identity(&self) -> ContributorIdentity {
        LlvmTextContributor::identity(self)
    }

    fn contribute(&self, input: &Path) -> Result<EvidenceContribution, String> {
        let acquired = acquire_llvm_ir(input, &self.clang, &self.clang_flags)?;
        let observations = observe_llvm_ir(&acquired.text)?;
        let content_fingerprint = fingerprint_parts(&[&acquired.text]);
        Ok(EvidenceContribution {
            input: ContributedInput {
                path: input.display().to_string(),
                evidence_artifact: evidence_artifact(input, &acquired.kind, &content_fingerprint),
                media_type: "application/llvm-ir".into(),
                acquisition_method: match &acquired.kind {
                    AcquiredInputKind::TextualLlvmIr => "declared-artifact".into(),
                    AcquiredInputKind::CCompiledToLlvmIr => "compiled-source".into(),
                },
                content_fingerprint,
            },
            callables: observations
                .functions
                .into_iter()
                .map(|function| ContributedCallable {
                    display_name: function.name,
                    defined: function.defined,
                    representation: "llvm-function".into(),
                })
                .collect(),
            direct_calls: observations
                .calls
                .into_iter()
                .filter_map(|call| match call.target {
                    ObservedCallTarget::Direct(callee_display_name) => {
                        Some(ContributedDirectCall {
                            caller_display_name: call.caller,
                            callee_display_name,
                            target_representation: "llvm-function".into(),
                            line: call.line,
                            evidence_type: "static-direct-call".into(),
                        })
                    }
                    ObservedCallTarget::Indirect => None,
                })
                .collect(),
        })
    }
}

fn evidence_artifact(path: &Path, kind: &AcquiredInputKind, fingerprint: &str) -> String {
    match kind {
        AcquiredInputKind::TextualLlvmIr => path.display().to_string(),
        AcquiredInputKind::CCompiledToLlvmIr => {
            format!("generated LLVM IR {fingerprint} from {}", path.display())
        }
    }
}

fn captured_name(captures: &regex::Captures<'_>, first: usize) -> String {
    captures
        .get(first)
        .or_else(|| captures.get(first + 1))
        .unwrap()
        .as_str()
        .to_owned()
}

fn observe_llvm_ir(text: &str) -> Result<LlvmObservations, String> {
    let symbol = r#"@(?:\"((?:[^\"\\]|\\.)+)\"|([-a-zA-Z$._0-9]+))"#;
    let function = Regex::new(&format!(r"^\s*(define|declare)\b.*?{symbol}\s*\("))
        .map_err(|e| e.to_string())?;
    let call = Regex::new(&format!(r"\b(?:call|invoke)\b[^@\n]*?{symbol}\s*\("))
        .map_err(|e| e.to_string())?;
    let any_call = Regex::new(r"\b(?:call|invoke)\b").map_err(|e| e.to_string())?;
    let mut observations = LlvmObservations::default();
    let mut current: Option<String> = None;
    let mut brace_depth: isize = 0;

    for (line_index, line) in text.lines().enumerate() {
        if let Some(found) = function.captures(line) {
            let name = captured_name(&found, 2);
            let defined = &found[1] == "define";
            observations.functions.push(ObservedFunction {
                name: name.clone(),
                defined,
            });
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
                observations.calls.push(ObservedCall {
                    caller: caller.to_owned(),
                    target: ObservedCallTarget::Direct(callee),
                    line: line_index + 1,
                });
            }
        } else if any_call.is_match(line) && !line.contains(" asm ") {
            observations.calls.push(ObservedCall {
                caller: caller.to_owned(),
                target: ObservedCallTarget::Indirect,
                line: line_index + 1,
            });
        }
        if brace_depth <= 0 && line.contains('}') {
            current = None;
        }
    }
    Ok(observations)
}

pub fn parse_llvm_ir(text: &str, source: Option<&str>) -> Result<Graph, String> {
    let observations = observe_llvm_ir(text)?;
    let mut graph = Graph::default();
    if let Some(source) = source {
        graph.inputs.push(source.into());
    }
    for function in observations.functions {
        graph.add_node(Node::function(
            function.name,
            function.defined,
            source.map(str::to_owned),
        ));
    }
    for call in observations.calls {
        match call.target {
            ObservedCallTarget::Direct(callee) => {
                graph.add_node(Node::function(&callee, false, None));
                graph.add_edge(&call.caller, &callee, "direct-call");
            }
            ObservedCallTarget::Indirect => {
                graph.add_node(Node {
                    id: "<indirect>".into(),
                    label: "indirect call".into(),
                    kind: "unknown".into(),
                    defined: false,
                    language: "llvm".into(),
                    source: None,
                });
                graph.add_edge(&call.caller, "<indirect>", "indirect-call");
            }
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

fn acquire_llvm_ir(path: &Path, clang: &str, flags: &[String]) -> Result<AcquiredLlvmIr, String> {
    let extension = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or_default();
    let (text, kind) = match extension {
        "ll" => (
            fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
            AcquiredInputKind::TextualLlvmIr,
        ),
        "c" | "i" => (
            compile_c(path, clang, flags)?,
            AcquiredInputKind::CCompiledToLlvmIr,
        ),
        _ => {
            return Err(format!(
                "unsupported input '{}'; expected .c, .i, or .ll",
                path.display()
            ));
        }
    };
    Ok(AcquiredLlvmIr { text, kind })
}

pub fn graph_from_path(path: &Path, clang: &str, flags: &[String]) -> Result<Graph, String> {
    let acquired = acquire_llvm_ir(path, clang, flags)?;
    let text = acquired.text;
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

    #[test]
    fn labels_compiled_source_locations_as_generated_llvm_ir() {
        assert_eq!(
            evidence_artifact(
                Path::new("fixture.c"),
                &AcquiredInputKind::CCompiledToLlvmIr,
                "fnv1a64:0123456789abcdef",
            ),
            "generated LLVM IR fnv1a64:0123456789abcdef from fixture.c"
        );
    }
}
