use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct Files {
    root: PathBuf,
    project: PathBuf,
    request: PathBuf,
    headers: PathBuf,
    stored_output: PathBuf,
}

impl Files {
    fn new(url: &str, method: &str, body: Option<&[u8]>) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ferrule_rest_json_cli_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let project = root.join("project.json");
        let request = root.join("request.json");
        let headers = root.join("protected-headers.json");
        let stored_output = root.join("stored-output.json");
        std::fs::write(&stored_output, b"original-output").unwrap();
        let mut value = serde_json::json!({
            "source":{"name":"Input","kind":{"kind":"group","children":[{"name":"Message","kind":{"kind":"scalar","ty":"string"}}]}},
            "target":{"name":"Result","kind":{"kind":"group","children":[{"name":"Echo","kind":{"kind":"scalar","ty":"string"}}]}},
            "source_path":"never-read-source.json", "target_path":stored_output.to_str().unwrap(),
            "graph":{"nodes":{"0":{"kind":"source_field","path":["Message"]}}},
            "root":{"bindings":[{"target_field":"Echo","node":0}]}
        });
        std::fs::write(&project, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        value = serde_json::json!({"url":url,"method":method,"timeout_seconds":2});
        if let Some(body) = body {
            std::fs::write(root.join("original-body.json"), body).unwrap();
            value["body_file"] = serde_json::json!("original-body.json");
        }
        std::fs::write(&request, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
        std::fs::write(
            &headers,
            br#"[{"name":"Authorization","value":"Bearer protected-token"}]"#,
        )
        .unwrap();
        Self {
            root,
            project,
            request,
            headers,
            stored_output,
        }
    }

    fn command(&self, grant: bool) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command
            .args(["--diagnostics", "json", "run-rest-json", "--project"])
            .arg(&self.project)
            .arg("--request")
            .arg(&self.request)
            .arg("--header-values")
            .arg(&self.headers)
            .args([
                "--response-identity",
                "response.json",
                "--output-identity",
                "result.json",
            ]);
        if grant {
            command.args(["--allow-live-rest", "--allow-insecure-http"]);
        }
        command
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

struct Endpoint {
    url: String,
    thread: JoinHandle<Vec<Vec<u8>>>,
}

impl Endpoint {
    fn new(body: Vec<u8>, project_replacement: Option<(PathBuf, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!(
            "http://{}/request?token=private-query",
            listener.local_addr().unwrap()
        );
        let thread = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let mut requests = Vec::new();
            while Instant::now() < deadline {
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                        continue;
                    }
                    Err(error) => panic!("local test accept: {error}"),
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut raw = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    let n = stream.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&chunk[..n]);
                    if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                        let length = header
                            .lines()
                            .find_map(|line| {
                                line.strip_prefix("content-length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if raw.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                requests.push(raw);
                if let Some((path, replacement)) = &project_replacement {
                    std::fs::write(path, replacement).unwrap();
                }
                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(header.as_bytes());
                let _ = stream.write_all(&body);
            }
            requests
        });
        Self { url, thread }
    }

    fn finish(self) -> Vec<Vec<u8>> {
        self.thread.join().unwrap()
    }
}

fn capture(output: &Output, requests: &[Vec<u8>]) {
    eprintln!(
        "{}",
        serde_json::json!({"status":output.status.code(), "stdout":output.stdout,
        "stderr":output.stderr,"requests":requests})
    );
}

#[test]
fn cli_defaults_to_no_effect_no_stdout_and_json_diagnostics() {
    let endpoint = Endpoint::new(br#"{"Message":"ordinary"}"#.to_vec(), None);
    let files = Files::new(&endpoint.url, "POST", Some(br#"{"secret":"body-secret"}"#));
    let output = files.command(false).output().unwrap();
    let requests = endpoint.finish();
    capture(&output, &requests);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(requests.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr
            .lines()
            .all(|line| serde_json::from_str::<serde_json::Value>(line).is_ok())
    );
    for secret in ["private-query", "protected-token", "body-secret"] {
        assert!(!stderr.contains(secret));
    }
    assert_eq!(
        std::fs::read(&files.stored_output).unwrap(),
        b"original-output"
    );
}

#[test]
fn cli_post_returns_only_complete_json_and_leaves_configured_files_untouched() {
    let endpoint = Endpoint::new(br#"{"Message":"success <&>"}"#.to_vec(), None);
    let body = b" { \"n\" : 9007199254740993 } ";
    let files = Files::new(&endpoint.url, "POST", Some(body));
    let output = files.command(true).output().unwrap();
    let requests = endpoint.finish();
    capture(&output, &requests);
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert_eq!(requests.len(), 1);
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value, serde_json::json!({"Echo":"success <&>"}));
    let raw = &requests[0];
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    assert_eq!(&raw[end..], body);
    assert!(
        String::from_utf8(raw[..end].to_vec())
            .unwrap()
            .to_ascii_lowercase()
            .contains("authorization: bearer protected-token\r\n")
    );
    assert_eq!(
        std::fs::read(&files.stored_output).unwrap(),
        b"original-output"
    );
}

#[test]
fn malformed_response_and_mapping_failure_publish_no_partial_stdout() {
    for body in [
        br#"{"Message":"secret-response"} trailing"#.as_slice(),
        br#"{"Message":{"unusable":"data"}}"#.as_slice(),
    ] {
        let endpoint = Endpoint::new(body.to_vec(), None);
        let files = Files::new(&endpoint.url, "GET", None);
        let output = files.command(true).output().unwrap();
        let requests = endpoint.finish();
        capture(&output, &requests);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert_eq!(requests.len(), 1);
        assert_eq!(
            std::fs::read(&files.stored_output).unwrap(),
            b"original-output"
        );
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.contains("private-query"));
        assert!(!stderr.contains("protected-token"));
        // Mapping failures retain native causes; only transport/parser categories
        // promise no response-data text in their diagnostic surface.
        if body.ends_with(b"trailing") {
            assert!(!stderr.contains("secret-response"));
        }
    }
}

#[test]
fn request_description_failures_happen_before_connecting() {
    for description in [
        "method",
        "unknown",
        "timeout",
        "get-body",
        "header-injection",
    ] {
        let endpoint = Endpoint::new(br#"{"Message":"ordinary"}"#.to_vec(), None);
        let files = Files::new(&endpoint.url, "GET", None);
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&files.request).unwrap()).unwrap();
        match description {
            "method" => value["method"] = serde_json::json!("PUT"),
            "unknown" => value["credentials"] = serde_json::json!("protected-token"),
            "timeout" => value["timeout_seconds"] = serde_json::json!(301),
            "get-body" => {
                value["body_file"] = serde_json::json!("body.json");
                std::fs::write(files.root.join("body.json"), b"{}").unwrap();
            }
            "header-injection" => {
                std::fs::write(
                    &files.headers,
                    br#"[{"name":"Authorization","value":"protected-token\r\nInjected: yes"}]"#,
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        std::fs::write(&files.request, serde_json::to_vec(&value).unwrap()).unwrap();
        let output = files.command(true).output().unwrap();
        let requests = endpoint.finish();
        capture(&output, &requests);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(requests.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(!stderr.contains("private-query"));
        assert!(!stderr.contains("protected-token"));
    }
}

#[test]
fn loaded_project_is_not_reopened_after_remote_effect() {
    // Build the path before starting the endpoint that will replace it.
    let files = Files::new("http://127.0.0.1:9/replaced", "GET", None);
    let endpoint = Endpoint::new(
        br#"{"Message":"original mapping"}"#.to_vec(),
        Some((
            files.project.clone(),
            b"invalid replacement project".to_vec(),
        )),
    );
    std::fs::write(
        &files.request,
        serde_json::to_vec(&serde_json::json!({"url":endpoint.url,"method":"GET"})).unwrap(),
    )
    .unwrap();
    let output = files.command(true).output().unwrap();
    let requests = endpoint.finish();
    capture(&output, &requests);
    assert!(output.status.success());
    assert_eq!(requests.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({"Echo":"original mapping"})
    );
    assert_eq!(
        std::fs::read(&files.project).unwrap(),
        b"invalid replacement project"
    );
    assert_eq!(
        std::fs::read(&files.stored_output).unwrap(),
        b"original-output"
    );
}

#[test]
fn child_environment_proxy_values_do_not_change_the_selected_endpoint() {
    let endpoint = Endpoint::new(br#"{"Message":"direct"}"#.to_vec(), None);
    let proxy = Endpoint::new(br#"{"Message":"proxy"}"#.to_vec(), None);
    let files = Files::new(&endpoint.url, "GET", None);
    let proxy_origin = proxy.url.split("/request").next().unwrap();
    let output = files
        .command(true)
        .env("HTTP_PROXY", proxy_origin)
        .env("http_proxy", proxy_origin)
        .env("HTTPS_PROXY", proxy_origin)
        .env("https_proxy", proxy_origin)
        .env("ALL_PROXY", proxy_origin)
        .env("all_proxy", proxy_origin)
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .output()
        .unwrap();
    let direct_requests = endpoint.finish();
    let proxy_requests = proxy.finish();
    capture(&output, &direct_requests);
    eprintln!("{}", serde_json::json!({"proxy_requests":proxy_requests}));
    assert!(output.status.success());
    assert_eq!(direct_requests.len(), 1);
    assert!(proxy_requests.is_empty());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap(),
        serde_json::json!({"Echo":"direct"})
    );
}
