//! The local query service answers the same bounded named queries as the CLI,
//! and answers nothing else: there is no way through it to obtain the program
//! snapshot and reinterpret it in a browser.

use gloom::app::Application;
use gloom::queries::*;
use gloom::service::*;
use gloom::*;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};

fn input(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/bounded-queries")
        .join(name)
}

fn context() -> ObservationContext {
    ObservationContext::static_analysis(
        "local-viewer",
        "server",
        "debug",
        "LLVM fixture",
        "gloom.llvm-text",
        env!("CARGO_PKG_VERSION"),
        "LLVM IR",
    )
}

fn snapshot() -> PublishedSnapshot {
    Application
        .publish_snapshot(
            &[input("calls.ll"), input("local.ll")],
            context(),
            &LlvmTextContributor::new("clang", &[]),
        )
        .unwrap()
}

fn request(query: Investigation) -> BoundedQuery {
    BoundedQuery {
        scope: QueryScope {
            build_target: "server".into(),
            observation_context_ids: vec![context().id],
        },
        resolution_policy: ResolutionPolicy::IncludePossible,
        world: WorldPolicy::Open {},
        bounds: QueryBounds {
            max_depth: 4,
            max_results: 100,
            max_steps: 10_000,
        },
        query,
    }
}

fn service(snapshot: &PublishedSnapshot) -> LocalQueryService {
    Application.local_query_service(snapshot.clone())
}

fn answered(service: &LocalQueryService, request: &ServiceRequest) -> (u16, serde_json::Value) {
    let response = service.respond(request);
    (
        response.status,
        serde_json::from_slice(&response.body).expect("every JSON route answers with JSON"),
    )
}

fn investigated(service: &LocalQueryService, query: &BoundedQuery) -> (u16, serde_json::Value) {
    answered(
        service,
        &ServiceRequest::post(INVESTIGATE_PATH, serde_json::to_vec(query).unwrap()),
    )
}

