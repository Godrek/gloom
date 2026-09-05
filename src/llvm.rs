use crate::contributor::{
    ContributedCallKind, ContributedCallSite, ContributedCallable, ContributedEvidence,
    ContributedEvidenceLocation, ContributedInput, ContributedTargetClaim, ContributorCallSiteId,
    ContributorIdentity, EVIDENCE_CONTRIBUTOR_CONTRACT_VERSION, EvidenceCapability,
    EvidenceContribution, EvidenceContributor, LLVM_ALIAS_REPRESENTATION,
    LLVM_FUNCTION_REPRESENTATION, LLVM_IFUNC_REPRESENTATION, STATIC_DIRECT_CALL_EVIDENCE_TYPE,
    fingerprint_parts,
};
use crate::model::{Graph, Node};
use crate::snapshot::{
    CallableIdentityScope, CompletenessBasis, ContributorCallableIdentity, EvidenceScope,
    EvidenceSupport, ObservationContext, Resolution,
};
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

/// A callable global the module declares: a `define`, a `declare`, or an
/// alias or ifunc whose chain reaches one. Every one of them is contributed as
/// a callable manifestation with its own contributor-identity evidence, read
/// at the line that declares it, so a direct target claim naming it rests on
/// the declaration rather than introducing the callable itself.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedCallable {
    pub name: LlvmGlobal,
    pub defined: bool,
    pub line: usize,
    pub representation: &'static str,
    pub identity_scope: CallableIdentityScope,
}

/// A module-scope global identifier, as LLVM itself distinguishes them.
///
/// Two spellings that reach the linker as one symbol must reach Gloom as one
/// identity, and two that reach it as different symbols must not be collapsed.
/// Both cases were verified against the assembler:
///
/// - `@foo` and `@"\66oo"` emit the single symbol `foo`, because LLVM decodes
///   a quoted name's `\XX` hex escapes before the symbol is emitted. A
///   backslash not followed by two hex digits stays literal, and `\22` is how
///   a quote is written inside a name, so a backslash never ends the token.
/// - `@0` and `@"0"` emit two symbols, `__unnamed_1` and `0`: an unquoted
///   all-digit global is an *unnamed* value numbered by its slot, while the
///   quoted form is a *named* global whose name happens to be a digit.
///
/// Identity is therefore derived from this decoded, discriminated form rather
/// than from the source spelling.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum LlvmGlobal {
    Named(Vec<u8>),
    Unnamed(u64),
}

impl LlvmGlobal {
    /// Reads a global's identifier from the bytes between its sigil and its
    /// end, given whether the source quoted it.
    fn parse(text: &str, quoted: bool) -> Self {
        if !quoted && !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()) {
            if let Ok(slot) = text.parse::<u64>() {
                return Self::Unnamed(slot);
            }
        }
        if quoted {
            Self::Named(decode_llvm_escapes(text))
        } else {
            Self::Named(text.as_bytes().to_vec())
        }
    }

    /// The label a person reads. Two different globals may share one, which is
    /// exactly why it is not the identity.
    fn display_name(&self) -> String {
        match self {
            Self::Named(name) => String::from_utf8_lossy(name).into_owned(),
            Self::Unnamed(slot) => slot.to_string(),
        }
    }

    /// The two parts a contributor callable identity is built from: a tag
    /// distinguishing a named global from an unnamed slot, and the text that
    /// names it. The tag lives in the identity's prefix rather than inside it,
    /// so a global genuinely named `unnamed:0` can never collide with slot 0.
    fn identity_parts(&self) -> (&'static str, String) {
        match self {
            Self::Named(name) => {
                // Encode bytes outside the ordinary identifier alphabet,
                // including the escape marker itself. Identity stays lossless
                // and cannot acquire trailing whitespace from a quoted name.
                let mut encoded = String::new();
                for &byte in name {
                    if byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b'$') {
                        encoded.push(char::from(byte));
                    } else {
                        use std::fmt::Write;
                        write!(&mut encoded, "%{byte:02X}")
                            .expect("writing an identity into a String cannot fail");
                    }
                }
                ("", encoded)
            }
            Self::Unnamed(slot) => ("-unnamed", slot.to_string()),
        }
    }

    fn is_intrinsic(&self) -> bool {
        matches!(self, Self::Named(name) if name.starts_with(b"llvm."))
    }
}

