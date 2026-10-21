//! Restricted HTTP GET sources: exact origins and refused redirects, against
//! loopback test servers only.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_restricted_http_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(path.join("out")).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Serves one canned response to the first connection within three seconds.
struct TestServer {
    origin: String,
    handle: JoinHandle<bool>,
}

impl TestServer {
    fn spawn(head: String, body: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            return false;
                        }
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("test server accept failed: {error}"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
            }
            let _ = stream.write_all(
                format!(
                    "{head}Content-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            );
            let _ = stream.write_all(&body);
            true
        });
        Self { origin, handle }
    }

    fn ok(body: Vec<u8>) -> Self {
        Self::spawn(
            "HTTP/1.1 200 OK\r\nContent-Type: application/xml\r\n".into(),
            body,
        )
    }

    /// Whether a client connected before the deadline.
    fn connected(self) -> bool {
        self.handle.join().unwrap()
    }
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/orders")
}

fn orders() -> Vec<u8> {
    std::fs::read(fixture_dir().join("Orders.xml")).unwrap()
}

fn run(dir: &TestDir, url: &str, origins: &[&str]) -> anyhow::Result<cli::RunOutcome> {
    let mut project: mapping::Project =
        serde_json::from_str(&std::fs::read_to_string(fixture_dir().join("project.json")).unwrap())
            .unwrap();
    project.source_path = Some(url.into());
    let project_path = dir.0.join("project.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project).unwrap()).unwrap();
    let mut policy = cli::RestrictedPolicy::new(cli::ExecutionBudgets::new(1_000_000, 512, 0))
        .allow_root(dir.0.join("out"), cli::RootAccess::ReadWrite)
        .unwrap();
    for origin in origins {
        policy = policy.allow_origin(origin).unwrap();
    }
    let policy = cli::ExecutionPolicy::Restricted(policy);
    let output = dir.0.join("out/output.json");
    cli::run_project_with_options(
        &project_path,
        &cli::RunOptions::new()
            .with_output_path(&output)
            .with_execution_policy(&policy),
    )
}

fn refusal(error: &anyhow::Error) -> &cli::PolicyRefusal {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<cli::PolicyRefusal>())
        .unwrap_or_else(|| panic!("no policy refusal in {error:#}"))
}

#[test]
fn listed_origins_are_fetched_and_others_are_refused_before_connecting() {
    let dir = TestDir::new("origins");
    let server = TestServer::ok(orders());
    let url = format!("{}/orders.xml", server.origin);
    let outcome = run(&dir, &url, &[&server.origin]).unwrap();
    assert_eq!(outcome.records_written, 6);
    assert!(server.connected());

    let server = TestServer::ok(orders());
    let url = format!("{}/orders.xml", server.origin);
    // Same port under another host name, and the same host over HTTPS.
    let other_host = server.origin.replace("127.0.0.1", "localhost");
    let https = server.origin.replace("http://", "https://");
    let error = run(&dir, &url, &[&other_host, &https]).unwrap_err();
    assert!(matches!(
        refusal(&error),
        cli::PolicyRefusal::HttpOriginDenied { origin } if url.starts_with(origin.as_str())
    ));
    assert!(
        !server.connected(),
        "a refused origin must not be contacted"
    );
}

#[test]
fn redirects_are_refused_without_contacting_their_target() {
    let dir = TestDir::new("redirect");
    let target = TestServer::ok(orders());
    let redirect = TestServer::spawn(
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {}/orders.xml\r\n",
            target.origin
        ),
        Vec::new(),
    );
    let url = format!("{}/start.xml", redirect.origin);
    // Even listing the redirect target does not admit following the redirect.
    let error = run(&dir, &url, &[&redirect.origin, &target.origin]).unwrap_err();
    assert_eq!(
        refusal(&error),
        &cli::PolicyRefusal::UnsupportedCapability {
            capability: cli::RestrictedCapability::HttpRedirect
        }
    );
    assert!(redirect.connected());
    assert!(
        !target.connected(),
        "the redirect target must not be contacted"
    );
    assert!(!dir.0.join("out/output.json").exists());
}
