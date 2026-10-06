use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::*;

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("ferrule-gui-rest-{}-{id}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project(value: &str) -> Project {
    serde_json::from_value(serde_json::json!({
        "source":{"name":"Input","kind":{"kind":"group","children":[
            {"name":"Value","kind":{"kind":"scalar","ty":"string"}}
        ]}},
        "target":{"name":"Result","kind":{"kind":"group","children":[
            {"name":"Value","kind":{"kind":"scalar","ty":"string"}}
        ]}},
        "graph":{"nodes":{"0":{"kind":"const","value":value}}},
        "root":{"bindings":[{"target_field":"Value","node":0}]},
        "target_path":"output.json"
    }))
    .unwrap()
}

fn app(files: &Files) -> FerruleApp {
    let project_path = files.path("mapping.json");
    let mut app = FerruleApp {
        project: project("saved"),
        ..Default::default()
    };
    app.main_canvas.snarl = build_snarl(&app.project);
    app.document = DocumentLocation::saved(project_path.clone());
    app.save_document_to(&project_path).unwrap();
    app.project.graph.nodes.insert(
        0,
        mapping::Node::Const {
            value: ir::Value::String("unsaved snapshot".into()),
        },
    );
    std::fs::write(files.path("output.json"), b"untouched").unwrap();
    app
}

struct Endpoint {
    url: String,
    accepted: Receiver<()>,
    worker: JoinHandle<Vec<Vec<u8>>>,
}
impl Endpoint {
    fn new(body: &'static [u8], delay: Duration) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!(
            "http://{}/data?opaque=gui-query-sentinel",
            listener.local_addr().unwrap()
        );
        let (signal, accepted) = mpsc::channel();
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
                let _ = signal.send(());
                thread::sleep(delay);
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
            originals
        });
        Self {
            url,
            accepted,
            worker,
        }
    }
    fn finish(self) -> Vec<Vec<u8>> {
        self.worker.join().unwrap()
    }
}

fn select_files(app: &mut FerruleApp, files: &Files, endpoint: &Endpoint, method: &str) {
    std::fs::write(files.path("body.json"), br#"{"original":9007199254740993}"#).unwrap();
    let description = if method == "POST" {
        serde_json::json!({"url":endpoint.url,"method":method,"body_file":"body.json"})
    } else {
        serde_json::json!({"url":endpoint.url,"method":method})
    };
    std::fs::write(
        files.path("request-path-sentinel.json"),
        serde_json::to_vec(&description).unwrap(),
    )
    .unwrap();
    std::fs::write(
        files.path("header-path-sentinel.json"),
        br#"[{"name":"Authorization","value":"gui-credential-sentinel"}]"#,
    )
    .unwrap();
    app.begin_rest_run();
    // The existing asynchronous file-dialog completion is the only seam;
    // production picker polling and grant reset are exercised unchanged.
    for (kind, path) in [
        (
            RestPicker::Request,
            files.path("request-path-sentinel.json"),
        ),
        (RestPicker::Headers, files.path("header-path-sentinel.json")),
    ] {
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Some(path.to_string_lossy().into_owned()))
            .unwrap();
        app.rest_run_draft.as_mut().unwrap().picker = Some((kind, receiver));
        app.rest_run_draft.as_mut().unwrap().poll_picker();
    }
}

fn context() -> egui::Context {
    let context = egui::Context::default();
    crate::icons::install(&context);
    context
}

fn form_frame(
    context: &egui::Context,
    draft: &mut RestRunDraft,
    events: Vec<egui::Event>,
    step: &mut usize,
) -> (egui::Rect, egui::Rect, egui::Rect, bool) {
    let mut observed = (
        egui::Rect::NOTHING,
        egui::Rect::NOTHING,
        egui::Rect::NOTHING,
        false,
    );
    let _ = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 650.0),
            )),
            time: Some(*step as f64 * 0.1),
            events,
            ..Default::default()
        },
        |ui| {
            let controls = rest_controls(ui, draft);
            observed = (
                controls.grant.rect,
                controls.http.rect,
                controls.run.rect,
                controls.run.clicked(),
            );
        },
    );
    *step += 1;
    observed
}