/// Decodes the `\XX` hex escapes LLVM allows inside a quoted identifier. A
/// backslash that is not followed by two hex digits is a literal backslash.
/// LLVM identifiers can contain non-UTF-8 bytes, which must remain distinct.
fn decode_llvm_escapes(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = (bytes[index] == b'\\')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .filter(|pair| pair.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match escape {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    decoded
}

/// LLVM's local linkage types.
///
/// Per the LangRef a `private` or `internal` global is visible only inside its
/// own module: `private` is not even emitted into the symbol table, and
/// `internal` becomes a local symbol the linker never joins. So two modules may
/// each write one under the same name and they remain two callables. Every
/// other function linkage — `external`, `weak`, `linkonce`,
/// `available_externally`, their `_odr` forms, and `extern_weak` — leaves the
/// symbol visible to the link.
fn is_local_linkage(word: &str) -> bool {
    matches!(word, "private" | "internal")
}

/// The scope a module-scope definition's linkage keywords put it in.
///
/// The keywords sit between the introducer (`define`, `declare`, or the `=` of
/// an alias or ifunc) and the name or keyword that ends the prefix, so only
/// that span is read: `internal` is neither a type name nor a parameter
/// attribute, and nothing else in the prefix spells it.
fn declared_identity_scope(
    tokens: &[LlvmToken],
    start: usize,
    end: usize,
) -> CallableIdentityScope {
    if tokens[start..end]
        .iter()
        .any(|token| matches!(&token.kind, LlvmTokenKind::Word(word) if is_local_linkage(word)))
    {
        CallableIdentityScope::AcquiredInput
    } else {
        CallableIdentityScope::LinkageNamespace
    }
}

/// The identity this contributor asserts for a callable.
///
/// A display name is a label, so it never serves as the identity on its own. A
/// callable the link can see is identified by the symbol the link joins it by,
/// so the same identity in two acquired inputs names one callable. A callable
/// private to its module is identified within the acquired input it was read
/// from, named by the content fingerprint this contribution declares for that
/// input, so an identically spelled local callable in another input is a
/// different identity. Two acquisitions of the same module text share a
/// fingerprint and are, as #23 established for call-site identities, genuinely
/// indistinguishable to this contributor.
fn contributor_callable_identity(
    global: &LlvmGlobal,
    identity_scope: CallableIdentityScope,
    content_fingerprint: &str,
) -> ContributorCallableIdentity {
    let (kind, text) = global.identity_parts();
    let id = match identity_scope {
        CallableIdentityScope::LinkageNamespace => format!("llvm-symbol{kind}:{text}"),
        CallableIdentityScope::AcquiredInput => {
            format!("llvm-module-local{kind}:{content_fingerprint}:{text}")
        }
    };
    ContributorCallableIdentity::new(id, identity_scope)
        .expect("generated contributor callable identity must be well formed")
}

/// How a callable global is written in the module, kept as the representation
/// of the manifestation contributed for it. The contributor contract owns the
/// vocabulary, since it is what a direct target claim is checked against when
/// a snapshot is published or read back.
const LLVM_FUNCTION: &str = LLVM_FUNCTION_REPRESENTATION;
const LLVM_ALIAS: &str = LLVM_ALIAS_REPRESENTATION;
const LLVM_IFUNC: &str = LLVM_IFUNC_REPRESENTATION;

/// A callee named by a call site, with the kind of callable global it
/// resolved to.
#[derive(Clone, Debug, Eq, PartialEq)]
struct ObservedCallee {
    pub name: LlvmGlobal,
    pub representation: &'static str,
    pub identity_scope: CallableIdentityScope,
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
    pub caller: LlvmGlobal,
    pub caller_identity_scope: CallableIdentityScope,
    pub target: ObservedCallTarget,
    pub line: usize,
}

/// A call site whose callee operand has been parsed but not yet resolved
/// against the module's declared globals.
#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingCall {
    pub caller: LlvmGlobal,
    pub callee: CalleeOperand,
    pub line: usize,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct LlvmObservations {
    pub callables: Vec<ObservedCallable>,
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
        let artifact = evidence_artifact(input, &acquired.kind, &content_fingerprint);
        Ok(EvidenceContribution {
            input: ContributedInput {
                path: input.display().to_string(),
                evidence_artifact: artifact.clone(),
                media_type: "application/llvm-ir".into(),
                acquisition_method: match &acquired.kind {
                    AcquiredInputKind::TextualLlvmIr => "declared-artifact".into(),
                    AcquiredInputKind::CCompiledToLlvmIr => "compiled-source".into(),
                },
                content_fingerprint: content_fingerprint.clone(),
            },
            observation_contexts: vec![context.clone()],
            callables: observations
                .callables
                .into_iter()
                .map(|callable| ContributedCallable {
                    callable_identity: contributor_callable_identity(
                        &callable.name,
                        callable.identity_scope,
                        &content_fingerprint,
                    ),
                    display_name: callable.name.display_name(),
                    defined: callable.defined,
                    representation: callable.representation.into(),
                    observation_context_id: context.id.clone(),
                    line: callable.line,
                    identity_evidence: ContributedEvidence {
                        evidence_type: "static-callable-identity".into(),
                        scope: EvidenceScope::Static,
                        support: EvidenceSupport::ContributorIdentity,
                        completeness_basis: None,
                        location: ContributedEvidenceLocation {
                            evidence_artifact: artifact.clone(),
                            line: callable.line,
                        },
                    },
                })
                .collect(),
            call_sites: observations
                .calls
                .into_iter()
                .enumerate()
                .map(|(call_index, call)| {
                    // This contributor identifies a call site by the index of
                    // its call instruction within the acquired artifact rather
                    // than by `<caller>:<line>`: textual LLVM IR does not
                    // guarantee one call instruction per line, so a line-based
                    // identity would not be unique in the artifact, and the
                    // artifact itself is pinned by its content fingerprint.
                    let contributor_call_site_id =
                        ContributorCallSiteId::new(format!("llvm-call:{call_index}"))
                            .expect("generated call-site identity must be well formed");
                    let location = ContributedEvidenceLocation {
                        evidence_artifact: artifact.clone(),
                        line: call.line,
                    };
                    match call.target {
                        ObservedCallTarget::Direct(callee) => ContributedCallSite {
                            contributor_call_site_id,
                            kind: ContributedCallKind::Direct,
                            caller_callable_identity: contributor_callable_identity(
                                &call.caller,
                                call.caller_identity_scope,
                                &content_fingerprint,
                            ),
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
                                location: location.clone(),
                            },
                            target_claims: vec![ContributedTargetClaim {
                                target_callable_identity: contributor_callable_identity(
                                    &callee.name,
                                    callee.identity_scope,
                                    &content_fingerprint,
                                ),
                                callee_display_name: callee.name.display_name(),
                                target_representation: callee.representation.into(),
                                observation_context_id: context.id.clone(),
                                evidence: vec![ContributedEvidence {
                                    evidence_type: STATIC_DIRECT_CALL_EVIDENCE_TYPE.into(),
                                    scope: EvidenceScope::Static,
                                    support: EvidenceSupport::TargetClaim,
                                    completeness_basis: None,
                                    location,
                                }],
                            }],
                        },
                        ObservedCallTarget::Indirect => ContributedCallSite {
                            contributor_call_site_id,
                            kind: ContributedCallKind::Indirect,
                            caller_callable_identity: contributor_callable_identity(
                                &call.caller,
                                call.caller_identity_scope,
                                &content_fingerprint,
                            ),
                            line: call.line,
                            observation_context_id: context.id.clone(),
                            resolution: Resolution::Absent,
                            evidence: ContributedEvidence {
                                evidence_type: "static-indirect-call".into(),
                                scope: EvidenceScope::Static,
                                support: EvidenceSupport::CallSiteResolution,
                                completeness_basis: None,
                                location,
                            },
                            target_claims: Vec::new(),
                        },
                    }
                })
                .collect(),
            call_site_attachments: Vec::new(),
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
    Global(LlvmGlobal),
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
    /// The `,` that separates a definition's operands, such as an alias's type
    /// from its aliasee.
    Comma,
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
            // A quoted LLVM identifier ends at the next `"`. A backslash never
            // escapes it: `\22` is how a quote is written inside a name, as the
            // assembler confirms.
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
                let quoted = bytes.get(index) == Some(&b'"');
                let text = if quoted {
                    let end = quoted_token_end(bytes, index, &mut line)?;
                    let text = String::from_utf8_lossy(&bytes[index + 1..end - 1]).into_owned();
                    index = end;
                    text
                } else {
                    let start = index;
                    while index < bytes.len() && llvm_name_byte(bytes[index]) {
                        index += 1;
                    }
                    String::from_utf8_lossy(&bytes[start..index]).into_owned()
                };
                tokens.push(LlvmToken {
                    kind: if global {
                        LlvmTokenKind::Global(LlvmGlobal::parse(&text, quoted))
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
            b',' => {
                tokens.push(LlvmToken {
                    kind: LlvmTokenKind::Comma,
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

/// A named wrapper that yields the function it wraps. Per the LangRef,
/// `dso_local_equivalent @f` is a function equivalent to `@f` — a dso-local
/// stub that jumps to it — and `no_cfi @f` is `@f`'s address without CFI
/// checks. A call through either invokes `@f`.
fn is_identity_preserving_wrapper(word: &str) -> bool {
    matches!(word, "dso_local_equivalent" | "no_cfi")
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
    Global(LlvmGlobal),
    /// An operand that names no global, such as a register or a computed
    /// address.
    Unnamed,
}

/// The global an operand names, or `Unnamed` when it names none.
///
/// Exactly four operand shapes name a global:
///
/// - a bare global, `@f`;
/// - `bitcast (<type> <operand> to <type>)` and `addrspacecast (...)`, which
///   preserve the identity of the operand they wrap;
/// - `dso_local_equivalent <operand>`, a function equivalent to the one it
///   wraps;
/// - `no_cfi <operand>`, the wrapped function's address without CFI checks.
///
/// Every other head — `select`, `getelementptr`, `inttoptr`, `ptrtoint`,
/// `blockaddress`, any other keyword or expression — fails closed, with no
/// scan past it for a global inside: which global such an expression yields is
/// reasoning this evidence does not support, and guessing one would publish a
/// target claim the module does not make.
/// In particular, `ptrauth` signs a pointer; it does not yield its raw function
/// address. Resolving a call through it would require authentication semantics
/// (including the call's key and discriminator), which this extractor does not
/// model. See <https://llvm.org/docs/PointerAuth.html#operand-bundle>.
fn operand_global(tokens: &[LlvmToken], index: usize) -> CalleeOperand {
    let mut index = index;
    loop {
        let Some(token) = tokens.get(index) else {
            return CalleeOperand::Unnamed;
        };
        match &token.kind {
            LlvmTokenKind::Global(name) => return CalleeOperand::Global(name.clone()),
            LlvmTokenKind::Word(word) if is_identity_preserving_wrapper(word) => index += 1,
            LlvmTokenKind::Word(_) if is_cast_expression(tokens, index) => {
                let Some(value) = cast_value_operand(tokens, index) else {
                    return CalleeOperand::Unnamed;
                };
                index = value;
            }
            _ => return CalleeOperand::Unnamed,
        }
    }
}

/// Where the value operand of the constant cast at `index` begins.
///
/// A cast is written `<type> <value> to <type>`, so the value ends just before
/// the cast's own `to`. A parenthesised value is another expression, whose own
/// head begins just before its opening parenthesis.
fn cast_value_operand(tokens: &[LlvmToken], index: usize) -> Option<usize> {
    let close = matching_right_parenthesis(tokens, index + 1)?;
    let keyword = cast_conversion_keyword(tokens, index + 1, close)?;
    let value = keyword.checked_sub(1).filter(|value| *value > index + 1)?;
    match &tokens[value].kind {
        LlvmTokenKind::RightParenthesis => matching_left_parenthesis(tokens, value)
            .and_then(|open| open.checked_sub(1))
            .filter(|head| *head > index),
        _ => Some(value),
    }
}

/// Where the operand starting at `index` ends, for the four shapes
/// [`operand_global`] accepts, plus a register. Used to tell an operand
/// followed by an argument list from one followed by anything else.
fn operand_end(tokens: &[LlvmToken], index: usize) -> Option<usize> {
    let mut index = index;
    loop {
        match &tokens.get(index)?.kind {
            LlvmTokenKind::Global(_) | LlvmTokenKind::Local => return Some(index + 1),
            LlvmTokenKind::Word(word) if is_identity_preserving_wrapper(word) => index += 1,
            LlvmTokenKind::Word(_) if is_cast_expression(tokens, index) => {
                return Some(matching_right_parenthesis(tokens, index + 1)? + 1);
            }
            _ => return None,
        }
    }
}

/// Whether an operand starting at `index` is followed by a parenthesised list,
/// which at a call is either the argument list or a spelled-out function type.
fn operand_precedes_list(tokens: &[LlvmToken], index: usize) -> bool {
    operand_end(tokens, index)
        .and_then(|end| tokens.get(end))
        .is_some_and(|token| token.kind == LlvmTokenKind::LeftParenthesis)
}

/// Finds the callee operand of a `call` or `invoke` instruction.
///
/// `tokens` must end at the enclosing function body's closing brace, so the
/// search is bounded by the next call opcode or the end of the body rather
/// than by braces: a literal aggregate return type such as
/// `call { i32, i32 } @pair()` legitimately contains `}` before the callee.
///
/// The callee is the operand immediately before the argument list, in the
/// shapes [`operand_global`] accepts, or a register. A named type can also
/// precede a parenthesised list when the instruction spells out its function
/// type, as in `call %Pair (i32, ...) @callee(i32 1)`; that operand is a type,
/// not the callee, so the search continues past its parameter list when
/// another operand precedes a list after it.
fn call_callee_operand(tokens: &[LlvmToken], call_index: usize) -> Option<CalleeOperand> {
    let mut index = call_index + 1;
    while index < tokens.len() {
        match &tokens[index].kind {
            LlvmTokenKind::Word(word) if word == "asm" => return None,
            LlvmTokenKind::Word(word) if word == "call" || word == "invoke" => break,
            _ => {}
        }
        if operand_precedes_list(tokens, index) {
            let list = operand_end(tokens, index).expect("operand precedes a list");
            if let Some(next) = matching_right_parenthesis(tokens, list).map(|end| end + 1)
                && next < tokens.len()
                && operand_precedes_list(tokens, next)
            {
                index = next;
                continue;
            }
            return Some(operand_global(tokens, index));
        }
        index += 1;
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
enum DeclaredGlobalKind {
    /// A `define` or `declare`.
    Function,
    /// An `alias`, which is callable only when its aliasee is.
    Alias { aliasee: CalleeOperand },
    /// An `ifunc`, whose resolver supplies the callee at load time.
    IFunc,
}

/// A module-scope global with the linkage scope its definition declares, so a
/// call that names it can be given the identity the module gives it rather
/// than its spelling.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DeclaredGlobal {
    kind: DeclaredGlobalKind,
    identity_scope: CallableIdentityScope,
}

/// Reports the alias or ifunc a module-scope definition introduces, as in
/// `@aliased = alias void (), ptr @aliasee`.
///
/// Aliases and ifuncs are globals that no `define` or `declare` introduces, so
/// they are collected separately. Requiring the `@name =` of a definition, and
/// reading the keyword only from the words that follow it, keeps unrelated
/// globals and their initialisers out.
fn declared_alias(tokens: &[LlvmToken], index: usize) -> Option<(&LlvmGlobal, DeclaredGlobal)> {
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
    let kind = match &keyword.kind {
        LlvmTokenKind::Word(word) if word == "ifunc" => DeclaredGlobalKind::IFunc,
        _ => DeclaredGlobalKind::Alias {
            aliasee: alias_aliasee(tokens, index + 2 + offset),
        },
    };
    // An alias's linkage keywords sit between its `=` and the `alias` or
    // `ifunc` keyword.
    let identity_scope = declared_identity_scope(tokens, index + 2, index + 2 + offset);
    Some((
        name,
        DeclaredGlobal {
            kind,
            identity_scope,
        },
    ))
}

/// Finds where the module-scope definition containing `start` ends.
///
/// Most module-scope definitions are introduced by a name and an `=`, so the
/// next such pair bounds the current one; the rest are introduced by their own
/// keyword. Definitions are bounded by tokens rather than by lines: an alias
/// may be written across several lines.
fn module_definition_end(tokens: &[LlvmToken], start: usize) -> usize {
    (start..tokens.len())
        .find(|index| {
            matches!(&tokens[*index].kind,
            LlvmTokenKind::Word(word)
                if matches!(
                    word.as_str(),
                    "define" | "declare" | "attributes" | "module" | "uselistorder"
                ))
                || matches!(&tokens[*index].kind, LlvmTokenKind::Metadata)
                || tokens
                    .get(index + 1)
                    .is_some_and(|token| token.kind == LlvmTokenKind::Equals)
        })
        .unwrap_or(tokens.len())
}

/// The aliasee of an alias definition, parsed from its operand position.
///
/// An alias is written `alias <AliaseeTy>, <AliaseeTy>* @aliasee` with an
/// optional trailing clause such as `, partition "..."`, so the aliasee is the
/// operand after the comma that ends the aliasee type, not the definition's
/// last token: a trailing clause, or a following `module asm` or `attributes`
/// block, must not hide it. The pointer type before the operand is stepped
/// over as a type, and the operand itself is read exactly, in the shapes
/// [`operand_global`] accepts. Any other head fails closed as `Unnamed`.
fn alias_aliasee(tokens: &[LlvmToken], keyword: usize) -> CalleeOperand {
    let end = module_definition_end(tokens, keyword + 1);
    let Some(operand) =
        type_separator(tokens, keyword + 1, end).and_then(|comma| type_end(tokens, comma + 1, end))
    else {
        return CalleeOperand::Unnamed;
    };
    if operand >= end {
        return CalleeOperand::Unnamed;
    }
    operand_global(tokens, operand)
}

/// Where the type starting at `index` ends: a named or primitive type, a
/// braced struct type, or a function type with its parenthesised parameter
/// list. Pointer stars carry no token, so `void ()*` ends with its parameter
/// list and `ptr` with itself.
///
/// A pointer type may name the address space it points into, as
/// `ptr addrspace(1)` or `void () addrspace(1)*`, so an `addrspace` clause
/// belongs to the type it follows rather than to the operand after it.
fn type_end(tokens: &[LlvmToken], index: usize, end: usize) -> Option<usize> {
    if index >= end {
        return None;
    }
    let mut next = match &tokens[index].kind {
        LlvmTokenKind::Word(_) | LlvmTokenKind::Local => index + 1,
        LlvmTokenKind::LeftBrace => matching_right_brace(tokens, index)? + 1,
        _ => return None,
    };
    while next < end {
        let clause = match &tokens[next].kind {
            LlvmTokenKind::LeftParenthesis => next,
            LlvmTokenKind::Word(word) if word == "addrspace" => next + 1,
            _ => break,
        };
        if tokens.get(clause).map(|token| &token.kind) != Some(&LlvmTokenKind::LeftParenthesis) {
            break;
        }
        next = matching_right_parenthesis(tokens, clause)? + 1;
    }
    Some(next)
}

/// Finds the comma that ends an alias's aliasee type, skipping the commas
/// inside parenthesised function types and braced struct types.
fn type_separator(tokens: &[LlvmToken], start: usize, end: usize) -> Option<usize> {
    let mut index = start;
    while index < end {
        match &tokens[index].kind {
            LlvmTokenKind::Comma => return Some(index),
            LlvmTokenKind::LeftParenthesis => {
                index = matching_right_parenthesis(tokens, index)? + 1
            }
            LlvmTokenKind::LeftBrace => index = matching_right_brace(tokens, index)? + 1,
            _ => index += 1,
        }
    }
    None
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
    declarations: &BTreeMap<LlvmGlobal, DeclaredGlobal>,
) -> ObservedCallTarget {
    match callee {
        CalleeOperand::Global(name) => match callable_declaration(&name, declarations) {
            Some((representation, identity_scope)) => ObservedCallTarget::Direct(ObservedCallee {
                name,
                representation,
                identity_scope,
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
fn callable_declaration(
    name: &LlvmGlobal,
    declarations: &BTreeMap<LlvmGlobal, DeclaredGlobal>,
) -> Option<(&'static str, CallableIdentityScope)> {
    let declared = declarations.get(name)?;
    let representation = match declared.kind {
        DeclaredGlobalKind::Function => LLVM_FUNCTION,
        DeclaredGlobalKind::Alias { .. } => LLVM_ALIAS,
        DeclaredGlobalKind::IFunc => LLVM_IFUNC,
    };
    // The identity scope, like the representation, describes the named global
    // itself: an `internal` alias to an external function is private to this
    // module however visible its aliasee is.
    let identity_scope = declared.identity_scope;
    let mut visited = BTreeSet::new();
    let mut current = name.clone();
    loop {
        if !visited.insert(current.clone()) {
            return None;
        }
        match &declarations.get(&current)?.kind {
            DeclaredGlobalKind::Function | DeclaredGlobalKind::IFunc => {
                return Some((representation, identity_scope));
            }
            DeclaredGlobalKind::Alias {
                aliasee: CalleeOperand::Global(aliasee),
            } => current = aliasee.clone(),
            DeclaredGlobalKind::Alias {
                aliasee: CalleeOperand::Unnamed,
            } => return None,
        }
    }
}

/// A module-scope global read during the token walk, before the module's
/// declarations are complete.
///
/// A `define` or `declare` is callable on sight, but whether an alias or an
/// ifunc is depends on globals the module may declare further down, so it is
/// staged with the line that declares it and resolved once the walk is over.
enum PendingGlobal {
    Function(ObservedCallable),
    AliasOrIfunc { name: LlvmGlobal, line: usize },
}

fn observe_llvm_ir(text: &str) -> Result<LlvmObservations, String> {
    let tokens = tokenize_llvm_ir(text)?;
    let mut observations = LlvmObservations::default();
    let mut declarations: BTreeMap<LlvmGlobal, DeclaredGlobal> = BTreeMap::new();
    let mut pending_globals: Vec<PendingGlobal> = Vec::new();
    let mut pending_calls: Vec<PendingCall> = Vec::new();
    let mut current = None;
    let mut body_end = 0_usize;
    let mut index = 0;
    while index < tokens.len() {
        if current.is_none() {
            if let Some((name, declaration)) = declared_alias(&tokens, index) {
                let name = name.clone();
                pending_globals.push(PendingGlobal::AliasOrIfunc {
                    name: name.clone(),
                    line: tokens[index].line,
                });
                declarations.insert(name, declaration);
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
            // A function's linkage keywords sit between `define`/`declare`
            // and its global name.
            let identity_scope = declared_identity_scope(&tokens, index + 1, name_index);
            declarations.insert(
                name.clone(),
                DeclaredGlobal {
                    kind: DeclaredGlobalKind::Function,
                    identity_scope,
                },
            );
            pending_globals.push(PendingGlobal::Function(ObservedCallable {
                name: name.clone(),
                defined,
                line: tokens[index].line,
                representation: LLVM_FUNCTION,
                identity_scope,
            }));
            let signature_end = function_signature_end(&tokens, name_index).ok_or_else(|| {
                format!(
                    "LLVM function '{}' has an incomplete parameter list",
                    name.display_name()
                )
            })?;
            if !defined {
                index = signature_end + 1;
                continue;
            }
            let (body_index, end) =
                function_body_bounds(&tokens, signature_end, &name.display_name())?;
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

    // An alias or ifunc is contributed as a callable of its own kind, in the
    // order the module writes it, but only once the whole module is observed:
    // it is callable only when its chain of aliasees reaches a function or an
    // ifunc, and the aliasee may be declared further down. An alias to data,
    // to an undeclared global, or around a cycle is no callable and is
    // contributed as none. A name a `define` or `declare` already introduced
    // stays that function: LLVM rejects a module that spells one global twice,
    // and a contributor must not assert one callable identity twice either.
    let function_names = pending_globals
        .iter()
        .filter_map(|global| match global {
            PendingGlobal::Function(callable) => Some(callable.name.clone()),
            PendingGlobal::AliasOrIfunc { .. } => None,
        })
        .collect::<BTreeSet<_>>();
    let mut aliased_names = BTreeSet::new();
    for global in &pending_globals {
        match global {
            PendingGlobal::Function(callable) => observations.callables.push(callable.clone()),
            PendingGlobal::AliasOrIfunc { name, line } => {
                if function_names.contains(name) || !aliased_names.insert(name.clone()) {
                    continue;
                }
                if let Some((representation, identity_scope)) =
                    callable_declaration(name, &declarations)
                {
                    observations.callables.push(ObservedCallable {
                        name: name.clone(),
                        defined: false,
                        line: *line,
                        representation,
                        identity_scope,
                    });
                }
            }
        }
    }

    // Callee operands are resolved after the whole module has been observed:
    // textual LLVM IR may declare a called function, or the aliasee an alias
    // points at, after the call site.
    for call in pending_calls {
        let target = resolve_callee(call.callee, &declarations);
        if matches!(&target, ObservedCallTarget::Direct(callee) if callee.name.is_intrinsic()) {
            continue;
        }
        // A caller is always a function this module defines, so the module
        // states its linkage and the call site inherits the identity the module
        // gives its caller rather than the caller's spelling.
        let (_, caller_identity_scope) = callable_declaration(&call.caller, &declarations)
            .expect("a function body's caller must be a declared callable");
        observations.calls.push(ObservedCall {
            caller: call.caller,
            caller_identity_scope,
            target,
            line: call.line,
        });
    }
    Ok(observations)
}

/// The opaque identity a callable has in the prototype graph.
///
/// It is derived from the contributor's scoped identity evidence, not from the
/// callable's display label. A linkage-namespace identity therefore remains
/// one graph identity across inputs, while an acquired-input identity is also
/// qualified by its input. Hashing the qualified evidence keeps the legacy
/// schema's human-readable label separate from its machine identity.
fn graph_node_id(identity: &ContributorCallableIdentity, source: Option<&str>) -> String {
    let input_qualifier = match identity.scope() {
        CallableIdentityScope::AcquiredInput => source.unwrap_or("in-memory LLVM IR"),
        CallableIdentityScope::LinkageNamespace => "linkage namespace",
    };
    format!(
        "callable:{}",
        fingerprint_parts(&[identity.as_str(), input_qualifier])
    )
}

pub fn parse_llvm_ir(text: &str, source: Option<&str>) -> Result<Graph, String> {
    let observations = observe_llvm_ir(text)?;
    let content_fingerprint = fingerprint_parts(&[text]);
    let mut graph = Graph::default();
    if let Some(source) = source {
        graph.inputs.push(source.into());
    }
    for callable in observations.callables {
        let identity = contributor_callable_identity(
            &callable.name,
            callable.identity_scope,
            &content_fingerprint,
        );
        graph.add_node(Node::callable(
            graph_node_id(&identity, source),
            callable.name.display_name(),
            callable.defined,
            source.map(str::to_owned),
        ));
    }
    for call in observations.calls {
        let caller_identity = contributor_callable_identity(
            &call.caller,
            call.caller_identity_scope,
            &content_fingerprint,
        );
        let caller = graph_node_id(&caller_identity, source);
        match call.target {
            ObservedCallTarget::Direct(callee) => {
                let callee_identity = contributor_callable_identity(
                    &callee.name,
                    callee.identity_scope,
                    &content_fingerprint,
                );
                let callee_id = graph_node_id(&callee_identity, source);
                graph.add_node(Node::callable(
                    &callee_id,
                    callee.name.display_name(),
                    false,
                    None,
                ));
                graph.add_edge(&caller, &callee_id, "direct-call");
            }
            ObservedCallTarget::Indirect => {
                graph.add_node(Node {
                    id: "unknown:indirect-call-target".into(),
                    label: "indirect call".into(),
                    kind: "unknown".into(),
                    defined: false,
                    language: "llvm".into(),
                    source: None,
                });
                graph.add_edge(&caller, "unknown:indirect-call-target", "indirect-call");
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

    fn node_id(graph: &Graph, label: &str) -> String {
        graph
            .nodes
            .values()
            .find(|node| node.label == label)
            .unwrap_or_else(|| panic!("missing graph node labelled {label:?}"))
            .id
            .clone()
    }

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
        let main = node_id(&graph, "main");
        let worker = node_id(&graph, "worker");
        let puts = node_id(&graph, "puts");
        assert!(graph.nodes[&main].defined);
        assert!(!graph.nodes[&puts].defined);
        assert_eq!(
            graph.edges[&(main.clone(), worker, "direct-call".into())].call_count,
            2
        );
        assert!(graph.edges.contains_key(&(
            main,
            "unknown:indirect-call-target".into(),
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
            assert!(!graph.nodes.values().any(|node| node.label == uncallable));
        }
        let caller = node_id(&graph, "caller");
        assert!(graph.edges.contains_key(&(
            caller.clone(),
            "unknown:indirect-call-target".into(),
            "indirect-call".into()
        )));
        for callable in ["declared_target", "function_alias", "split_alias"] {
            assert!(graph.edges.contains_key(&(
                caller.clone(),
                node_id(&graph, callable),
                "direct-call".into()
            )));
        }
        assert_eq!(
            graph.edges[&(
                caller,
                "unknown:indirect-call-target".into(),
                "indirect-call".into()
            )]
                .call_count,
            4
        );
    }

    #[test]
    fn signed_pointer_constants_do_not_publish_unverified_target_claims() {
        // Cover the archived signed-pointer examples, plus casts and call
        // bundles. A referenced function or discriminator alone does not
        // establish that the signed pointer is authenticated for this call.
        for instruction in [
            "call void ptrauth (ptr @target, i32 0, i64 7, ptr @discriminator)()",
            "call void ptrauth (ptr null, i32 0, i64 7, ptr @target)()",
            "call void bitcast (ptr ptrauth (ptr @target, i32 0) to ptr)()",
            "call void ptrauth (ptr bitcast (ptr @target to ptr), i32 0)()",
            "call void ptrauth (ptr @target, i32 0, i64 7)() [ \"ptrauth\"(i32 1, i64 7) ]",
            "call void ptrauth (ptr @target, i32 0, i64 7)() [ \"ptrauth\"(i32 0, i64 7) ]",
        ] {
            let ir = format!(
                "declare void @target()\n@discriminator = external global i8\n\
                 define void @caller() {{\n{instruction}\nret void\n}}"
            );
            let graph = parse_llvm_ir(&ir, Some("signed-pointer.ll")).unwrap();
            let caller = node_id(&graph, "caller");
            assert_eq!(graph.edges.len(), 1, "{instruction}");
            assert!(
                graph.edges.contains_key(&(
                    caller,
                    "unknown:indirect-call-target".into(),
                    "indirect-call".into()
                )),
                "{instruction}"
            );
        }
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
