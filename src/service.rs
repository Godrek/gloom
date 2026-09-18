//! A loopback-only query service that answers bounded investigations for a
//! local viewer.
//!
//! The service pins one published snapshot and exposes exactly three things: the
//! scope a person selects within, the bounded named queries the CLI runs, and
//! the expansion of one explanation handle. It deliberately has no endpoint that
//! returns the snapshot, its projection, or any unbounded listing, so a client
//! cannot obtain the program snapshot and reinterpret it. Every analysis
//! decision stays behind `Application`.
//!
//! The transport is a small hand-written HTTP/1.1 reader and writer over
//! `std::net::TcpListener`. It adds no dependency, so the claim that the local
//! workflow performs no network access remains auditable: the listener binds the
//! loopback address, requests naming another host are refused, and the served
//! page declares a content security policy that forbids loading anything.

use crate::app::Application;
use crate::snapshot::PublishedSnapshot;
use serde::Deserialize;
use serde::Serialize;
use std::io::{BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// The viewer page, served from the executable rather than from a directory, so
/// serving a snapshot never also serves the files around it.
const VIEWER_PAGE: &str = include_str!("../assets/local-viewer.html");

/// The selectable build targets, observation contexts, and bounds limits.
pub const SCOPE_PATH: &str = "/investigation-scope";
/// One `BoundedQuery`, the same typed request `gloom investigate` reads.
pub const INVESTIGATE_PATH: &str = "/investigate";
/// One explanation handle reported by a previous result.
pub const EXPLAIN_PATH: &str = "/explain";

/// Request line and headers are read within this budget before any routing.
const MAX_HEAD_BYTES: usize = 8 * 1024;
/// A bounded query request is small; anything larger is refused unread.
const MAX_BODY_BYTES: usize = 256 * 1024;
const MAX_HEADERS: usize = 64;
/// The total time one request may take to arrive.
///
/// A per-read timeout does not bound this: a client that sends one byte just
/// inside each read's timeout keeps every individual read succeeding and holds
/// the single-threaded accept loop for as long as it likes. The budget is
/// therefore spent across a whole request, not restarted by each byte.
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(15);
/// How long writing one response may block on a client that stops reading.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(15);

/// Forbids every external load, so the page cannot acquire code or data from a
/// network even if one is reachable. Inline style and script are the page's own.
const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'unsafe-inline'; \
     style-src 'unsafe-inline'; connect-src 'self'; form-action 'none'; base-uri 'none'";

/// One parsed HTTP request, reduced to what routing needs.
#[derive(Clone, Debug)]
pub struct ServiceRequest {
    pub method: String,
    pub path: String,
    pub host: Option<String>,
    pub body: Vec<u8>,
}

impl ServiceRequest {
    pub fn get(path: &str) -> Self {
        Self {
            method: "GET".into(),
            path: path.into(),
            host: Some("127.0.0.1".into()),
            body: Vec::new(),
        }
    }

    pub fn post(path: &str, body: impl Into<Vec<u8>>) -> Self {
        Self {
            method: "POST".into(),
            path: path.into(),
            host: Some("127.0.0.1".into()),
            body: body.into(),
        }
    }

    /// A service bound to the loopback interface answers only requests that
    /// address it there. A request naming another host reached this listener
    /// through a name that resolves to it, which is how a page on the wider
    /// network would try to read a developer's snapshot.
    fn addresses_loopback(&self) -> bool {
        let Some(host) = self.host.as_deref() else {
            return false;
        };
        let host = match host.strip_prefix('[') {
            Some(rest) => match rest.split_once(']') {
                Some((address, _)) => address,
                None => return false,
            },
            None => host.split(':').next().unwrap_or_default(),
        };
        matches!(host, "localhost" | "127.0.0.1" | "::1")
    }
}

/// One HTTP response, complete before any byte is written.
#[derive(Clone, Debug)]
pub struct ServiceResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

#[derive(Serialize)]
struct RefusedRequest<'a> {
    error: &'a str,
}

