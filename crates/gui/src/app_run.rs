use super::*;

impl FerruleApp {
    pub(super) fn clear_run_report(&mut self) {
        self.run_report = None;
        self.show_run_report = false;
    }

    pub(super) fn run(&mut self, ctx: &egui::Context) {
        let issues = cli::validate(&self.project);
        if !issues.is_empty() {
            self.status = format!("run blocked by {} validation issue(s)", issues.len());
            self.diagnostics.validation(issues);
            return;
        }
        self.show_run_report = false;
        self.diagnostics.clear();
        self.save_with_continuation(Some(SaveContinuation::Run), ctx);
    }

    pub(super) fn run_saved(&mut self) {
        let Some(project_path) = self.document.saved_path() else {
            self.status = "run failed".to_string();
            self.diagnostics
                .error("Run failed", "project has no saved file");
            return;
        };
        let input_path = nonempty_path(&self.input_path);
        let output_path = nonempty_path(&self.output_path);
        let started = std::time::Instant::now();
        match cli::run_project_with_paths(
            project_path,
            input_path.as_deref(),
            output_path.as_deref(),
        ) {
            Ok(outcome) => {
                self.status = format!(
                    "wrote {} record(s) to {}",
                    outcome.records_written,
                    outcome.output_path.display()
                );
                let report = crate::run_report::RunReport::from_outcome(outcome, started.elapsed());
                self.run_report = Some(crate::run_report::RunReportView::new(report));
                self.show_run_report = true;
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "run failed".to_string();
                self.diagnostics.error("Run failed", error.to_string());
            }
        }
    }
}

fn nonempty_path(value: &str) -> Option<PathBuf> {
    (!value.trim().is_empty()).then(|| PathBuf::from(value.trim()))
}
