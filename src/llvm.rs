use crate::contributor::{
    ContributedCallKind, ContributedCallSite, ContributedCallable, ContributedEvidence,
    ContributedInput, ContributedTargetClaim, ContributorIdentity,
    EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability, EvidenceContribution,
    EvidenceContributor, fingerprint_parts,
};
use crate::model::{Graph, Node};
use crate::snapshot::{
    CompletenessBasis, EvidenceScope, EvidenceSupport, ObservationContext, Resolution,
};
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
    pub line: usize,
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
                EvidenceCapability::IndirectCallEvidence,
            ],
        }
    }
}

impl EvidenceContributor for LlvmTextContributor {
    fn identity(&self) -> ContributorIdentity {
        LlvmTextContributor::identity(self)
    }

    fn contribute(
        &self,
        input: &Path,
        context: &ObservationContext,
    ) -> Result<EvidenceContribution, String> {
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
            observation_contexts: vec![context.clone()],
            callables: observations
                .functions
                .into_iter()
                .map(|function| ContributedCallable {
                    contributor_callable_id: function.name.clone(),
                    display_name: function.name,
                    defined: function.defined,
                    representation: "llvm-function".into(),
                    observation_context_id: context.id.clone(),
                    line: function.line,
                    identity_evidence: ContributedEvidence {
                        evidence_type: "static-callable-identity".into(),
                        scope: EvidenceScope::Static,
                        support: EvidenceSupport::ContributorIdentity,
                        completeness_basis: None,
                    },
                })
                .collect(),
            call_sites: observations
                .calls
                .into_iter()
                .map(|call| match call.target {
                    ObservedCallTarget::Direct(callee_display_name) => ContributedCallSite {
                        kind: ContributedCallKind::Direct,
                        caller_callable_id: call.caller,
                        line: call.line,
                        observation_context_id: context.id.clone(),
                        resolution: Resolution::Complete,
                        evidence: ContributedEvidence {
                            evidence_type: "static-call-site".into(),
                            scope: EvidenceScope::Static,
                            support: EvidenceSupport::CallSiteResolution,
                            completeness_basis: Some(CompletenessBasis {
                                boundary: "the call instruction".into(),
                                guarantee:
                                    "a direct call instruction names exactly one callee operand"
                                        .into(),
                            }),
                        },
                        target_claims: vec![ContributedTargetClaim {
                            target_callable_id: callee_display_name.clone(),
                            callee_display_name,
                            target_representation: "llvm-function".into(),
                            observation_context_id: context.id.clone(),
                            evidence: vec![ContributedEvidence {
                                evidence_type: "static-direct-call".into(),
                                scope: EvidenceScope::Static,
                                support: EvidenceSupport::TargetClaim,
                                completeness_basis: None,
                            }],
                        }],
                    },
                    ObservedCallTarget::Indirect => ContributedCallSite {
                        kind: ContributedCallKind::Indirect,
                        caller_callable_id: call.caller,
                        line: call.line,
                        observation_context_id: context.id.clone(),
                        resolution: Resolution::Absent,
                        evidence: ContributedEvidence {
                            evidence_type: "static-indirect-call".into(),
                            scope: EvidenceScope::Static,
                            support: EvidenceSupport::CallSiteResolution,
                            completeness_basis: None,
                        },
                        target_claims: Vec::new(),
                    },
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

#[derive(Clone, Debug, Eq, PartialEq)]
enum LlvmTokenKind {
    Word(String),
    Global(String),
    Local,
    Metadata,
    StringLiteral,
    LeftBrace,
    RightBrace,
    LeftParenthesis,
    RightParenthesis,
    Colon,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LlvmToken {
    kind: LlvmTokenKind,
    line: usize,
}

fn llvm_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'$' | b'.' | b'_')
}

fn quoted_token_end(bytes: &[u8], start: usize, line: &mut usize) -> Result<usize, String> {
    let start_line = *line;
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if index + 1 < bytes.len() => {
                if bytes[index + 1] == b'\n' {
                    *line += 1;
                }
                index += 2;
            }
            b'"' => return Ok(index + 1),
            b'\n' => {
                *line += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    Err(format!(
        "unterminated LLVM quoted token at line {start_line}"
    ))
}

fn tokenize_llvm_ir(text: &str) -> Result<Vec<LlvmToken>, String> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1;
    while index < bytes.len() {
        match bytes[index] {
            b' ' | b'\t' | b'\r' => index += 1,
            b'\n' => {
                line += 1;
                index += 1;
            }
            b';' => {
                while index < bytes.len() && !matches!(bytes[index], b'\n' | b'\r') {
                    index += 1;
                }
            }
            b'@' | b'%' => {
                let token_line = line;
                let global = bytes[index] == b'@';
                index += 1;
                let name = if bytes.get(index) == Some(&b'"') {
                    let end = quoted_token_end(bytes, index, &mut line)?;
                    let name = String::from_utf8_lossy(&bytes[index + 1..end - 1]).into_owned();
                    index = end;
                    name
                } else {
                    let start = index;
                    while index < bytes.len() && llvm_name_byte(bytes[index]) {
                        index += 1;
                    }
                    String::from_utf8_lossy(&bytes[start..index]).into_owned()
                };
                tokens.push(LlvmToken {
                    kind: if global {
                        LlvmTokenKind::Global(name)
                    } else {
                        LlvmTokenKind::Local
                    },
                    line: token_line,
                });
            }
            b'!' => {
                let token_line = line;
                index += 1;
                if bytes.get(index) == Some(&b'"') {
                    index = quoted_token_end(bytes, index, &mut line)?;
                } else {
                    while index < bytes.len() && llvm_name_byte(bytes[index]) {
                        index += 1;
                    }
                }
                tokens.push(LlvmToken {
                    kind: LlvmTokenKind::Metadata,
                    line: token_line,
                });
            }
            b'"' => {
                let token_line = line;
                index = quoted_token_end(bytes, index, &mut line)?;
                tokens.push(LlvmToken {
                    kind: LlvmTokenKind::StringLiteral,
                    line: token_line,
                });
            }
            b'{' => {
                tokens.push(LlvmToken {
                    kind: LlvmTokenKind::LeftBrace,
                    line,
                });
                index += 1;
            }
            b'}' => {
                tokens.push(LlvmToken {
                    kind: LlvmTokenKind::RightBrace,
                    line,
                });
                index += 1;
            }
            b'(' => {
                tokens.push(LlvmToken {
                    kind: LlvmTokenKind::LeftParenthesis,
                    line,
                });
                index += 1;
            }
            b')' => {
                tokens.push(LlvmToken {
                    kind: LlvmTokenKind::RightParenthesis,
                    line,
                });
                index += 1;
            }
            b':' => {
                tokens.push(LlvmToken {
                    kind: LlvmTokenKind::Colon,
                    line,
                });
                index += 1;
            }
            byte if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'$' | b'.' | b'_') => {
                let start = index;
                while index < bytes.len() && llvm_name_byte(bytes[index]) {
                    index += 1;
                }
                tokens.push(LlvmToken {
                    kind: LlvmTokenKind::Word(
                        String::from_utf8_lossy(&bytes[start..index]).into_owned(),
                    ),
                    line,
                });
            }
            _ => index += 1,
        }
    }
    Ok(tokens)
}

fn matching_right_parenthesis(tokens: &[LlvmToken], start: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match &token.kind {
            LlvmTokenKind::LeftParenthesis => depth += 1,
            LlvmTokenKind::RightParenthesis => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn is_callee_operand(tokens: &[LlvmToken], index: usize) -> bool {
    matches!(
        tokens[index].kind,
        LlvmTokenKind::Global(_) | LlvmTokenKind::Local
    ) && tokens
        .get(index + 1)
        .is_some_and(|token| token.kind == LlvmTokenKind::LeftParenthesis)
}

/// Finds the callee operand of a `call` or `invoke` instruction.
///
/// `tokens` must end at the enclosing function body's closing brace, so the
/// search is bounded by the next call opcode or the end of the body rather
/// than by braces: a literal aggregate return type such as
/// `call { i32, i32 } @pair()` legitimately contains `}` before the callee.
///
/// The callee is the last `@global(` or `%local(` operand before the argument
/// list. A named type can also precede a parenthesised list when the
/// instruction spells out its function type, as in `call %Pair (i32, ...)
/// @callee(i32 1)`; that operand is a type, not the callee, so the search
/// continues past its parameter list when another callee-shaped operand
/// follows it.
fn call_target(tokens: &[LlvmToken], call_index: usize) -> Option<ObservedCallTarget> {
    let mut index = call_index + 1;
    while index < tokens.len() {
        match &tokens[index].kind {
            LlvmTokenKind::Word(word) if word == "asm" => return None,
            LlvmTokenKind::Global(_) | LlvmTokenKind::Local if is_callee_operand(tokens, index) => {
                let arguments_end = matching_right_parenthesis(tokens, index + 1);
                if let Some(next) = arguments_end.map(|end| end + 1)
                    && next < tokens.len()
                    && is_callee_operand(tokens, next)
                {
                    index = next;
                    continue;
                }
                return Some(match &tokens[index].kind {
                    LlvmTokenKind::Global(name) => ObservedCallTarget::Direct(name.clone()),
                    _ => ObservedCallTarget::Indirect,
                });
            }
            LlvmTokenKind::Word(word) if word == "call" || word == "invoke" => break,
            _ => index += 1,
        }
    }
    Some(ObservedCallTarget::Indirect)
}

fn function_signature_end(tokens: &[LlvmToken], name_index: usize) -> Option<usize> {
    let start = (name_index + 1..tokens.len())
        .find(|index| tokens[*index].kind == LlvmTokenKind::LeftParenthesis)?;
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match &token.kind {
            LlvmTokenKind::LeftParenthesis => depth += 1,
            LlvmTokenKind::RightParenthesis => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn matching_right_brace(tokens: &[LlvmToken], start: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match &token.kind {
            LlvmTokenKind::LeftBrace => depth += 1,
            LlvmTokenKind::RightBrace => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn brace_group_is_function_body(tokens: &[LlvmToken], start: usize, end: usize) -> bool {
    let mut depth = 1_usize;
    for index in start + 1..end {
        match &tokens[index].kind {
            LlvmTokenKind::LeftBrace => depth += 1,
            LlvmTokenKind::RightBrace => depth -= 1,
            LlvmTokenKind::Word(word)
                if depth == 1
                    && matches!(
                        word.as_str(),
                        "ret"
                            | "br"
                            | "switch"
                            | "indirectbr"
                            | "invoke"
                            | "callbr"
                            | "resume"
                            | "catchswitch"
                            | "catchret"
                            | "cleanupret"
                            | "unreachable"
                    )
                    && !tokens
                        .get(index + 1)
                        .is_some_and(|token| token.kind == LlvmTokenKind::Colon) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

fn function_body_bounds(
    tokens: &[LlvmToken],
    signature_end: usize,
    name: &str,
) -> Result<(usize, usize), String> {
    let mut index = signature_end + 1;
    while index < tokens.len() {
        if tokens[index].kind == LlvmTokenKind::LeftBrace {
            let end = matching_right_brace(tokens, index)
                .ok_or_else(|| format!("LLVM function '{name}' has an incomplete braced value"))?;
            if brace_group_is_function_body(tokens, index, end) {
                return Ok((index, end));
            }
            index = end + 1;
            continue;
        }
        if matches!(&tokens[index].kind, LlvmTokenKind::Word(word) if word == "define" || word == "declare")
        {
            break;
        }
        index += 1;
    }
    Err(format!("LLVM function '{name}' has no body"))
}

fn is_call_opcode(tokens: &[LlvmToken], index: usize) -> bool {
    matches!(&tokens[index].kind, LlvmTokenKind::Word(word) if word == "call" || word == "invoke")
        && !tokens
            .get(index + 1)
            .is_some_and(|token| token.kind == LlvmTokenKind::Colon)
}

fn observe_llvm_ir(text: &str) -> Result<LlvmObservations, String> {
    let tokens = tokenize_llvm_ir(text)?;
    let mut observations = LlvmObservations::default();
    let mut current = None;
    let mut body_end = 0_usize;
    let mut index = 0;
    while index < tokens.len() {
        if current.is_none() {
            let defined = match &tokens[index].kind {
                LlvmTokenKind::Word(word) if word == "define" => true,
                LlvmTokenKind::Word(word) if word == "declare" => false,
                _ => {
                    index += 1;
                    continue;
                }
            };
            let name_index = (index + 1..tokens.len())
                .find(|candidate| matches!(&tokens[*candidate].kind, LlvmTokenKind::Global(_)))
                .ok_or_else(|| {
                    format!(
                        "LLVM function declaration at line {} has no global identity",
                        tokens[index].line
                    )
                })?;
            let LlvmTokenKind::Global(name) = &tokens[name_index].kind else {
                unreachable!()
            };
            observations.functions.push(ObservedFunction {
                name: name.clone(),
                defined,
                line: tokens[index].line,
            });
            let signature_end = function_signature_end(&tokens, name_index).ok_or_else(|| {
                format!("LLVM function '{name}' has an incomplete parameter list")
            })?;
            if !defined {
                index = signature_end + 1;
                continue;
            }
            let (body_index, end) = function_body_bounds(&tokens, signature_end, name)?;
            current = Some(name.clone());
            body_end = end;
            index = body_index + 1;
            continue;
        }

        if index == body_end {
            current = None;
        } else if is_call_opcode(&tokens, index) {
            if let Some(target) = call_target(&tokens[..body_end], index) {
                if !matches!(&target, ObservedCallTarget::Direct(name) if name.starts_with("llvm."))
                {
                    observations.calls.push(ObservedCall {
                        caller: current.clone().expect("function body must have a caller"),
                        target,
                        line: tokens[index].line,
                    });
                }
            }
        }
        index += 1;
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