impl ServiceResponse {
    fn html(status: u16, body: &str) -> Self {
        Self {
            status,
            content_type: "text/html; charset=utf-8",
            body: body.as_bytes().to_vec(),
        }
    }

    fn json(status: u16, value: &impl Serialize) -> Self {
        match serde_json::to_vec(value) {
            Ok(body) => Self {
                status,
                content_type: "application/json",
                body,
            },
            // A published snapshot serializes; a failure here is the service's,
            // not the request's, and is reported as such rather than panicking.
            Err(error) => Self::refused(500, &error.to_string()),
        }
    }

    /// A refusal carries the same message the CLI prints, so a viewer never
    /// paraphrases a query error into its own vocabulary.
    fn refused(status: u16, error: &str) -> Self {
        Self::json(status, &RefusedRequest { error })
    }

    fn reason(&self) -> &'static str {
        match self.status {
            200 => "OK",
            400 => "Bad Request",
            404 => "Not Found",
            405 => "Method Not Allowed",
            408 => "Request Timeout",
            411 => "Length Required",
            413 => "Content Too Large",
            _ => "Internal Server Error",
        }
    }

    fn write(&self, stream: &mut TcpStream) -> std::io::Result<()> {
        let head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\n\
             X-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\
             Content-Security-Policy: {}\r\nConnection: close\r\n\r\n",
            self.status,
            self.reason(),
            self.content_type,
            self.body.len(),
            CONTENT_SECURITY_POLICY,
        );
        stream.write_all(head.as_bytes())?;
        stream.write_all(&self.body)?;
        stream.flush()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExplanationRequest {
    explanation_handle: String,
}

/// A local query service over one pinned published snapshot.
///
/// The snapshot is immutable for the life of the service, so every answer a
/// viewer assembles describes one program snapshot.
pub struct LocalQueryService {
    application: Application,
    snapshot: PublishedSnapshot,
}

impl LocalQueryService {
    pub fn new(snapshot: PublishedSnapshot) -> Self {
        Self {
            application: Application,
            snapshot,
        }
    }

    /// Answer one request. Routing is exhaustive: an unlisted path is refused
    /// rather than resolved against the filesystem.
    pub fn respond(&self, request: &ServiceRequest) -> ServiceResponse {
        if !request.addresses_loopback() {
            return ServiceResponse::refused(
                400,
                "the local query service answers only requests addressed to its loopback host",
            );
        }
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/") => ServiceResponse::html(200, VIEWER_PAGE),
            ("GET", SCOPE_PATH) => {
                ServiceResponse::json(200, &self.application.investigation_scope(&self.snapshot))
            }
            ("POST", INVESTIGATE_PATH) => self.investigate(&request.body),
            ("POST", EXPLAIN_PATH) => self.explain(&request.body),
            ("GET", _) | ("POST", _) => ServiceResponse::refused(
                404,
                "the local query service exposes only the viewer page, investigation scope, \
                 bounded investigations, and explanation expansion",
            ),
            _ => ServiceResponse::refused(
                405,
                "use GET for the viewer page and scope, POST for investigations and explanations",
            ),
        }
    }

    fn investigate(&self, body: &[u8]) -> ServiceResponse {
        let request: crate::queries::BoundedQuery = match serde_json::from_slice(body) {
            Ok(request) => request,
            Err(error) => return ServiceResponse::refused(400, &error.to_string()),
        };
        match self
            .application
            .investigate_snapshot(&self.snapshot, &request)
        {
            Ok(result) => ServiceResponse::json(200, &result),
            Err(error) => ServiceResponse::refused(400, &error),
        }
    }

    fn explain(&self, body: &[u8]) -> ServiceResponse {
        let request: ExplanationRequest = match serde_json::from_slice(body) {
            Ok(request) => request,
            Err(error) => return ServiceResponse::refused(400, &error.to_string()),
        };
        match self
            .application
            .expand_explanation(&self.snapshot, &request.explanation_handle)
        {
            Ok(explanation) => ServiceResponse::json(200, &explanation),
            Err(error) => ServiceResponse::refused(400, &error),
        }
    }

    /// Listen on the loopback interface only. The address is not a parameter:
    /// a snapshot served to a wider network is a different decision than this
    /// service makes.
    pub fn bind(self, port: u16) -> Result<BoundLocalQueryService, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .map_err(|error| format!("127.0.0.1:{port}: {error}"))?;
        Ok(BoundLocalQueryService {
            service: self,
            listener,
            request_deadline: REQUEST_DEADLINE,
        })
    }
}

