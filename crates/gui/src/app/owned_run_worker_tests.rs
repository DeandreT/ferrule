//! Real preview and pipeline workers are owned by their exact thread handles:
//! dropping a paused owner cancels it and returns only after that thread has
//! exited and released everything it shared with the UI thread.

use super::*;

#[test]
fn dropping_a_paused_debug_preview_tears_down_its_worker_before_returning() {
    let mut app = two_field_debug_preview_app();
    app.execute_debug_preview();
    let _ = wait_for_debug_pause(&mut app);
    let pending = app.pending_preview.take().expect("debug preview is paused");
    let shared = pending.cancellation_for_test();
    assert!(
        shared.strong_count() >= 2,
        "the paused worker shares the flag"
    );
    drop(pending);
    assert_eq!(
        shared.strong_count(),
        0,
        "the worker thread exited and released its state before drop returned"
    );
}

#[test]
fn dropping_a_paused_debug_pipeline_tears_down_its_worker_before_returning() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-owned-teardown")?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    app.start_pipeline_debug_run();
    let _ = wait_for_pipeline_pause(&mut app);
    let pending = app
        .pending_pipeline_run
        .take()
        .expect("debug pipeline is paused");
    let shared = pending.cancellation_for_test();
    assert!(
        shared.strong_count() >= 2,
        "the paused worker shares the flag"
    );
    drop(pending);
    assert_eq!(
        shared.strong_count(),
        0,
        "the worker thread exited and released its state before drop returned"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn plain_pipeline_run_cancels_without_a_stage_debug_hook() -> anyhow::Result<()> {
    let (mut app, pipeline_path) = two_stage_pipeline_app("pipeline-plain-cancel")?;
    let directory = pipeline_path.parent().expect("pipeline has directory");
    app.start_pipeline_run();
    app.pipeline_run_command(pipeline_ui::PipelineRunCommand::Cancel);
    wait_for_pipeline_completion(&mut app);
    assert_eq!(app.status, "pipeline cancelled");
    assert!(app.run_report.is_none());
    assert_eq!(
        std::fs::read_to_string(directory.join("prepare.json"))?,
        "old prepare"
    );
    assert_eq!(
        std::fs::read_to_string(directory.join("finish.json"))?,
        "old finish"
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