fn click(
    context: &egui::Context,
    draft: &mut RestRunDraft,
    point: egui::Pos2,
    step: &mut usize,
) -> bool {
    let mut clicked = false;
    for pressed in [true, false] {
        clicked |= form_frame(
            context,
            draft,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            step,
        )
        .3;
    }
    clicked
}

fn grant_and_start(app: &mut FerruleApp, context: &egui::Context) -> [bool; 6] {
    let draft = app.rest_run_draft.as_mut().unwrap();
    let mut step = 0;
    let (grant, http, run, _) = form_frame(context, draft, Vec::new(), &mut step);
    let blocked_click = click(context, draft, run.center(), &mut step);
    eprintln!(
        "{}",
        serde_json::json!({"default_grant":draft.grant,"default_http":draft.insecure_http,"default_run_clicked":blocked_click})
    );
    let initial = [blocked_click, draft.grant, draft.insecure_http];
    click(context, draft, grant.center(), &mut step);
    click(context, draft, http.center(), &mut step);
    let (_, _, run, _) = form_frame(context, draft, Vec::new(), &mut step);
    let launch = click(context, draft, run.center(), &mut step);
    eprintln!(
        "{}",
        serde_json::json!({"explicit_grant":draft.grant,"explicit_http":draft.insecure_http,"approved_run_clicked":launch})
    );
    let observed = [
        initial[0],
        initial[1],
        initial[2],
        launch,
        draft.grant,
        draft.insecure_http,
    ];
    if launch {
        app.start_rest_run();
    }
    observed
}

fn finish(app: &mut FerruleApp, context: &egui::Context) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.pending_rest_run.is_some() && Instant::now() < deadline {
        app.poll_rest_run(context);
        thread::sleep(Duration::from_millis(5));
    }
}

fn result_window_text(app: &mut FerruleApp, context: &egui::Context) -> String {
    fn collect(shape: &egui::epaint::Shape, text: &mut String) {
        match shape {
            egui::epaint::Shape::Text(value) => {
                text.push_str(value.galley.text());
                text.push('\n');
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, text);
                }
            }
            _ => {}
        }
    }
    let mut text = String::new();
    // Two real window frames allow its first layout pass to settle.
    for _ in 0..2 {
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 650.0),
                )),
                ..Default::default()
            },
            |ui| app.show_rest_run(ui.ctx()),
        );
        for shape in output.shapes {
            collect(&shape.shape, &mut text);
        }
    }
    text
}

fn observe(app: &FerruleApp, raw: &[Vec<u8>]) {
    eprintln!(
        "{}",
        serde_json::json!({
            "original_requests":raw,"status":app.status,
            "diagnostics":app.diagnostics.items().iter().map(|d| &d.message).collect::<Vec<_>>(),
            "pending_worker":app.pending_rest_run.is_some(),"pending_draft":app.rest_run_draft.is_some(),
            "original_result_buffers":app.rest_run_result.as_ref().map(|r|r.outcome.artifacts.iter().map(|a|&a.bytes).collect::<Vec<_>>()),
        })
    );
}

#[derive(Default)]
struct Storage(std::collections::BTreeMap<String, String>);
impl eframe::Storage for Storage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.0.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.0.insert(key.into(), value);
    }
    fn remove_string(&mut self, key: &str) {
        self.0.remove(key);
    }
    fn flush(&mut self) {}
}