/// A local query service listening on a loopback port.
pub struct BoundLocalQueryService {
    service: LocalQueryService,
    listener: TcpListener,
    request_deadline: Duration,
}

impl BoundLocalQueryService {
    /// Cap the total time one request may occupy the service at something
    /// other than `REQUEST_DEADLINE`.
    pub fn with_request_deadline(mut self, deadline: Duration) -> Self {
        self.request_deadline = deadline;
        self
    }

    pub fn local_addr(&self) -> Result<SocketAddr, String> {
        self.listener.local_addr().map_err(|e| e.to_string())
    }

    /// Serve until the listener fails. One connection carries one request, and
    /// every request has a total time budget, so no client can hold the loop.
    pub fn serve(&self) -> Result<(), String> {
        loop {
            match self.listener.accept() {
                Ok((stream, _)) => self.serve_connection(stream),
                Err(error) => return Err(error.to_string()),
            }
        }
    }

    /// Accept and answer exactly one connection.
    pub fn serve_one(&self) -> Result<(), String> {
        let (stream, _) = self.listener.accept().map_err(|e| e.to_string())?;
        self.serve_connection(stream);
        Ok(())
    }

    fn serve_connection(&self, mut stream: TcpStream) {
        let _ = stream.set_write_timeout(Some(RESPONSE_TIMEOUT));
        let response =
            match read_request(&mut stream, Deadline::starting_now(self.request_deadline)) {
                Ok(request) => self.service.respond(&request),
                Err(refusal) => refusal,
            };
        let _ = response.write(&mut stream);
        let _ = stream.shutdown(Shutdown::Both);
    }
}

/// The instant by which one whole request must have arrived.
struct Deadline(std::time::Instant);

impl Deadline {
    fn starting_now(budget: Duration) -> Self {
        Self(std::time::Instant::now() + budget)
    }

    /// Give the next blocking read only the time the request has left, so no
    /// single read can outlive the budget and no sequence of reads can renew it.
    fn arm(&self, stream: &TcpStream) -> Result<(), ServiceResponse> {
        let remaining = self.0.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() || stream.set_read_timeout(Some(remaining)).is_err() {
            return Err(ServiceResponse::refused(
                408,
                "the request did not arrive within the local query service deadline",
            ));
        }
        Ok(())
    }
}

/// A read that ran out of time is a deadline refusal, not a malformed request.
fn incomplete(error: std::io::Error, malformed: &str) -> ServiceResponse {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => ServiceResponse::refused(
            408,
            "the request did not arrive within the local query service deadline",
        ),
        _ => ServiceResponse::refused(400, malformed),
    }
}

/// Read one request line by line within an explicit byte budget and deadline.
fn read_line(
    reader: &mut BufReader<&mut TcpStream>,
    budget: &mut usize,
    deadline: &Deadline,
) -> Result<String, ServiceResponse> {
    let mut line = Vec::new();
    loop {
        if *budget == 0 {
            return Err(ServiceResponse::refused(
                413,
                "request head exceeds the local query service limit",
            ));
        }
        // Only an empty buffer means the next byte has to come off the socket,
        // so the deadline is re-armed exactly when a read could block.
        if reader.buffer().is_empty() {
            deadline.arm(reader.get_ref())?;
        }
        let mut byte = [0u8; 1];
        if let Err(error) = reader.read_exact(&mut byte) {
            return Err(incomplete(error, "incomplete request"));
        }
        *budget -= 1;
        if byte[0] == b'\n' {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return String::from_utf8(line)
                .map_err(|_| ServiceResponse::refused(400, "request head is not valid UTF-8"));
        }
        line.push(byte[0]);
    }
}

