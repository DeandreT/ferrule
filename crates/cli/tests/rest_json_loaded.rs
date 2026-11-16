use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use test_support::TempDir as Files;

fn project(value: &str) -> mapping::Project {
    serde_json::from_value(serde_json::json!({
        "source":{"name":"Input","kind":{"kind":"group","children":[
            {"name":"Value","kind":{"kind":"scalar","ty":"string"}}
        ]}},
        "target":{"name":"Result","kind":{"kind":"group","children":[
            {"name":"Value","kind":{"kind":"scalar","ty":"string"}}
        ]}},
        "graph":{"nodes":{"0":{"kind":"const","value":value}}},
        "root":{"bindings":[{"target_field":"Value","node":0}]},
        "target_path":"must-not-write.json"
    }))
    .unwrap()
}

fn endpoint(project_path: PathBuf) -> (String, thread::JoinHandle<Vec<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!(
        "http://{}/loaded?opaque=fixture-query",
        listener.local_addr().unwrap()
    );
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut originals = Vec::new();
        while Instant::now() < deadline {
            let mut stream = match listener.accept() {
                Ok((stream, _)) => stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(_) => break,
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut raw = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&chunk[..n]);
                if let Some(end) = raw.windows(4).position(|b| b == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .and_then(|n| n.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if raw.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            originals.push(raw);
            let _ = std::fs::write(&project_path, b"not a project after the request");
            let body = br#"{"Value":"remote"}"#;
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body);
        }
        originals
    });
    (url, worker)
}

#[test]
fn loaded_snapshot_and_single_host_file_parser_preserve_get_post_and_denial() {
    for method in ["GET", "POST"] {
        let files = Files::new("rest-loaded").unwrap();
        let project_path = files.join("mapping.json");
        std::fs::write(
            &project_path,
            mapping::project_file::encode_pretty(&project("saved")).unwrap(),
        )
        .unwrap();
        std::fs::write(files.join("must-not-write.json"), b"untouched").unwrap();
        let snapshot = project("unsaved snapshot");
        let (url, server) = endpoint(project_path.clone());
        let body = br#"{"exact":9007199254740993}"#;
        std::fs::write(files.join("body.json"), body).unwrap();
        let description = if method == "POST" {
            serde_json::json!({"method":method,"url":url,"body_file":"body.json"})
        } else {
            serde_json::json!({"method":method,"url":url})
        };
        std::fs::write(
            files.join("request.json"),
            serde_json::to_vec(&description).unwrap(),
        )
        .unwrap();
        std::fs::write(
            files.join("headers.json"),
            br#"[{"name":"Authorization","value":"fixture-credential"}]"#,
        )
        .unwrap();
        let options =
            cli::RestJsonMappingOptions::new(Path::new("response.json"), Path::new("result.json"));
        let denied = cli::run_project_value_rest_json_request_file_payloads(
            &snapshot,
            &project_path,
            &files.join("missing-request.json"),
            None,
            cli::RestExecutionPolicy::Deny,
            &options,
        );
        let result = cli::run_project_value_rest_json_request_file_payloads(
            &snapshot,
            &project_path,
            &files.join("request.json"),
            Some(&files.join("headers.json")),
            cli::RestExecutionPolicy::AllowSingleRequest {
                allow_insecure_http: true,
            },
            &options,
        );
        let raw = server.join().unwrap();
        let disk_after = std::fs::read(&project_path).unwrap();
        let target_after = std::fs::read(files.join("must-not-write.json")).unwrap();
        eprintln!(
            "{}",
            serde_json::json!({
                "method":method,"original_requests":raw,"original_request_description":description,
                "denied_error":denied.as_ref().err().map(|e|format!("{e:?}")),
                "returned_error":result.as_ref().err().map(|e|format!("{e:?}")),
                "returned_original_artifacts":result.as_ref().ok().map(|o|o.artifacts.iter().map(|a|&a.bytes).collect::<Vec<_>>()),
                "physical_project_after":disk_after,"physical_target_after":target_after,
            })
        );
        assert!(matches!(denied, Err(cli::RestJsonError::Disabled)));
        let outcome = result.unwrap();
        assert_eq!(raw.len(), 1);
        let head = raw[0].windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
        assert!(raw[0].starts_with(format!("{method} /loaded?opaque=fixture-query ").as_bytes()));
        assert!(String::from_utf8_lossy(&raw[0][..head]).contains("fixture-credential"));
        let expected: &[u8] = if method == "POST" { &body[..] } else { &[] };
        assert_eq!(&raw[0][head..], expected);
        assert_eq!(outcome.artifacts.len(), 1);
        let mapped: serde_json::Value =
            serde_json::from_slice(&outcome.artifacts[0].bytes).unwrap();
        assert_eq!(mapped["Value"], "unsaved snapshot");
        assert_eq!(disk_after, b"not a project after the request");
        assert_eq!(target_after, b"untouched");
    }
}
