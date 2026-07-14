use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAX_HTTP_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/orders")
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_http_get_{name}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct TestServer {
    url: String,
    handle: JoinHandle<Vec<u8>>,
}

impl TestServer {
    fn spawn(path_and_query: &str, status: &str, body: Vec<u8>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let status = status.to_string();
        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let (mut stream, _) = loop {
                match listener.accept() {
                    Ok(connection) => break connection,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(
                            Instant::now() < deadline,
                            "client did not connect to test server"
                        );
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("test server accept failed: {error}"),
                }
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                assert!(request.len() <= 64 * 1024, "request headers are too large");
            }

            let header = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&body);
            request
        });
        Self {
            url: format!("http://{address}{path_and_query}"),
            handle,
        }
    }

    fn join(self) -> String {
        let request = self.handle.join().unwrap();
        String::from_utf8(request).unwrap()
    }
}

fn orders_project() -> mapping::Project {
    serde_json::from_str(&std::fs::read_to_string(fixture_dir().join("project.json")).unwrap())
        .unwrap()
}

fn write_project(
    dir: &Path,
    project: &mapping::Project,
    http_timeout_seconds: Option<u16>,
) -> PathBuf {
    let mut value = serde_json::to_value(project).unwrap();
    if let Some(timeout) = http_timeout_seconds {
        value["source_options"]["http_get"] = serde_json::json!({
            "timeout_seconds": timeout
        });
    }
    let path = dir.join("project.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    path
}

fn configured_project(source_path: &str, output_path: &Path) -> mapping::Project {
    let mut project = orders_project();
    project.source_path = Some(source_path.to_string());
    project.target_path = Some(output_path.to_string_lossy().into_owned());
    project
}

fn assert_get_request(request: &str, expected_target: &str) {
    let lower = request.to_ascii_lowercase();
    assert!(
        request.starts_with(&format!("GET {expected_target} HTTP/1.1\r\n")),
        "{request}"
    );
    assert!(lower.contains("user-agent: ferrule/"), "{request}");
    assert!(!lower.contains("content-length:"), "{request}");
    assert!(!lower.contains("transfer-encoding:"), "{request}");
}

#[test]
fn stored_http_source_executes_an_xml_mapping() {
    let server = TestServer::spawn(
        "/orders.xml",
        "200 OK",
        std::fs::read(fixture_dir().join("Orders.xml")).unwrap(),
    );
    let dir = TestDir::new("success");
    let output = dir.path().join("output.json");
    let project = configured_project(&server.url, &output);
    let project_path = write_project(dir.path(), &project, Some(5));

    let outcome = cli::run_project_with_paths(&project_path, None, None).unwrap();
    let request = server.join();

    assert_eq!(outcome.records_written, 6);
    assert_eq!(
        outcome.input_path,
        PathBuf::from(&project.source_path.unwrap())
    );
    let rows: serde_json::Value =
        serde_json::from_slice(&std::fs::read(outcome.output_path).unwrap()).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 6);
    assert_get_request(&request, "/orders.xml");
}

#[test]
fn explicit_local_input_overrides_a_stored_http_source() {
    let dir = TestDir::new("local_override");
    let output = dir.path().join("output.json");
    let project = configured_project("http://127.0.0.1:9/not-requested", &output);
    let project_path = write_project(dir.path(), &project, Some(5));

    let outcome =
        cli::run_project_with_paths(&project_path, Some(&fixture_dir().join("Orders.xml")), None)
            .unwrap();

    assert_eq!(outcome.records_written, 6);
    assert_eq!(outcome.input_path, fixture_dir().join("Orders.xml"));
}

#[test]
fn stored_http_output_is_rejected_before_reading_input() {
    let dir = TestDir::new("remote_output");
    let mut project = configured_project("missing.xml", Path::new("unused.xml"));
    project.target_path = Some("https://example.invalid/output.xml".to_string());
    let project_path = write_project(dir.path(), &project, None);

    let error = cli::run_project_with_paths(&project_path, None, None).unwrap_err();

    assert!(
        error
            .to_string()
            .contains("HTTP output URLs are not supported"),
        "{error:#}"
    );
}

