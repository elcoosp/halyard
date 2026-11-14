//! The real-transport loopback lane (§15.1 mock-forge server, E2E):
//! `UreqTransport` driven against a `std::net` HTTP/1.1 server — no
//! sockets mocked, no TLS on the loopback, no new dependencies. The
//! exchange pins what a scripted test cannot: the real client sends the
//! configured method/headers/body and maps statuses and connection
//! failures exactly like the seam contract requires. House rules apply
//! even here: no indexing/slicing outside `.get()` (the restriction
//! lint has no test allowance), no `expect` outside `#[test]` bodies.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

use halyard_forge::ForgeError;
use halyard_forge::transport::{HttpRequest, HttpTransport};
use halyard_forge_github::UreqTransport;

type BoxError = Box<dyn std::error::Error>;

/// Spawns a one-shot HTTP server that echoes the salient parts of the
/// next request it receives (method, path, body, the mandatory GitHub
/// headers) back as a JSON body; returns the base URL and the thread's
/// completion signal. The echo riding the response body is the test's
/// assertion surface.
fn serve_echo() -> Result<(String, thread::JoinHandle<()>), BoxError> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let handle = thread::spawn(move || {
        if let Ok((mut stream, _addr)) = listener.accept() {
            let _ = serve_exchange(&mut stream);
        }
    });
    Ok((format!("http://127.0.0.1:{port}"), handle))
}

/// Reads one HTTP/1.1 request (headers + Content-Length body), writes
/// the echo response. Errors are swallowed by the caller's thread (the
/// client-side assertions carry the test).
fn serve_exchange(stream: &mut TcpStream) -> Result<(), BoxError> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0_u8; 2048];
    let mut header_end = None;
    while header_end.is_none() {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(());
        }
        if let Some(slice) = chunk.get(..read) {
            buf.extend_from_slice(slice);
        }
        header_end = buf.windows(4).position(|window| window == b"\r\n\r\n");
    }
    let header_end = header_end.unwrap_or_default();
    let head = buf.get(..header_end).map_or(String::new(), |bytes| {
        String::from_utf8_lossy(bytes).into_owned()
    });
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or_default().to_string();
    let mut parts = request_line.split(' ');
    let method = parts.next().unwrap_or_default().to_owned();
    let path = parts.next().unwrap_or_default().to_owned();
    let mut content_length = 0_usize;
    let mut authorization = None;
    let mut accept = None;
    let mut api_version = None;
    let mut user_agent = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_ascii_lowercase();
        let value = value.trim().to_owned();
        match name.as_str() {
            "content-length" => content_length = value.parse().unwrap_or(0),
            "authorization" => authorization = Some(value),
            "accept" => accept = Some(value),
            "x-github-api-version" => api_version = Some(value),
            "user-agent" => user_agent = Some(value),
            _ => {}
        }
    }
    let body_start = header_end + 4;
    let body_end = body_start.saturating_add(content_length);
    while buf.len() < body_end {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if let Some(slice) = chunk.get(..read) {
            buf.extend_from_slice(slice);
        }
    }
    let body = buf
        .get(body_start..body_end)
        .map_or(String::new(), |bytes| {
            String::from_utf8_lossy(bytes).into_owned()
        });
    let echo = serde_json::json!({
        "method": method,
        "path": path,
        "body": body,
        "authorization": authorization,
        "accept": accept,
        "api_version": api_version,
        "user_agent": user_agent,
    });
    let payload = serde_json::to_string(&echo)?;
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        payload.len(),
        payload
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()?;
    Ok(())
}

/// A field of the echo document as a string (missing fields render
/// empty — the assertions below make the difference visible).
fn field(echo: &serde_json::Value, name: &str) -> String {
    echo.get(name)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The real client sends the configured headers verbatim (research §1):
/// the adapter always supplies the `halyard/<version>` User-Agent
/// (pinned in the contract tests over the scripted transport); here the
/// raw request supplies one so the pass-through is what's asserted —
/// without it the client would inject its own default (`ureq/x.y`), a
/// GitHub-visible shape the adapter must never rely on.
#[test]
fn loopback_request_carries_the_github_headers() -> Result<(), BoxError> {
    let (base, server) = serve_echo()?;
    let transport = UreqTransport::new();
    let request = HttpRequest {
        method: "GET",
        url: format!("{base}/repos/octo/halyard-demo/pulls?state=open"),
        headers: vec![
            (
                "Accept".to_owned(),
                "application/vnd.github+json".to_owned(),
            ),
            ("X-GitHub-Api-Version".to_owned(), "2022-11-28".to_owned()),
            ("Authorization".to_owned(), "Bearer t".to_owned()),
            ("User-Agent".to_owned(), "halyard/0.0.0".to_owned()),
        ],
        body: None,
    };
    let response = transport.execute(request)?;
    server.join().expect("the server thread completes");
    assert_eq!(response.status, 200);
    let echo: serde_json::Value = serde_json::from_str(&response.body)?;
    assert_eq!(field(&echo, "method"), "GET");
    assert_eq!(
        field(&echo, "path"),
        "/repos/octo/halyard-demo/pulls?state=open"
    );
    assert_eq!(field(&echo, "accept"), "application/vnd.github+json");
    assert_eq!(field(&echo, "api_version"), "2022-11-28");
    assert_eq!(field(&echo, "authorization"), "Bearer t");
    assert_eq!(
        field(&echo, "user_agent"),
        "halyard/0.0.0",
        "the configured User-Agent passes through verbatim"
    );
    Ok(())
}

/// A POST with a JSON body round-trips byte-identically through the
/// real client.
#[test]
fn loopback_post_roundtrips_the_body() -> Result<(), BoxError> {
    let (base, server) = serve_echo()?;
    let transport = UreqTransport::new();
    let body = "{\"head\":\"halyard/release/main\",\"base\":\"main\"}".to_owned();
    let request = HttpRequest {
        method: "POST",
        url: format!("{base}/repos/octo/halyard-demo/pulls"),
        headers: vec![("Content-Type".to_owned(), "application/json".to_owned())],
        body: Some(body.clone()),
    };
    let response = transport.execute(request)?;
    server.join().expect("the server thread completes");
    assert_eq!(response.status, 200);
    let echo: serde_json::Value = serde_json::from_str(&response.body)?;
    assert_eq!(field(&echo, "method"), "POST");
    assert_eq!(
        field(&echo, "body"),
        body,
        "the body round-trips byte-identically"
    );
    Ok(())
}

/// A refused connection is a transport error with the endpoint named —
/// the fail-closed surface the machine maps to exit 1.
#[test]
fn refused_connection_is_a_transport_error() {
    // A bind-then-drop listener is the reliable "closed port" on the
    // loopback; nothing is listening by the time the client connects.
    let listener = TcpListener::bind("127.0.0.1:0")
        .unwrap_or_else(|error| unreachable!("loopback bind: {error}"));
    let port = listener.local_addr().expect("bound").port();
    drop(listener);
    let transport = UreqTransport::new();
    let request = HttpRequest {
        method: "GET",
        url: format!("http://127.0.0.1:{port}/repos/octo/halyard-demo"),
        headers: Vec::new(),
        body: None,
    };
    let err = transport
        .execute(request)
        .expect_err("nothing is listening");
    assert_eq!(err.code(), ForgeError::TRANSPORT_CODE, "{err}");
    assert!(
        err.to_string().contains("127.0.0.1"),
        "the reason names the endpoint: {err}"
    );
}
