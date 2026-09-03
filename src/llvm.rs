use crate::contributor::{
    ContributedCallKind, ContributedCallSite, ContributedCallable, ContributedEvidence,
    ContributedInput, ContributedTargetClaim, ContributorIdentity,
    EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability, EvidenceContribution,
    EvidenceContributor, fingerprint_parts,
};
use crate::model::{Graph, Node};
use crate::snapshot::{EvidenceScope, EvidenceSupport, ObservationContext, Resolution};
use std::collections::{BTreeMap, BTreeSet};
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

/// How a callable global is written in the module, kept as the representation
/// of the manifestation contributed for it.
const LLVM_FUNCTION: &str = "llvm-function";
const LLVM_ALIAS: &str = "llvm-alias";
const LLVM_IFUNC: &str = "llvm-ifunc";

/// A callee named by a call site, with the kind of callable global it
/// resolved to.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedCallee {
    pub name: String,
    pub representation: &'static str,
}

/// What a call site's textual evidence says about its target.
///
/// `Direct` is reserved for a callee operand that resolves to a callable the
/// module declares, defines, or aliases. Everything else is `Indirect`: the
/// evidence names no callable, so the call site's targets are unresolved.
#[derive(Clone, Debug, Eq, PartialEq)]
enum ObservedCallTarget {
    Direct(ObservedCallee),
    Indirect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedCall {
    pub caller: String,
    pub target: ObservedCallTarget,
    pub line: usize,
}

/// A call site whose callee operand has been parsed but not yet resolved
/// against the module's declared globals.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingCall {
    pub caller: String,
    pub callee: CalleeOperand,
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
                    representation: LLVM_FUNCTION.into(),
                    observation_context_id: context.id.clone(),
                })
                .collect(),
            call_sites: observations
                .calls
                .into_iter()
                .map(|call| match call.target {
                    ObservedCallTarget::Direct(callee) => ContributedCallSite {
                        kind: ContributedCallKind::Direct,
                        caller_callable_id: call.caller,
                        line: call.line,
                        observation_context_id: context.id.clone(),
                        resolution: Resolution::Complete,
                        evidence: ContributedEvidence {
                            evidence_type: "static-call-site".into(),
                            scope: EvidenceScope::Static,
                            support: EvidenceSupport::CallSiteResolution,
                        },
                        target_claims: vec![ContributedTargetClaim {
                            target_callable_id: callee.name.clone(),
                            callee_display_name: callee.name,
                            target_representation: callee.representation.into(),
                            observation_context_id: context.id.clone(),
                            evidence: vec![ContributedEvidence {
                                evidence_type: "static-direct-call".into(),
                                scope: EvidenceScope::Static,
                                support: EvidenceSupport::TargetClaim,
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
    /// The `=` of a module-scope definition or a local assignment. It marks
    /// where one module-scope definition ends and the next begins.
    Equals,
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
            b'=' => {
                tokens.push(LlvmToken {
                    kind: LlvmTokenKind::Equals,
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

fn matching_left_parenthesis(tokens: &[LlvmToken], end: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for index in (0..=end).rev() {
        match &tokens[index].kind {
            LlvmTokenKind::RightParenthesis => depth += 1,
            LlvmTokenKind::LeftParenthesis => {
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

/// A constant cast that preserves the identity of the value it wraps, so the
/// callee it names can still be recovered. `inttoptr` and `ptrtoint` are
/// excluded: they convert an address, and recovering a callable from one would
/// require reasoning Gloom's textual evidence does not support.
fn is_identity_preserving_cast(word: &str) -> bool {
    matches!(word, "bitcast" | "addrspacecast")
}

fn is_cast_expression(tokens: &[LlvmToken], index: usize) -> bool {
    matches!(&tokens[index].kind, LlvmTokenKind::Word(word) if is_identity_preserving_cast(word))
        && tokens
            .get(index + 1)
            .is_some_and(|token| token.kind == LlvmTokenKind::LeftParenthesis)
}

/// Recognises a constant cast expression used as the callee operand, as in
/// `call void bitcast (void (...)* @f to void ()*)()`: the parenthesised cast
/// is immediately followed by the argument list.
fn is_cast_callee_operand(tokens: &[LlvmToken], index: usize) -> bool {
    is_cast_expression(tokens, index)
        && matching_right_parenthesis(tokens, index + 1)
            .and_then(|end| tokens.get(end + 1))
            .is_some_and(|token| token.kind == LlvmTokenKind::LeftParenthesis)
}

/// Finds the `to` keyword that separates a constant cast's value operand from
/// its destination type. Nested casts carry their own parentheses, so only the
/// keyword at the cast's own nesting depth belongs to it.
fn cast_conversion_keyword(tokens: &[LlvmToken], open: usize, close: usize) -> Option<usize> {
    let mut depth = 0_usize;
    for (index, token) in tokens.iter().enumerate().take(close).skip(open) {
        match &token.kind {
            LlvmTokenKind::LeftParenthesis => depth += 1,
            LlvmTokenKind::RightParenthesis => depth -= 1,
            LlvmTokenKind::Word(word) if depth == 1 && word == "to" => return Some(index),
            _ => {}
        }
    }
    None
}

/// The name of the callee operand of a `call` or `invoke` instruction, as the
/// instruction spells it. Whether the name identifies a callable is a separate
/// question, answered by resolving it against the module's callable globals.
#[derive(Clone, Debug, Eq, PartialEq)]
enum CalleeOperand {
    /// A global name, reached directly or through identity-preserving casts.
    Global(String),
    /// An operand that names no global, such as a register or a computed
    /// address.
    Unnamed,
}

/// Strips identity-preserving constant casts from the callee operand starting
/// at `index` to reach the value they wrap.
///
/// A cast operand is written `<type> <value> to <type>`, so the value is the
/// token before the cast's own `to`. That value may be another cast, which is
/// unwrapped in turn.
fn cast_callee_operand(tokens: &[LlvmToken], index: usize) -> CalleeOperand {
    let mut cast = index;
    loop {
        if !is_cast_expression(tokens, cast) {
            return CalleeOperand::Unnamed;
        }
        let Some(close) = matching_right_parenthesis(tokens, cast + 1) else {
            return CalleeOperand::Unnamed;
        };
        let Some(keyword) = cast_conversion_keyword(tokens, cast + 1, close) else {
            return CalleeOperand::Unnamed;
        };
        let Some(value) = keyword.checked_sub(1).filter(|value| *value > cast + 1) else {
            return CalleeOperand::Unnamed;
        };
        match &tokens[value].kind {
            LlvmTokenKind::Global(name) => return CalleeOperand::Global(name.clone()),
            LlvmTokenKind::RightParenthesis => {
                let Some(nested) = matching_left_parenthesis(tokens, value)
                    .and_then(|open| open.checked_sub(1))
                    .filter(|nested| *nested > cast)
                else {
                    return CalleeOperand::Unnamed;
                };
                cast = nested;
            }
            _ => return CalleeOperand::Unnamed,
        }
    }
}

/// Finds the callee operand of a `call` or `invoke` instruction.
///
/// `tokens` must end at the enclosing function body's closing brace, so the
/// search is bounded by the next call opcode or the end of the body rather
/// than by braces: a literal aggregate return type such as
/// `call { i32, i32 } @pair()` legitimately contains `}` before the callee.
///
/// The callee is the last operand before the argument list: `@global(`,
/// `%local(`, or a constant cast wrapping either. A named type can also
/// precede a parenthesised list when the instruction spells out its function
/// type, as in `call %Pair (i32, ...) @callee(i32 1)`; that operand is a type,
/// not the callee, so the search continues past its parameter list when
/// another callee operand follows it.
fn call_callee_operand(tokens: &[LlvmToken], call_index: usize) -> Option<CalleeOperand> {
    let mut index = call_index + 1;
    while index < tokens.len() {
        match &tokens[index].kind {
            LlvmTokenKind::Word(word) if word == "asm" => return None,
            LlvmTokenKind::Word(_) if is_cast_callee_operand(tokens, index) => {
                return Some(cast_callee_operand(tokens, index));
            }
            LlvmTokenKind::Global(_) | LlvmTokenKind::Local if is_callee_operand(tokens, index) => {
                let arguments_end = matching_right_parenthesis(tokens, index + 1);
                if let Some(next) = arguments_end.map(|end| end + 1)
                    && next < tokens.len()
                    && (is_callee_operand(tokens, next) || is_cast_callee_operand(tokens, next))
                {
                    index = next;
                    continue;
                }
                return Some(match &tokens[index].kind {
                    LlvmTokenKind::Global(name) => CalleeOperand::Global(name.clone()),
                    _ => CalleeOperand::Unnamed,
                });
            }
            LlvmTokenKind::Word(word) if word == "call" || word == "invoke" => break,
            _ => index += 1,
        }
    }
    Some(CalleeOperand::Unnamed)
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

/// A module-scope global, kept by the kind that decides whether calling it
/// names a callable.
///
/// Global variables are absent by design: an operand that names one, directly
/// or through an alias, names no callable.
#[derive(Clone, Debug, Eq, PartialEq)]
enum DeclaredGlobal {
    /// A `define` or `declare`.
    Function,
    /// An `alias`, which is callable only when its aliasee is.
    Alias { aliasee: CalleeOperand },
    /// An `ifunc`, whose resolver supplies the callee at load time.
    IFunc,
}

/// Reports the alias or ifunc a module-scope definition introduces, as in
/// `@aliased = alias void (), ptr @aliasee`.
///
/// Aliases and ifuncs are globals that no `define` or `declare` introduces, so
/// they are collected separately. Requiring the `@name =` of a definition, and
/// reading the keyword only from the words that follow it, keeps unrelated
/// globals and their initialisers out.
fn declared_alias(tokens: &[LlvmToken], index: usize) -> Option<(&str, DeclaredGlobal)> {
    let LlvmTokenKind::Global(name) = &tokens[index].kind else {
        return None;
    };
    if tokens.get(index + 1)?.kind != LlvmTokenKind::Equals {
        return None;
    }
    let (offset, keyword) = tokens[index + 2..]
        .iter()
        .enumerate()
        .take_while(|(_, token)| matches!(&token.kind, LlvmTokenKind::Word(_)))
        .find(|(_, token)| {
            matches!(&token.kind, LlvmTokenKind::Word(word) if word == "alias" || word == "ifunc")
        })?;
    let declaration = match &keyword.kind {
        LlvmTokenKind::Word(word) if word == "ifunc" => DeclaredGlobal::IFunc,
        _ => DeclaredGlobal::Alias {
            aliasee: alias_aliasee(tokens, index + 2 + offset),
        },
    };
    Some((name.as_str(), declaration))
}

/// Finds where the module-scope definition containing `start` ends.
///
/// Every module-scope definition but `define` and `declare` is introduced by a
/// name and an `=`, so the next such pair, or the next function, bounds the
/// current one. Definitions are bounded by tokens rather than by lines: an
/// alias may be written across several lines.
fn module_definition_end(tokens: &[LlvmToken], start: usize) -> usize {
    (start..tokens.len())
        .find(|index| {
            matches!(&tokens[*index].kind, LlvmTokenKind::Word(word) if word == "define" || word == "declare")
                || tokens
                    .get(index + 1)
                    .is_some_and(|token| token.kind == LlvmTokenKind::Equals)
        })
        .unwrap_or(tokens.len())
}

/// The aliasee of an alias definition, parsed from the tokens between the
/// `alias` keyword and the end of the definition.
///
/// Only two shapes name a callable: a bare global, and an identity-preserving
/// cast wrapping one. The aliasee is the definition's last operand, so it is
/// read from the end. Every other constant expression — `select`,
/// `getelementptr`, `inttoptr`, and the rest — fails closed as `Unnamed`:
/// deciding which of a `select`'s operands the alias resolves to is reasoning
/// this evidence does not support, and guessing one would publish a target
/// claim the module does not make.
fn alias_aliasee(tokens: &[LlvmToken], keyword: usize) -> CalleeOperand {
    let Some(last) = module_definition_end(tokens, keyword + 1)
        .checked_sub(1)
        .filter(|last| *last > keyword)
    else {
        return CalleeOperand::Unnamed;
    };
    match &tokens[last].kind {
        LlvmTokenKind::Global(name) => CalleeOperand::Global(name.clone()),
        LlvmTokenKind::RightParenthesis => matching_left_parenthesis(tokens, last)
            .and_then(|open| open.checked_sub(1))
            .filter(|head| *head > keyword)
            .map_or(CalleeOperand::Unnamed, |head| {
                cast_callee_operand(tokens, head)
            }),
        _ => CalleeOperand::Unnamed,
    }
}

/// Classifies a parsed callee operand against the module's declared globals.
///
/// A global names a direct target only when it identifies a callable: a
/// function, an ifunc, or an alias whose chain of aliasees reaches one. A call
/// through a global variable, such as `@fp = external global ptr` followed by
/// `call void @fp()`, and a call through an alias to data are both valid IR
/// that names no callable, so they stay indirect call sites rather than
/// becoming claimed targets named after the global.
fn resolve_callee(
    callee: CalleeOperand,
    declarations: &BTreeMap<String, DeclaredGlobal>,
) -> ObservedCallTarget {
    match callee {
        CalleeOperand::Global(name) => match callable_representation(&name, declarations) {
            Some(representation) => ObservedCallTarget::Direct(ObservedCallee {
                name,
                representation,
            }),
            None => ObservedCallTarget::Indirect,
        },
        CalleeOperand::Unnamed => ObservedCallTarget::Indirect,
    }
}

/// Reports how a callable global named at a call site is written, or `None`
/// when the name identifies no callable.
///
/// The representation describes the named global itself, while the alias chain
/// is followed only to decide whether it is callable at all. A chain that
/// reaches a global the module never declares, an aliasee the parse could not
/// name, or a cycle is not callable.
fn callable_representation(
    name: &str,
    declarations: &BTreeMap<String, DeclaredGlobal>,
) -> Option<&'static str> {
    let representation = match declarations.get(name)? {
        DeclaredGlobal::Function => LLVM_FUNCTION,
        DeclaredGlobal::Alias { .. } => LLVM_ALIAS,
        DeclaredGlobal::IFunc => LLVM_IFUNC,
    };
    let mut visited = BTreeSet::new();
    let mut current = name.to_owned();
    loop {
        if !visited.insert(current.clone()) {
            return None;
        }
        match declarations.get(&current)? {
            DeclaredGlobal::Function | DeclaredGlobal::IFunc => return Some(representation),
            DeclaredGlobal::Alias {
                aliasee: CalleeOperand::Global(aliasee),
            } => current = aliasee.clone(),
            DeclaredGlobal::Alias {
                aliasee: CalleeOperand::Unnamed,
            } => return None,
        }
    }
}

fn observe_llvm_ir(text: &str) -> Result<LlvmObservations, String> {
    let tokens = tokenize_llvm_ir(text)?;
    let mut observations = LlvmObservations::default();
    let mut declarations: BTreeMap<String, DeclaredGlobal> = BTreeMap::new();
    let mut pending_calls: Vec<PendingCall> = Vec::new();
    let mut current = None;
    let mut body_end = 0_usize;
    let mut index = 0;
    while index < tokens.len() {
        if current.is_none() {
            if let Some((name, declaration)) = declared_alias(&tokens, index) {
                declarations.insert(name.to_owned(), declaration);
                index += 1;
                continue;
            }
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
            declarations.insert(name.clone(), DeclaredGlobal::Function);
            observations.functions.push(ObservedFunction {
                name: name.clone(),
                defined,
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
        } else if is_call_opcode(&tokens, index)
            && let Some(callee) = call_callee_operand(&tokens[..body_end], index)
        {
            pending_calls.push(PendingCall {
                caller: current.clone().expect("function body must have a caller"),
                callee,
                line: tokens[index].line,
            });
        }
        index += 1;
    }

    // Callee operands are resolved after the whole module has been observed:
    // textual LLVM IR may declare a called function, or the aliasee an alias
    // points at, after the call site.
    for call in pending_calls {
        let target = resolve_callee(call.callee, &declarations);
        if matches!(&target, ObservedCallTarget::Direct(callee) if callee.name.starts_with("llvm."))
        {
            continue;
        }
        observations.calls.push(ObservedCall {
            caller: call.caller,
            target,
            line: call.line,
        });
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
                graph.add_node(Node::function(&callee.name, false, None));
                graph.add_edge(&call.caller, &callee.name, "direct-call");
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

    /// Aliases forming a cycle are rejected by LLVM itself, so this text is
    /// deliberately beyond what a real module can contain: the parse must stay
    /// conservative on hand-written input rather than loop.
    const CALLEE_OPERAND_IR: &str = r#"
@handler = external global ptr
@data = global i8 0
@data_alias = alias i8, ptr @data
@function_alias = alias void (), ptr @declared_target
@select_alias = alias void (), ptr select (i1 false, ptr @declared_target, ptr @data)
@split_alias = alias void (),
    ptr @declared_target
@cycle = alias void (), ptr @other_cycle
@other_cycle = alias void (), ptr @cycle
define void @caller() {
  call void @handler()
  call void bitcast (void (...)* @declared_target to void ()*)()
  call void @data_alias()
  call void @function_alias()
  call void @select_alias()
  call void @split_alias()
  call void @cycle()
  ret void
}
declare void @declared_target()"#;

    #[test]
    fn resolves_callee_operands_against_the_module_declarations() {
        let graph = parse_llvm_ir(CALLEE_OPERAND_IR, Some("fixture.ll")).unwrap();
        for uncallable in [
            "handler",
            "data",
            "data_alias",
            "select_alias",
            "cycle",
            "other_cycle",
        ] {
            assert!(!graph.nodes.contains_key(uncallable));
        }
        assert!(graph.edges.contains_key(&(
            "caller".into(),
            "<indirect>".into(),
            "indirect-call".into()
        )));
        for callable in ["declared_target", "function_alias", "split_alias"] {
            assert!(graph.edges.contains_key(&(
                "caller".into(),
                callable.into(),
                "direct-call".into()
            )));
        }
        assert_eq!(
            graph.edges[&("caller".into(), "<indirect>".into(), "indirect-call".into())].call_count,
            4
        );
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