fn read_request(
    stream: &mut TcpStream,
    deadline: Deadline,
) -> Result<ServiceRequest, ServiceResponse> {
    let mut reader = BufReader::new(stream);
    let mut budget = MAX_HEAD_BYTES;
    let start = read_line(&mut reader, &mut budget, &deadline)?;
    let mut parts = start.split(' ');
    let (Some(method), Some(target), Some(_)) = (parts.next(), parts.next(), parts.next()) else {
        return Err(ServiceResponse::refused(400, "malformed request line"));
    };
    let mut host = None;
    let mut content_length = 0usize;
    let mut headers = 0usize;
    loop {
        let line = read_line(&mut reader, &mut budget, &deadline)?;
        if line.is_empty() {
            break;
        }
        headers += 1;
        if headers > MAX_HEADERS {
            return Err(ServiceResponse::refused(413, "too many request headers"));
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(ServiceResponse::refused(400, "malformed request header"));
        };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "host" => host = Some(value.to_owned()),
            // Chunked bodies are not read, so a request that announces one is
            // refused rather than partially interpreted.
            "transfer-encoding" => {
                return Err(ServiceResponse::refused(
                    411,
                    "send a request body with an explicit Content-Length",
                ));
            }
            "content-length" => {
                content_length = value
                    .parse()
                    .map_err(|_| ServiceResponse::refused(400, "malformed Content-Length"))?;
                if content_length > MAX_BODY_BYTES {
                    return Err(ServiceResponse::refused(
                        413,
                        "request body exceeds the local query service limit",
                    ));
                }
            }
            _ => {}
        }
    }
    // Read in chunks rather than with `read_exact`, which would loop over
    // blocking reads of its own and so escape the request's time budget.
    let mut body = vec![0u8; content_length];
    let mut filled = 0;
    while filled < content_length {
        if reader.buffer().is_empty() {
            deadline.arm(reader.get_ref())?;
        }
        match reader.read(&mut body[filled..]) {
            Ok(0) => return Err(ServiceResponse::refused(400, "incomplete request body")),
            Ok(read) => filled += read,
            Err(error) => return Err(incomplete(error, "incomplete request body")),
        }
    }
    Ok(ServiceRequest {
        method: method.to_owned(),
        // A query string selects nothing here: every request states its
        // selection in a typed body.
        path: target.split('?').next().unwrap_or_default().to_owned(),
        host,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(host: Option<&str>) -> ServiceRequest {
        ServiceRequest {
            method: "GET".into(),
            path: "/".into(),
            host: host.map(str::to_owned),
            body: Vec::new(),
        }
    }

    #[test]
    fn only_loopback_hosts_address_the_local_service() {
        for host in ["127.0.0.1", "127.0.0.1:7878", "localhost:7878", "[::1]:80"] {
            assert!(request(Some(host)).addresses_loopback(), "{host}");
        }
        for host in [
            "gloom.example.com",
            "gloom.example.com:7878",
            "127.0.0.1.example.com",
            "[::1",
            "",
        ] {
            assert!(!request(Some(host)).addresses_loopback(), "{host}");
        }
        assert!(!request(None).addresses_loopback());
    }

    #[test]
    fn the_served_page_loads_nothing_from_a_network() {
        // The page vendors every byte it needs. Its one absolute URI is the SVG
        // namespace name, which identifies an XML vocabulary and is never
        // fetched, so every other occurrence of a scheme would be a load.
        const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
        assert_eq!(
            VIEWER_PAGE.matches("http").count(),
            VIEWER_PAGE.matches(SVG_NAMESPACE).count()
        );
        assert!(!VIEWER_PAGE.contains("//cdn"));
        assert!(!VIEWER_PAGE.contains("@import"));
    }
}
