use crate::model::{
    AcquisitionIdentity, CallObservation, EvidenceContributorMetadata, ExtractedObservationContext,
    Graph, Node,
};
use regex::Regex;
use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const LLVM_EMISSION_ARGUMENTS: &[&str] =
    &["-S", "-emit-llvm", "-g", "-O0", "-fno-discard-value-names"];

fn content_fingerprint(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("fnv1a64:{hash:016x}")
}

fn captured_name(captures: &regex::Captures<'_>, first: usize) -> String {
    captures
        .get(first)
        .or_else(|| captures.get(first + 1))
        .unwrap()
        .as_str()
        .to_owned()
}

struct LexicalLine<'a> {
    code: &'a str,
    unquoted: String,
    symbols: String,
}

fn scan_llvm_line(line: &str) -> LexicalLine<'_> {
    let mut unquoted = String::with_capacity(line.len());
    let mut symbols = String::with_capacity(line.len());
    let mut quoted = false;
    let mut preserve_symbol = false;
    let mut escaped = false;
    let mut code_end = line.len();
    for (index, character) in line.char_indices() {
        if quoted {
            unquoted.extend(std::iter::repeat_n(' ', character.len_utf8()));
            if preserve_symbol {
                symbols.push(character);
            } else {
                symbols.extend(std::iter::repeat_n(' ', character.len_utf8()));
            }
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
                preserve_symbol = false;
            }
        } else if character == ';' {
            code_end = index;
            break;
        } else if character == '"' {
            quoted = true;
            preserve_symbol = index > 0 && line.as_bytes()[index - 1] == b'@';
            unquoted.push(' ');
            symbols.push(if preserve_symbol { character } else { ' ' });
        } else {
            unquoted.push(character);
            symbols.push(character);
        }
    }
    LexicalLine {
        code: &line[..code_end],
        unquoted,
        symbols,
    }
}

fn is_llvm_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'$' | b'.' | b'_')
}

fn is_identifier_or_label(line: &str, start: usize, end: usize) -> bool {
    let bytes = line.as_bytes();
    if (start > 0 && is_llvm_identifier_byte(bytes[start - 1]))
        || bytes
            .get(end)
            .is_some_and(|byte| is_llvm_identifier_byte(*byte))
    {
        return true;
    }
    let mut identifier_start = start;
    while identifier_start > 0 && is_llvm_identifier_byte(bytes[identifier_start - 1]) {
        identifier_start -= 1;
    }
    (identifier_start > 0 && matches!(bytes[identifier_start - 1], b'%' | b'@' | b'!'))
        || bytes.get(end) == Some(&b':')
}

fn call_instruction_ranges(
    line: &LexicalLine<'_>,
    call_opcode: &Regex,
    inline_asm: &Regex,
) -> Vec<(usize, usize)> {
    let opcodes: Vec<_> = call_opcode
        .find_iter(&line.unquoted)
        .filter(|found| !is_identifier_or_label(&line.unquoted, found.start(), found.end()))
        .collect();
    opcodes
        .iter()
        .enumerate()
        .filter(|(index, found)| {
            let end = opcodes
                .get(index + 1)
                .map_or(line.unquoted.len(), |next| next.start());
            !inline_asm
                .find_iter(&line.unquoted[found.end()..end])
                .any(|keyword| {
                    !is_identifier_or_label(
                        &line.unquoted,
                        found.end() + keyword.start(),
                        found.end() + keyword.end(),
                    )
                })
        })
        .map(|(index, found)| {
            let end = opcodes
                .get(index + 1)
                .map_or(line.code.len(), |next| next.start());
            (found.start(), end)
        })
        .collect()
}

fn compiler_build_configuration(flags: &[String]) -> String {
    let arguments: Vec<_> = LLVM_EMISSION_ARGUMENTS
        .iter()
        .copied()
        .chain(flags.iter().map(String::as_str))
        .collect();
    format!(
        "clang argv: {}",
        serde_json::to_string(&arguments).expect("compiler arguments must serialize")
    )
}