#[test]
fn explicit_http_input_overrides_a_stored_local_source() {
    let server = TestServer::spawn(
        "/override.xml",
        "200 OK",
        std::fs::read(fixture_dir().join("Orders.xml")).unwrap(),
    );
    let dir = TestDir::new("http_override");
    let output = dir.path().join("output.json");
    let project = configured_project("missing.xml", &output);
    let project_path = write_project(dir.path(), &project, Some(5));
    let input_url = server.url.clone();

    let outcome =
        cli::run_project_with_paths(&project_path, Some(Path::new(&input_url)), Some(&output))
            .unwrap();
    let request = server.join();

    assert_eq!(outcome.records_written, 6);
    assert_eq!(outcome.input_path, PathBuf::from(input_url));
    assert_get_request(&request, "/override.xml");
}

#[test]
fn http_status_error_omits_the_url_query() {
    let server = TestServer::spawn("/orders.xml?token=private", "503 Unavailable", Vec::new());
    let dir = TestDir::new("status");
    let project = configured_project(&server.url, &dir.path().join("output.json"));
    let project_path = write_project(dir.path(), &project, Some(5));

    let error = cli::run_project_with_paths(&project_path, None, None).unwrap_err();
    let request = server.join();
    let message = format!("{error:#}");

    assert!(message.contains("returned status 503"), "{message}");
    assert!(message.contains("/orders.xml"), "{message}");
    assert!(!message.contains("private"), "{message}");
    assert_get_request(&request, "/orders.xml?token=private");
}

#[test]
fn oversized_http_response_is_rejected_before_xml_parsing() {
    let server = TestServer::spawn(
        "/large.xml",
        "200 OK",
        vec![b'x'; MAX_HTTP_RESPONSE_BYTES + 1],
    );
    let dir = TestDir::new("oversized");
    let project = configured_project(&server.url, &dir.path().join("output.json"));
    let project_path = write_project(dir.path(), &project, Some(5));

    let error = cli::run_project_with_paths(&project_path, None, None).unwrap_err();
    let request = server.join();
    let message = format!("{error:#}");

    assert!(message.contains("response exceeded 8 MiB"), "{message}");
    assert_get_request(&request, "/large.xml");
}

#[test]
fn non_utf8_http_response_is_rejected() {
    let server = TestServer::spawn(
        "/invalid-utf8.xml",
        "200 OK",
        b"<Orders>\xff</Orders>".to_vec(),
    );
    let dir = TestDir::new("invalid_utf8");
    let project = configured_project(&server.url, &dir.path().join("output.json"));
    let project_path = write_project(dir.path(), &project, Some(5));

    let error = cli::run_project_with_paths(&project_path, None, None).unwrap_err();
    let request = server.join();
    let message = format!("{error:#}");

    assert!(message.contains("response that is not UTF-8"), "{message}");
    assert_get_request(&request, "/invalid-utf8.xml");
}

#[test]
fn malformed_xml_http_response_has_remote_context() {
    let server = TestServer::spawn("/malformed.xml", "200 OK", b"<Orders>".to_vec());
    let dir = TestDir::new("malformed_xml");
    let project = configured_project(&server.url, &dir.path().join("output.json"));
    let project_path = write_project(dir.path(), &project, Some(5));

    let error = cli::run_project_with_paths(&project_path, None, None).unwrap_err();
    let request = server.join();
    let message = format!("{error:#}");

    assert!(
        message.contains("parsing XML response from HTTP GET"),
        "{message}"
    );
    assert!(message.contains("/malformed.xml"), "{message}");
    assert_get_request(&request, "/malformed.xml");
}

#[test]
fn stored_http_extra_source_is_loaded() {
    use ir::{ScalarType, SchemaNode};
    use mapping::{FormatOptions, NamedSource};

    let server = TestServer::spawn(
        "/catalog.xml",
        "200 OK",
        b"<Catalog><Version>v1</Version></Catalog>".to_vec(),
    );
    let dir = TestDir::new("extra_source");
    std::fs::copy(
        fixture_dir().join("Orders.xml"),
        dir.path().join("Orders.xml"),
    )
    .unwrap();
    let mut project = configured_project("Orders.xml", &dir.path().join("output.json"));
    project.extra_sources.push(NamedSource {
        name: "Catalog".into(),
        path: server.url.clone(),
        schema: SchemaNode::group(
            "Catalog",
            vec![SchemaNode::scalar("Version", ScalarType::String)],
        ),
        options: FormatOptions::default(),
    });
    let project_path = write_project(dir.path(), &project, None);

    let outcome = cli::run_project_with_paths(&project_path, None, None).unwrap();
    let request = server.join();

    assert_eq!(outcome.records_written, 6);
    assert_get_request(&request, "/catalog.xml");
}