#[test]
fn transient_file_grant_maps_unsaved_snapshot_once_without_persisting_credentials() {
    for method in ["GET", "POST"] {
        let files = Files::new();
        let mut app = app(&files);
        let context = context();
        let endpoint = Endpoint::new(br#"{"Value":"remote"}"#, Duration::ZERO);
        select_files(&mut app, &files, &endpoint, method);
        let project_before = mapping::project_file::encode_pretty(&app.project).unwrap();
        let snapshot_before =
            editor_snapshot(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
                .state
                .serialized_project;
        app.start_rest_run(); // Unchecked grant cannot create a worker or effect.
        let default_pending = app.pending_rest_run.is_some();
        let approval = grant_and_start(&mut app, &context);
        finish(&mut app, &context);
        let raw = endpoint.finish();
        observe(&app, &raw);
        let painted_result = result_window_text(&mut app, &context);
        eprintln!(
            "{}",
            serde_json::json!({"original_result_window_text":painted_result})
        );
        let project_after = mapping::project_file::encode_pretty(&app.project).unwrap();
        let snapshot_after =
            editor_snapshot(&app.project, &app.main_canvas.snarl, &app.mapping_workspace)
                .state
                .serialized_project;
        let physical_target = std::fs::read(files.path("output.json")).unwrap();
        app.save_document_to(&files.path("mapping.json")).unwrap();
        let saved = std::fs::read_to_string(files.path("mapping.json")).unwrap();
        let layout = std::fs::read_to_string(crate::layout_store::layout_path(
            &files.path("mapping.json"),
        ))
        .unwrap();
        let mut storage = Storage::default();
        eframe::App::save(&mut app, &mut storage);
        let persisted = format!(
            "{saved}{layout}{}",
            storage.0.values().cloned().collect::<String>()
        );
        eprintln!(
            "{}",
            serde_json::json!({"default_pending":default_pending,"project_before":project_before,"project_after":project_after,"snapshot_before":snapshot_before,"snapshot_after":snapshot_after,"persisted_originals":persisted,"physical_target":physical_target})
        );
        assert_eq!(approval, [false, false, false, true, true, true]);
        assert!(!default_pending);
        assert!(app.pending_rest_run.is_none());
        assert_eq!(raw.len(), 1);
        assert!(painted_result.contains("Live request result (in memory)"));
        assert!(!painted_result.contains("Preview results"));
        assert!(
            raw[0].starts_with(format!("{method} /data?opaque=gui-query-sentinel ").as_bytes())
        );
        assert!(String::from_utf8_lossy(&raw[0]).contains("gui-credential-sentinel"));
        let end = raw[0].windows(4).position(|b| b == b"\r\n\r\n").unwrap() + 4;
        assert_eq!(
            &raw[0][end..],
            if method == "POST" {
                &br#"{"original":9007199254740993}"#[..]
            } else {
                &[]
            }
        );
        let result = app.rest_run_result.as_ref().unwrap();
        assert_eq!(result.outcome.artifacts.len(), 1);
        let mapped: serde_json::Value =
            serde_json::from_slice(&result.outcome.artifacts[0].bytes).unwrap();
        assert_eq!(mapped["Value"], "unsaved snapshot");
        assert_eq!(physical_target, b"untouched");
        assert_eq!(project_before, project_after);
        assert_eq!(snapshot_before, snapshot_after);
        for secret in [
            "gui-credential-sentinel",
            "gui-query-sentinel",
            "request-path-sentinel",
            "header-path-sentinel",
        ] {
            assert!(!persisted.contains(secret));
        }
        assert!(app.rest_run_draft.is_none());
        app.begin_rest_run();
        assert!(!app.rest_run_draft.as_ref().unwrap().grant);
        app.cancel_rest_run();
        assert!(app.rest_run_draft.is_none());
    }
}

#[test]
fn pending_request_locks_ordinary_actions_and_close_waits_for_owned_worker() {
    let files = Files::new();
    let mut app = app(&files);
    let context = context();
    let endpoint = Endpoint::new(br#"{"Value":"remote"}"#, Duration::from_millis(300));
    select_files(&mut app, &files, &endpoint, "GET");
    let approval = grant_and_start(&mut app, &context);
    let accepted = endpoint
        .accepted
        .recv_timeout(Duration::from_secs(2))
        .is_ok();
    if !accepted || app.pending_rest_run.is_none() {
        finish(&mut app, &context);
        let raw = endpoint.finish();
        observe(&app, &raw);
        eprintln!(
            "{}",
            serde_json::json!({"approval":approval,"accepted":accepted})
        );
        panic!("the explicit request must own its worker after the server observes its effect");
    }
    let project_before = mapping::project_file::encode_pretty(&app.project).unwrap();
    let [undo, redo, _] = history_shortcuts();
    let editing = app.project_editing_enabled();
    let _ = context.run_ui(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::R,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            }],
            ..Default::default()
        },
        |ui| app.show_command_bar(ui, editing, LayoutClass::Wide, &undo, &redo),
    );
    app.run(&context);
    app.debug_run(&context);
    app.run_saved(false);
    app.begin_preview();
    app.begin_library_generation();
    app.start_pipeline_run();
    let destructive = app.request_destructive_action(DestructiveAction::NewProject);
    app.perform_destructive_action(DestructiveAction::NewProject, &context);
    app.save_with_continuation(None, &context);
    app.guard_app_close_requested(&context, true);
    let waiting_for_close = app.close_after_rest_run && app.pending_rest_run.is_some();
    finish(&mut app, &context);
    let raw = endpoint.finish();
    observe(&app, &raw);
    let project_after = mapping::project_file::encode_pretty(&app.project).unwrap();
    app.guard_app_close_requested(&context, true);
    eprintln!(
        "{}",
        serde_json::json!({"editing_enabled":editing,"waiting_for_close":waiting_for_close,"project_before":project_before,"project_after":project_after,"dirty_close_action":format!("{:?}",app.pending_destructive_action)})
    );
    assert_eq!(approval, [false, false, false, true, true, true]);
    assert!(!editing);
    assert!(waiting_for_close);
    assert!(destructive.is_none());
    assert!(app.pending_rest_run.is_none());
    assert!(!app.close_after_rest_run);
    assert!(app.rest_run_result.is_none());
    assert_eq!(app.status, "live request cancelled");
    assert_eq!(raw.len(), 1);
    assert!(app.pending_file_run.is_none());
    assert!(app.pending_preview.is_none());
    assert!(app.pending_pipeline_run.is_none());
    assert!(app.library_generation_draft.is_none());
    assert_eq!(project_before, project_after);
    assert_eq!(
        app.pending_destructive_action,
        Some(DestructiveAction::Close)
    );
    assert_eq!(
        std::fs::read(files.path("output.json")).unwrap(),
        b"untouched"
    );
}