pub fn parse_llvm_ir(text: &str, source: Option<&str>) -> Result<Graph, String> {
    let symbol = r#"@(?:\"((?:[^\"\\]|\\.)+)\"|([-a-zA-Z$._0-9]+))"#;
    let function = Regex::new(&format!(r"^\s*(define|declare)\b.*?{symbol}\s*\("))
        .map_err(|e| e.to_string())?;
    let call = Regex::new(&format!(r"\b(?:call|invoke)\b[^@\n]*?{symbol}\s*\("))
        .map_err(|e| e.to_string())?;
    let any_call = Regex::new(r"\b(?:call|invoke)\b").map_err(|e| e.to_string())?;
    let inline_asm = Regex::new(r"\basm\b").map_err(|e| e.to_string())?;
    let local_linkage = Regex::new(r"\b(?:internal|private)\b").map_err(|e| e.to_string())?;
    let local_symbols: HashSet<_> = text
        .lines()
        .filter_map(|raw_line| {
            let lexical = scan_llvm_line(raw_line);
            let found = function.captures(&lexical.symbols)?;
            let symbol_start = found.get(2).or_else(|| found.get(3))?.start();
            local_linkage
                .is_match(&lexical.unquoted[..symbol_start])
                .then(|| captured_name(&found, 2))
        })
        .collect();
    let mut graph = Graph::default();
    let fingerprint = content_fingerprint(text);
    let input = source.unwrap_or("<memory>");
    let acquisition = AcquisitionIdentity::new(input, &fingerprint);
    graph.acquisitions.push(acquisition.clone());
    graph.observation_target = input.to_owned();
    graph.build_configuration = "textual LLVM IR as acquired".into();
    graph.toolchain = "producer unspecified by textual LLVM IR".into();
    graph.contributor = EvidenceContributorMetadata {
        extraction_method: "gloom-llvm-text-v1".into(),
        analysis_stage: "textual-ir".into(),
        acquired_input_kind: "llvm-ir".into(),
        manifestation_kind: "llvm-function".into(),
        evidence_kind: "llvm-direct-call".into(),
        claim_kind: "direct-target".into(),
        derivation: "direct LLVM callee operand".into(),
        entity_language: "llvm".into(),
        call_resolution: "complete".into(),
    };
    let observation_context = ExtractedObservationContext::new(
        &graph.observation_target,
        &graph.build_configuration,
        &graph.toolchain,
        &graph.contributor,
    );
    graph.associate_acquisition(acquisition.clone(), observation_context.clone());
    let mut current: Option<String> = None;
    let mut brace_depth: isize = 0;

    for (line_index, raw_line) in text.lines().enumerate() {
        let lexical = scan_llvm_line(raw_line);
        let caller = if let Some(found) = function.captures(&lexical.symbols) {
            let name = captured_name(&found, 2);
            let defined = &found[1] == "define";
            graph.add_node(Node::function(&name, defined, source.map(str::to_owned)));
            if local_symbols.contains(&name) {
                graph.scope_entity_to_acquisition(&name, acquisition.clone());
            }
            graph.observe_manifestation(&name, acquisition.clone(), observation_context.clone());
            if defined {
                current = Some(name.clone());
                brace_depth = lexical.unquoted.matches('{').count() as isize
                    - lexical.unquoted.matches('}').count() as isize;
                name
            } else {
                continue;
            }
        } else {
            let Some(caller) = current.clone() else {
                continue;
            };
            brace_depth += lexical.unquoted.matches('{').count() as isize
                - lexical.unquoted.matches('}').count() as isize;
            caller
        };
        let mut direct_ordinal = 0;
        let mut indirect_call_count = 0;
        for (start, end) in call_instruction_ranges(&lexical, &any_call, &inline_asm) {
            let instruction = &lexical.symbols[start..end];
            if let Some(found) = call.captures(instruction) {
                direct_ordinal += 1;
                let callee = captured_name(&found, 1);
                if callee.starts_with("llvm.") {
                    continue;
                }
                let caller = caller.clone();
                graph.add_node(Node::function(&callee, false, None));
                graph.add_edge(&caller, &callee, "direct-call");
                graph.observe_manifestation(
                    &callee,
                    acquisition.clone(),
                    observation_context.clone(),
                );
                graph.call_observations.push(CallObservation {
                    caller,
                    callee,
                    acquisition: acquisition.clone(),
                    context: observation_context.clone(),
                    line: line_index + 1,
                    ordinal: direct_ordinal,
                });
            } else {
                indirect_call_count += 1;
            }
        }
        if indirect_call_count > 0 {
            let caller = caller.clone();
            graph.add_node(Node {
                id: "<indirect>".into(),
                label: "indirect call".into(),
                kind: "unknown".into(),
                defined: false,
                language: "llvm".into(),
                source: None,
            });
            for _ in 0..indirect_call_count {
                graph.add_edge(&caller, "<indirect>", "indirect-call");
            }
        }
        if brace_depth <= 0 && lexical.unquoted.contains('}') {
            current = None;
        }
    }
    Ok(graph)
}