/// Run one command of the installed CLI against an exported snapshot.
fn cli(snapshot: &PublishedSnapshot, arguments: &[&str], request: Option<&BoundedQuery>) -> String {
    let directory = std::env::temp_dir().join(format!(
        "gloom-local-viewer-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let stored = directory.join("snapshot.json");
    std::fs::write(&stored, Application.export_snapshot_json(snapshot).unwrap()).unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_gloom"));
    command.arg(arguments[0]).arg(&stored).args(&arguments[1..]);
    if let Some(request) = request {
        let path = directory.join("query.json");
        std::fs::write(&path, serde_json::to_vec(request).unwrap()).unwrap();
        command.arg("--request").arg(&path);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(directory).unwrap();
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn the_service_reports_the_scope_a_person_selects_before_exploring() {
    let snapshot = snapshot();
    let (status, scope) = answered(&service(&snapshot), &ServiceRequest::get(SCOPE_PATH));

    assert_eq!(status, 200);
    assert_eq!(scope["program_snapshot_id"], "local-viewer");
    assert_eq!(scope["build_targets"].as_array().unwrap().len(), 1);
    let target = &scope["build_targets"][0];
    assert_eq!(target["build_target"], "server");
    assert_eq!(target["observation_contexts"][0]["id"].as_str().unwrap(), {
        let id = context().id;
        id.as_str().to_owned()
    });
    assert_eq!(target["observation_contexts"][0]["build_target"], "server");
    assert_eq!(scope["maximum_bounds"]["max_depth"], 100);
    assert_eq!(scope["maximum_bounds"]["max_results"], 10_000);
    assert_eq!(scope["maximum_bounds"]["max_steps"], 1_000_000);
    assert_eq!(scope["maximum_observation_contexts"], 100);
    // The starting bounds keep a first exploration focused rather than whole.
    assert!(
        scope["default_bounds"]["max_depth"].as_u64().unwrap()
            < scope["maximum_bounds"]["max_depth"].as_u64().unwrap()
    );
    // Selecting a scope exposes no program entity, call site, or claim.
    for absent in [
        "program_entities",
        "call_graph_projection",
        "evidence_records",
    ] {
        assert!(scope.get(absent).is_none(), "{absent}");
    }
}

#[test]
fn the_service_and_the_cli_run_the_same_bounded_named_queries() {
    let snapshot = snapshot();
    let service = service(&snapshot);
    let selector = |label: &str| CallableSelector::by_label(label);
    for query in [
        Investigation::CallableSearch { label: "a".into() },
        Investigation::Callers {
            callee: selector("c"),
        },
        Investigation::Callees {
            caller: selector("a"),
        },
        Investigation::CallPath {
            start: selector("a"),
            end: selector("c"),
        },
        Investigation::RecursiveCycles {
            start: selector("self"),
        },
    ] {
        let mut request = request(query);
        let (status, served) = investigated(&service, &request);
        assert_eq!(status, 200, "{served}");
        assert_eq!(
            served,
            serde_json::to_value(
                Application
                    .investigate_snapshot(&snapshot, &request)
                    .unwrap()
            )
            .unwrap()
        );
        assert_eq!(
            served,
            serde_json::from_str::<serde_json::Value>(&cli(
                &snapshot,
                &["investigate"],
                Some(&request)
            ))
            .unwrap(),
            "the service must not reinterpret {}",
            served["query_name"]
        );

        // The resolution policy and context filter are the query layer's, not
        // the client's: narrowing them changes the served answer too.
        request.resolution_policy = ResolutionPolicy::CompleteOnly;
        let (_, complete_only) = investigated(&service, &request);
        assert_eq!(
            complete_only,
            serde_json::from_str::<serde_json::Value>(&cli(
                &snapshot,
                &["investigate"],
                Some(&request)
            ))
            .unwrap()
        );
    }
}

#[test]
fn incoming_expansion_keeps_unattributed_sites_the_client_never_infers() {
    let snapshot = snapshot();
    let (_, callers) = investigated(
        &service(&snapshot),
        &request(Investigation::Callers {
            callee: CallableSelector::by_label("b"),
        }),
    );
    let items = callers["items"].as_array().unwrap();
    // The fixture's indirect site resolves no target, so it is reported as
    // uncertainty rather than as an incoming relationship to the callee.
    assert!(
        items
            .iter()
            .any(|item| item["kind"] == "call-site" && item["unattributed"] == true)
    );
    assert!(
        items.iter().any(|item| item["kind"] == "relationship"
            && item["relationship"]["callee_display_name"] == "b")
    );
    assert_eq!(callers["direction"], "incoming");
    assert_eq!(
        callers["runtime_invocation_measure"],
        serde_json::Value::Null
    );
}

#[test]
fn explanation_handles_expand_into_the_same_evidence_as_the_cli() {
    let snapshot = snapshot();
    let service = service(&snapshot);
    let (_, callees) = investigated(
        &service,
        &request(Investigation::Callees {
            caller: CallableSelector::by_label("a"),
        }),
    );
    let handle = callees["items"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item["relationship"]["explanation_handle"].as_str())
        .expect("a relationship reports an explanation handle")
        .to_owned();

    let (status, explanation) = answered(
        &service,
        &ServiceRequest::post(
            EXPLAIN_PATH,
            serde_json::json!({"explanation_handle": handle}).to_string(),
        ),
    );
    assert_eq!(status, 200);
    assert_eq!(explanation["handle"], handle.as_str());
    assert!(
        !explanation["evidence_records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!explanation["target_claims"].as_array().unwrap().is_empty());
    assert_eq!(
        explanation,
        serde_json::from_str::<serde_json::Value>(&cli(
            &snapshot,
            &["query-snapshot", "--explain", &handle],
            None
        ))
        .unwrap()
    );

    let (status, refused) = answered(
        &service,
        &ServiceRequest::post(
            EXPLAIN_PATH,
            r#"{"explanation_handle":"explanation:not-a-handle"}"#,
        ),
    );
    assert_eq!(status, 400);
    assert_eq!(
        refused["error"],
        "unknown explanation handle 'explanation:not-a-handle'"
    );
}

#[test]
fn the_service_refuses_requests_it_cannot_answer_with_the_core_error() {
    let snapshot = snapshot();
    let service = service(&snapshot);

    let mut unknown_context = request(Investigation::CallableSearch { label: "a".into() });
    unknown_context.scope.observation_context_ids = vec![context().id];
    let mut wire = serde_json::to_value(&unknown_context).unwrap();
    wire["scope"]["observation_context_ids"] = serde_json::json!(["context:absent"]);
    let (status, refused) = answered(
        &service,
        &ServiceRequest::post(INVESTIGATE_PATH, wire.to_string()),
    );
    assert_eq!(status, 400);
    assert_eq!(
        refused["error"],
        "unknown observation context 'context:absent'"
    );

    // A generic traversal request is rejected by the same typed contract the
    // CLI reads; the service adds no query vocabulary of its own.
    let mut generic = serde_json::to_value(request(Investigation::Callees {
        caller: CallableSelector::by_label("a"),
    }))
    .unwrap();
    generic["query"]["name"] = "traverse".into();
    let (status, refused) = answered(
        &service,
        &ServiceRequest::post(INVESTIGATE_PATH, generic.to_string()),
    );
    assert_eq!(status, 400);
    assert!(refused["error"].as_str().unwrap().contains("traverse"));

    let mut unbounded = request(Investigation::Callees {
        caller: CallableSelector::by_label("a"),
    });
    unbounded.bounds.max_results = 0;
    let (status, refused) = investigated(&service, &unbounded);
    assert_eq!(status, 400);
    assert!(refused["error"].as_str().unwrap().contains("max_results"));

    let (status, _) = answered(&service, &ServiceRequest::get(INVESTIGATE_PATH));
    assert_eq!(status, 404);
    let (status, _) = answered(
        &service,
        &ServiceRequest {
            method: "DELETE".into(),
            ..ServiceRequest::get("/")
        },
    );
    assert_eq!(status, 405);
    let (status, _) = answered(
        &service,
        &ServiceRequest {
            host: Some("gloom.example.com".into()),
            ..ServiceRequest::get("/")
        },
    );
    assert_eq!(status, 400);
}

#[test]
fn the_service_exposes_no_route_that_returns_the_whole_snapshot() {
    let snapshot = snapshot();
    let service = service(&snapshot);
    for path in [
        "/snapshot",
        "/snapshot.json",
        "/program-entities",
        "/call-graph-projection",
        "/evidence-records",
        "/index.html",
        "/../snapshot.json",
    ] {
        let (status, _) = answered(&service, &ServiceRequest::get(path));
        assert_eq!(status, 404, "{path}");
    }

    let page = service.respond(&ServiceRequest::get("/"));
    assert_eq!(page.status, 200);
    assert_eq!(page.content_type, "text/html; charset=utf-8");
    let page = String::from_utf8(page.body).unwrap();
    // The page carries no snapshot content: it asks bounded questions instead.
    for entity in snapshot.program_entities() {
        assert!(!page.contains(entity.id.as_str()));
    }
    assert!(!page.contains("__SNAPSHOT_DATA__"));
    assert!(!page.contains("__GRAPH_DATA__"));
    // Nothing is loaded from anywhere but this service.
    assert!(!page.contains("https:"));
    assert!(!page.contains("<link"));
    assert!(!page.contains("script src"));
    assert!(!page.contains("<script s"));
}

#[test]
fn a_loopback_listener_answers_bounded_investigations_over_http() {
    let snapshot = snapshot();
    let bound = service(&snapshot).bind(0).unwrap();
    let address = bound.local_addr().unwrap();
    assert!(address.ip().is_loopback());
    let server = std::thread::spawn(move || {
        for _ in 0..4 {
            bound.serve_one().unwrap();
        }
    });

    let query = request(Investigation::Callees {
        caller: CallableSelector::by_label("a"),
    });
    let body = serde_json::to_string(&query).unwrap();
    let (status, _, served) = over_http(
        address,
        &format!(
            "POST {INVESTIGATE_PATH} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&served).unwrap(),
        serde_json::to_value(Application.investigate_snapshot(&snapshot, &query).unwrap()).unwrap()
    );

    let (status, head, page) = over_http(
        address,
        &format!(
            "GET / HTTP/1.1\r\nHost: localhost:{}\r\n\r\n",
            address.port()
        ),
    );
    assert_eq!(status, 200);
    assert!(page.starts_with("<!doctype html>"));
    assert!(head.contains("Content-Security-Policy: default-src 'none';"));
    assert!(head.contains("X-Content-Type-Options: nosniff"));

    // A request that reached this listener under another name is refused: a
    // page on the wider network cannot read a local snapshot through it.
    let (status, _, _) = over_http(address, "GET / HTTP/1.1\r\nHost: gloom.example.com\r\n\r\n");
    assert_eq!(status, 400);

    // A body larger than the service reads is refused before it is read.
    let (status, _, _) = over_http(
        address,
        &format!(
            "POST {INVESTIGATE_PATH} HTTP/1.1\r\nHost: {address}\r\nContent-Length: 99999999\r\n\r\n"
        ),
    );
    assert_eq!(status, 413);
    server.join().unwrap();
}

#[test]
fn the_page_asks_only_bounded_questions_and_renders_what_the_core_answers() {
    let snapshot = snapshot();
    let service = service(&snapshot);
    let (_, scope) = answered(&service, &ServiceRequest::get(SCOPE_PATH));
    let bounds = scope["default_bounds"].clone();
    let context_id = scope["build_targets"][0]["observation_contexts"][0]["id"].clone();
    // Exactly the request the page composes from its own controls: the client
    // chooses a scope and a named query, never a traversal rule.
    let compose = |query: serde_json::Value| {
        serde_json::json!({
            "scope": {"build_target": "server", "observation_context_ids": [context_id]},
            "resolution_policy": "include-possible",
            "world": {"kind": "open"},
            "bounds": bounds,
            "query": query,
        })
    };
    let answer = |request: &serde_json::Value| {
        let (status, body) = answered(
            &service,
            &ServiceRequest::post(INVESTIGATE_PATH, request.to_string()),
        );
        assert_eq!(status, 200, "{body}");
        body
    };
    let first_callable = |result: &serde_json::Value| {
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["kind"] == "callable")
            .expect("the search matched a callable")["entity_id"]
            .clone()
    };

    let search_a = compose(serde_json::json!({"name": "callable-search", "label": "a"}));
    let found_a = answer(&search_a);
    let start = first_callable(&found_a);
    let callees = compose(serde_json::json!({
        "name": "callees", "caller": {"entity_id": start, "label": null}
    }));
    let expanded = answer(&callees);
    let handle = expanded["items"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| {
            item["explanation_handle"]
                .as_str()
                .or_else(|| item["relationship"]["explanation_handle"].as_str())
        })
        .expect("a bounded neighborhood reports explanation handles")
        .to_owned();
    let (_, explanation) = answered(
        &service,
        &ServiceRequest::post(
            EXPLAIN_PATH,
            serde_json::json!({"explanation_handle": handle}).to_string(),
        ),
    );
    let search_c = compose(serde_json::json!({"name": "callable-search", "label": "c"}));
    let found_c = answer(&search_c);
    let path = compose(serde_json::json!({
        "name": "call-path",
        "start": {"entity_id": start, "label": null},
        "end": {"entity_id": first_callable(&found_c), "label": null},
    }));
    let traced = answer(&path);
    assert!(
        traced["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["kind"] == "path"),
        "the fixture has a path to draw"
    );
    let callee = expanded["items"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| item["relationship"]["callee_display_name"].as_str())
        .expect("the neighborhood names a callee")
        .to_owned();

    let input = serde_json::json!({
        "html": String::from_utf8(service.respond(&ServiceRequest::get("/")).body).unwrap(),
        "program_snapshot_id": scope["program_snapshot_id"],
        "build_target_count": scope["build_targets"].as_array().unwrap().len(),
        "observation_context_count": scope["build_targets"][0]["observation_contexts"]
            .as_array().unwrap().len(),
        "searches": ["a", "c"],
        "expected_callee": callee,
        "expected_evidence": explanation["evidence_records"][0]["id"],
        "exchanges": [
            {"path": SCOPE_PATH, "response": scope},
            {"path": INVESTIGATE_PATH, "request": search_a, "response": found_a},
            {"path": INVESTIGATE_PATH, "request": callees, "response": expanded},
            {"path": EXPLAIN_PATH,
             "request": {"explanation_handle": handle}, "response": explanation},
            {"path": INVESTIGATE_PATH, "request": search_c, "response": found_c},
            {"path": INVESTIGATE_PATH, "request": path, "response": traced},
        ],
    });
    let mut child = std::process::Command::new("node")
        .arg("tests/support/local-viewer.cjs")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .expect("Node.js is required to exercise the served viewer page");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
}

/// Speak HTTP/1.1 to the service directly, so the test depends on no client.
fn over_http(address: std::net::SocketAddr, request: &str) -> (u16, String, String) {
    let mut stream = TcpStream::connect(address).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    stream.flush().unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("a complete response");
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("a status line");
    (status, head.to_owned(), body.to_owned())
}