#[test]
fn malformed_response_has_redacted_gui_category_and_no_partial_result() {
    let files = Files::new();
    let mut app = app(&files);
    let context = context();
    let endpoint = Endpoint::new(b"not JSON", Duration::ZERO);
    select_files(&mut app, &files, &endpoint, "POST");
    let approval = grant_and_start(&mut app, &context);
    finish(&mut app, &context);
    let raw = endpoint.finish();
    observe(&app, &raw);
    let messages = app
        .diagnostics
        .items()
        .iter()
        .map(|d| d.message.as_str())
        .collect::<String>();
    assert_eq!(approval, [false, false, false, true, true, true]);
    assert!(app.pending_rest_run.is_none());
    assert!(app.rest_run_result.is_none());
    assert_eq!(raw.len(), 1);
    assert_eq!(app.status, "live request failed");
    assert!(messages.contains("response-json"));
    for secret in [
        "gui-credential-sentinel",
        "gui-query-sentinel",
        "request-path-sentinel",
        "header-path-sentinel",
    ] {
        assert!(!messages.contains(secret));
        assert!(!app.status.contains(secret));
    }
    assert_eq!(
        std::fs::read(files.path("output.json")).unwrap(),
        b"untouched"
    );
    app.begin_rest_run();
    assert!(!app.rest_run_draft.as_ref().unwrap().grant);
}