#[derive(Debug)]
struct ResolvedCompiler {
    requested: String,
    invocation: PathBuf,
    identity: String,
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn executable_candidates(path: &Path, extensions: &[String]) -> Vec<PathBuf> {
    let mut candidates = vec![path.to_path_buf()];
    if path.extension().is_none() {
        candidates.extend(
            extensions
                .iter()
                .filter(|extension| !extension.is_empty())
                .map(|extension| path.with_extension(extension.trim_start_matches('.'))),
        );
    }
    candidates
}

fn resolve_executable_from(
    command: &str,
    search_path: Option<&OsStr>,
    extensions: &[String],
) -> Option<PathBuf> {
    let requested = Path::new(command);
    let find_at = |path: &Path| {
        executable_candidates(path, extensions)
            .into_iter()
            .find(|candidate| is_executable(candidate))
    };
    if requested.is_absolute() || requested.parent().is_some_and(|p| p != Path::new("")) {
        find_at(requested)
    } else {
        search_path.and_then(|path| {
            env::split_paths(path).find_map(|directory| find_at(&directory.join(requested)))
        })
    }
}

fn resolve_executable(command: &str) -> Result<PathBuf, String> {
    #[cfg(windows)]
    let extensions: Vec<String> = env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .map(str::to_owned)
        .collect();
    #[cfg(not(windows))]
    let extensions = Vec::new();
    resolve_executable_from(command, env::var_os("PATH").as_deref(), &extensions).ok_or_else(|| {
        format!(
            "'{command}' was not found; install Clang, pass --clang PATH, or provide a .ll file"
        )
    })
}

fn resolve_compiler(command: &str) -> Result<ResolvedCompiler, String> {
    let resolved_invocation = resolve_executable(command)?;
    identify_compiler(command, resolved_invocation)
}

fn identify_compiler(
    requested: &str,
    resolved_invocation: PathBuf,
) -> Result<ResolvedCompiler, String> {
    let canonical_target = fs::canonicalize(&resolved_invocation).map_err(|error| {
        format!(
            "failed to resolve compiler '{}': {error}",
            resolved_invocation.display()
        )
    })?;
    let result = Command::new(&resolved_invocation)
        .arg("--version")
        .output()
        .map_err(|error| {
            format!(
                "failed to identify compiler '{}': {error}",
                resolved_invocation.display()
            )
        })?;
    if !result.status.success() {
        return Err(format!(
            "failed to identify compiler '{}': {}",
            resolved_invocation.display(),
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    let version = String::from_utf8_lossy(&result.stdout).trim().to_owned();
    if version.is_empty() {
        return Err(format!(
            "compiler '{}' returned no version identity",
            resolved_invocation.display()
        ));
    }
    let invocation_identity = resolved_invocation.to_string_lossy();
    let canonical_identity = canonical_target.to_string_lossy();
    let components = [
        invocation_identity.as_ref(),
        canonical_identity.as_ref(),
        version.as_str(),
    ];
    let identity = format!(
        "compiler identity: {}",
        serde_json::to_string(&components).expect("compiler identity must serialize")
    );
    Ok(ResolvedCompiler {
        requested: requested.to_owned(),
        invocation: resolved_invocation,
        identity,
    })
}

fn compile_c(path: &Path, compiler: &ResolvedCompiler, flags: &[String]) -> Result<String, String> {
    let unique = format!(
        "gloom-{}-{}.ll",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let output = std::env::temp_dir().join(unique);
    let result = Command::new(&compiler.invocation)
        .args(LLVM_EMISSION_ARGUMENTS)
        .args(flags)
        .arg(path)
        .arg("-o")
        .arg(&output)
        .output()
        .map_err(|error| error.to_string())?;
    if !result.status.success() {
        return Err(format!(
            "Clang '{}' failed for {}:\n{}",
            compiler.requested,
            path.display(),
            String::from_utf8_lossy(&result.stderr).trim()
        ));
    }
    let text = fs::read_to_string(&output).map_err(|e| e.to_string());
    let _ = fs::remove_file(output);
    text
}

fn acquired_input_name(path: &Path, extension: &str) -> String {
    match extension {
        "c" | "i" => format!("{}::<generated-llvm-ir>", path.display()),
        _ => path.display().to_string(),
    }
}

pub fn graph_from_path(path: &Path, clang: &str, flags: &[String]) -> Result<Graph, String> {
    let extension = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or_default();
    let (text, compiler) = match extension {
        "ll" => (
            fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
            None,
        ),
        "c" | "i" => {
            let compiler = resolve_compiler(clang)?;
            let text = compile_c(path, &compiler, flags)?;
            (text, Some(compiler))
        }
        _ => {
            return Err(format!(
                "unsupported input '{}'; expected .c, .i, or .ll",
                path.display()
            ));
        }
    };
    let acquired_input = acquired_input_name(path, extension);
    let mut graph = parse_llvm_ir(&text, Some(&acquired_input))?;
    graph.observation_target = path.display().to_string();
    match extension {
        "c" | "i" => {
            graph.build_configuration = compiler_build_configuration(flags);
            graph.toolchain = compiler
                .as_ref()
                .expect("compiled input must have a resolved compiler")
                .identity
                .clone();
        }
        _ => {
            graph.build_configuration = "textual LLVM IR as acquired".into();
            graph.toolchain = "producer unspecified by textual LLVM IR".into();
        }
    }
    let context = ExtractedObservationContext::new(
        &graph.observation_target,
        &graph.build_configuration,
        &graph.toolchain,
        &graph.contributor,
    );
    graph.acquisition_contexts.clear();
    for acquisition in graph.acquisitions.clone() {
        graph.associate_acquisition(acquisition, context.clone());
    }
    for observation in &mut graph.call_observations {
        observation.context = context.clone();
    }
    graph.observed_manifestations = graph
        .observed_manifestations
        .into_iter()
        .map(|mut observation| {
            observation.context = context.clone();
            observation
        })
        .collect();
    Ok(graph)
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
    fn records_distinct_ordinals_for_multiple_direct_calls_on_one_line() {
        let graph = parse_llvm_ir(
            "define void @caller() {\n  call void @first(), call void @second()\n  ret void\n}",
            Some("same-line.ll"),
        )
        .unwrap();

        assert_eq!(graph.call_observations.len(), 2);
        assert_eq!(graph.call_observations[0].callee, "first");
        assert_eq!(graph.call_observations[0].ordinal, 1);
        assert_eq!(graph.call_observations[1].callee, "second");
        assert_eq!(graph.call_observations[1].ordinal, 2);
    }

    #[test]
    fn review_fixes_parse_calls_on_function_definition_lines() {
        let graph = parse_llvm_ir(
            "define void @caller() { call void @first() call void @second() ret void }",
            Some("compact.ll"),
        )
        .unwrap();

        assert_eq!(graph.call_observations.len(), 2);
        assert_eq!(graph.call_observations[0].callee, "first");
        assert_eq!(graph.call_observations[0].ordinal, 1);
        assert_eq!(graph.call_observations[1].callee, "second");
        assert_eq!(graph.call_observations[1].ordinal, 2);
    }

    #[test]
    fn records_indirect_calls_that_share_a_line_with_direct_calls() {
        let graph = parse_llvm_ir(
            "define void @caller() {\n  call void @direct(), call void %indirect()\n  ret void\n}",
            Some("mixed.ll"),
        )
        .unwrap();

        assert_eq!(
            graph.edges[&("caller".into(), "<indirect>".into(), "indirect-call".into())].call_count,
            1
        );
        assert_eq!(graph.call_observations.len(), 1);
        assert_eq!(graph.call_observations[0].callee, "direct");
    }

    #[test]
    fn inline_assembly_does_not_suppress_neighboring_indirect_calls() {
        let graph = parse_llvm_ir(
            "define void @caller() {\n  call void asm sideeffect \"call @phantom\", \"\"(), call void @direct(), call void %indirect()\n  ret void\n}",
            Some("inline-asm.ll"),
        )
        .unwrap();

        assert_eq!(
            graph.edges[&("caller".into(), "<indirect>".into(), "indirect-call".into())].call_count,
            1
        );
        assert_eq!(graph.call_observations.len(), 1);
        assert_eq!(graph.call_observations[0].callee, "direct");
        assert!(!graph.nodes.contains_key("phantom"));
    }

    #[test]
    fn ignores_calls_in_comments() {
        let graph = parse_llvm_ir(
            "define void @caller() {\n  call void @direct() ; call void %callback()\n  ret void\n}",
            Some("comment.ll"),
        )
        .unwrap();

        assert_eq!(graph.call_observations.len(), 1);
        assert_eq!(graph.call_observations[0].callee, "direct");
        assert!(!graph.edges.contains_key(&(
            "caller".into(),
            "<indirect>".into(),
            "indirect-call".into()
        )));
    }

    #[test]
    fn ignores_call_keywords_in_quoted_operand_bundle_tags() {
        let graph = parse_llvm_ir(
            "define void @caller() {\n  call void @direct() [ \"call\"(i32 1), \"invoke\"(i32 2) ]\n  ret void\n}",
            Some("operand-bundle.ll"),
        )
        .unwrap();

        assert_eq!(
            graph.edges[&("caller".into(), "direct".into(), "direct-call".into())].call_count,
            1
        );
        assert!(!graph.edges.contains_key(&(
            "caller".into(),
            "<indirect>".into(),
            "indirect-call".into()
        )));
    }

    #[test]
    fn quoted_bundle_tags_do_not_turn_indirect_calls_into_direct_calls() {
        let graph = parse_llvm_ir(
            "define void @caller() {\n  call void %fp() [ \"@fake()\"(i32 1) ]\n  ret void\n}",
            Some("quoted-lookalike.ll"),
        )
        .unwrap();

        assert!(graph.call_observations.is_empty());
        assert!(!graph.nodes.contains_key("fake"));
        assert_eq!(
            graph.edges[&("caller".into(), "<indirect>".into(), "indirect-call".into())].call_count,
            1
        );
    }

    #[test]
    fn review_fixes_keep_quoted_symbol_braces_out_of_function_scope() {
        let graph = parse_llvm_ir(
            "declare void @\"callee}\"()\ndefine void @caller() {\n  call void @\"callee}\"()\n  call void @after()\n  ret void\n}",
            Some("quoted-brace.ll"),
        )
        .unwrap();

        assert_eq!(graph.call_observations.len(), 2);
        assert_eq!(graph.call_observations[0].callee, "callee}");
        assert_eq!(graph.call_observations[1].callee, "after");
    }

    #[test]
    fn opcode_prefixed_labels_are_not_calls() {
        let graph = parse_llvm_ir(
            "define void @caller() {\ncall.1:\n  ret void\n}",
            Some("label.ll"),
        )
        .unwrap();

        assert!(graph.edges.is_empty());
        assert!(graph.call_observations.is_empty());
    }

    #[test]
    fn review_fixes_ignore_call_keywords_used_as_llvm_identifiers() {
        let graph = parse_llvm_ir(
            "define void @caller() {\n  %call = call i32 @callee()\n  call void @call()\n  ret void\n}",
            Some("identifiers.ll"),
        )
        .unwrap();

        assert_eq!(graph.call_observations.len(), 2);
        assert!(!graph.edges.contains_key(&(
            "caller".into(),
            "<indirect>".into(),
            "indirect-call".into()
        )));
    }

    #[test]
    fn review_fixes_encode_compiler_arguments_losslessly() {
        let one_argument = compiler_build_configuration(&["-I/tmp/a -I/tmp/b".into()]);
        let two_arguments = compiler_build_configuration(&["-I/tmp/a".into(), "-I/tmp/b".into()]);

        assert_ne!(one_argument, two_arguments);
        let encoded_arguments = one_argument.strip_prefix("clang argv: ").unwrap();
        let arguments: Vec<String> = serde_json::from_str(encoded_arguments).unwrap();
        assert_eq!(arguments.last().unwrap(), "-I/tmp/a -I/tmp/b");
    }

    #[cfg(unix)]
    #[test]
    fn review_fixes_record_the_compiler_arguments_that_are_executed() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "gloom-compiler-arguments-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let compiler = directory.join("clang-alias");
        let captured = directory.join("arguments.txt");
        let source = directory.join("input.c");
        fs::write(&source, "void caller(void) {}\n").unwrap();
        fs::write(
            &compiler,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '%s\\n' 'clang version test'\n  exit 0\nfi\n: > '{captured}'\noutput=\nprevious=\nfor argument in \"$@\"; do\n  printf '%s\\n' \"$argument\" >> '{captured}'\n  if [ \"$previous\" = \"-o\" ]; then output=$argument; fi\n  previous=$argument\ndone\nprintf '%s\\n' 'define void @caller() {{' '  ret void' '}}' > \"$output\"\n",
                captured = captured.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&compiler).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&compiler, permissions).unwrap();
        let flags = vec!["-DEXTRA=two words".to_owned()];

        let graph = graph_from_path(&source, compiler.to_str().unwrap(), &flags).unwrap();
        let recorded: Vec<String> = serde_json::from_str(
            graph
                .build_configuration
                .strip_prefix("clang argv: ")
                .unwrap(),
        )
        .unwrap();
        let executed: Vec<_> = fs::read_to_string(&captured)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect();

        assert_eq!(&executed[..recorded.len()], recorded);
        assert_eq!(executed[recorded.len()], source.to_string_lossy().as_ref());
        assert_eq!(executed[recorded.len() + 1], "-o");
        assert_eq!(graph.call_observations.len(), 0);

        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn review_fixes_execute_the_resolved_compiler_for_identity_and_compilation() {
        use std::os::unix::fs::PermissionsExt;

        let directory = std::env::temp_dir().join(format!(
            "gloom-resolved-compiler-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let resolved = directory.join("clang.CMD");
        let source = directory.join("input.c");
        fs::write(&source, "void caller(void) {}\n").unwrap();
        fs::write(
            &resolved,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then\n  printf '%s\\n' 'resolved compiler version'\n  exit 0\nfi\noutput=\nprevious=\nfor argument in \"$@\"; do\n  if [ \"$previous\" = \"-o\" ]; then output=$argument; fi\n  previous=$argument\ndone\nprintf '%s\\n' 'define void @resolved() {' '  ret void' '}' > \"$output\"\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&resolved).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&resolved, permissions).unwrap();

        let compiler = identify_compiler("clang-not-on-path", resolved).unwrap();
        let ir = compile_c(&source, &compiler, &[]).unwrap();

        assert!(compiler.identity.contains("resolved compiler version"));
        assert!(ir.contains("define void @resolved()"));

        fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn resolving_a_compiler_alias_preserves_the_invocation_path() {
        use std::os::unix::fs::symlink;

        let directory = std::env::temp_dir().join(format!(
            "gloom-compiler-alias-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let alias = directory.join("clang++");
        symlink(std::env::current_exe().unwrap(), &alias).unwrap();

        assert_eq!(resolve_executable(alias.to_str().unwrap()).unwrap(), alias);

        fs::remove_file(&alias).unwrap();
        fs::remove_dir(&directory).unwrap();
    }

    #[test]
    fn review_fixes_resolve_compilers_with_pathext_candidates() {
        let directory = std::env::temp_dir().join(format!(
            "gloom-compiler-pathext-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let executable = directory.join("clang.EXE");
        fs::write(&executable, "test executable").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&executable).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&executable, permissions).unwrap();
        }
        let path = env::join_paths([&directory]).unwrap();

        assert_eq!(
            resolve_executable_from("clang", Some(path.as_os_str()), &[".EXE".into()]),
            Some(executable)
        );

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn review_fixes_qualify_compiler_toolchain_with_path_and_version() {
        let graph = graph_from_path(Path::new("examples/demo.c"), "clang", &[]).unwrap();
        let encoded = graph.toolchain.strip_prefix("compiler identity: ").unwrap();
        let [invocation, canonical_target, version]: [String; 3] =
            serde_json::from_str(encoded).unwrap();

        assert!(Path::new(&invocation).is_absolute());
        assert!(Path::new(&canonical_target).is_absolute());
        assert!(version.contains("clang version"));
        assert_ne!(graph.toolchain, "clang");
    }

    #[test]
    fn labels_compiled_inputs_as_generated_llvm_ir() {
        assert_eq!(
            acquired_input_name(Path::new("demo.c"), "c"),
            "demo.c::<generated-llvm-ir>"
        );
        assert_eq!(
            acquired_input_name(Path::new("fixture.ll"), "ll"),
            "fixture.ll"
        );
    }
}