#[test]
fn result_preview_preserves_utf8_at_64k_boundary_and_complete_output() {
    // These byte positions come from the public 64 KiB display contract,
    // rather than deriving expected output from the preview implementation.
    const DISPLAY_LIMIT: usize = 65_536;
    let json_prefix = |bytes: usize| format!("\"{}", "a".repeat(bytes - 1));
    let short = "\"é🙂\"".to_owned();
    let exact_limit = format!("\"{}🙂\"", "a".repeat(65_530));
    let scalar_ends_at_limit = format!("{}🙂", json_prefix(65_532));
    let scalar_starts_at_limit = json_prefix(65_536);
    let mut cases = vec![
        ("short multibyte output", short.clone(), short, false),
        (
            "complete output exactly at limit",
            exact_limit.clone(),
            exact_limit,
            false,
        ),
        (
            "scalar ends at limit before remaining output",
            format!("{scalar_ends_at_limit}tail\""),
            scalar_ends_at_limit,
            true,
        ),
        (
            "scalar starts at limit",
            format!("{scalar_starts_at_limit}🙂tail\""),
            scalar_starts_at_limit,
            true,
        ),
    ];
    for (name, prefix_bytes) in [
        ("cutoff after one scalar byte", 65_535),
        ("cutoff after two scalar bytes", 65_534),
        ("cutoff after three scalar bytes", 65_533),
    ] {
        let prefix = json_prefix(prefix_bytes);
        cases.push((name, format!("{prefix}🙂tail\""), prefix, true));
    }

    for (name, complete, expected_preview, expected_truncated) in cases {
        let original_bytes = complete.as_bytes().to_vec();
        let original_allocation = original_bytes.as_ptr();
        let result = RestRunResult::new(cli::PayloadRunOutcome {
            records_written: 1,
            artifacts: vec![cli::PayloadArtifact {
                target: "Primary".into(),
                records_written: 1,
                path: PathBuf::from("result.json"),
                bytes: original_bytes,
            }],
        });
        let artifact = &result.outcome.artifacts[0];
        let original_allocation_retained = artifact.bytes.as_ptr() == original_allocation;
        eprintln!(
            "{}",
            serde_json::json!({
                "case": name,
                "display_limit_bytes": DISPLAY_LIMIT,
                "original_artifact_bytes": artifact.bytes.as_slice(),
                "original_artifact_target": artifact.target.as_str(),
                "original_artifact_path": artifact.path,
                "original_artifact_records": artifact.records_written,
                "returned_outcome_records": result.outcome.records_written,
                "original_allocation_retained": original_allocation_retained,
                "expected_preview": expected_preview.as_str(),
                "returned_preview": result.preview.as_str(),
                "expected_truncated": expected_truncated,
                "returned_truncated": result.truncated,
            })
        );
        assert_eq!(result.preview, expected_preview, "{name}");
        assert_eq!(result.truncated, expected_truncated, "{name}");
        assert!(result.preview.len() <= DISPLAY_LIMIT, "{name}");
        assert!(!result.preview.contains('\u{fffd}'), "{name}");
        assert_eq!(artifact.bytes.as_slice(), complete.as_bytes(), "{name}");
        assert!(original_allocation_retained, "{name}");
        assert_eq!(artifact.target, "Primary", "{name}");
        assert_eq!(artifact.path, PathBuf::from("result.json"), "{name}");
        assert_eq!(artifact.records_written, 1, "{name}");
        assert_eq!(result.outcome.records_written, 1, "{name}");
        assert_eq!(result.outcome.artifacts.len(), 1, "{name}");
        if name == "complete output exactly at limit" {
            assert_eq!(complete.len(), DISPLAY_LIMIT);
        }
    }
}
