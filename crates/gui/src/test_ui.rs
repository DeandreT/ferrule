//! Frame-only tests inspect shapes and interaction state without a renderer.
//! Their texture uploads must be explicitly discarded under egui 0.37.
//! Rendering tests use egui_kittest, which applies the uploads instead.

pub(crate) trait ContextExt {
    fn run_test_ui(&self, input: egui::RawInput, ui: impl FnMut(&mut egui::Ui))
    -> egui::FullOutput;
}

impl ContextExt for egui::Context {
    fn run_test_ui(
        &self,
        input: egui::RawInput,
        ui: impl FnMut(&mut egui::Ui),
    ) -> egui::FullOutput {
        let mut output = self.run_ui(input, ui);
        output.textures_delta.clear();
        output
    }
}

/// Existing mapping-authoring tests address expressions by their stable node
/// identity. Keep those interaction fixtures independent of human-facing titles;
/// presentation tests assert the actual wording separately.
pub(crate) fn inspector_label_matches(actual: &str, wanted: &str) -> bool {
    if wanted.starts_with("Remove property ") {
        return actual == char::from(lucide_icons::Icon::Trash2).to_string();
    }
    if let Some((id, _)) = wanted.split_once(": ")
        && id.parse::<mapping::NodeId>().is_ok()
    {
        return actual.ends_with(&format!(" · #{id}"));
    }
    let icon = match wanted {
        "Move up" | "Up" => Some(lucide_icons::Icon::ArrowUp),
        "Move down" | "Down" => Some(lucide_icons::Icon::ArrowDown),
        "Remove rule" | "Remove key" | "x" => Some(lucide_icons::Icon::Trash2),
        _ => None,
    };
    if let Some(icon) = icon {
        return actual == char::from(icon).to_string();
    }
    let replacement = match wanted {
        "<current rows>" => "Current rows",
        "<item value>" => "Current item value",
        "+ binding" => "Add binding",
        "+ property" => "Add property",
        "sorted" => "Sort items",
        "descending" => "Descending",
        "iterates" => "Iterate source collection",
        "filtered" => "Filter items",
        "none" => "No grouping",
        "key" => "By key",
        "adjacent key" => "Adjacent keys",
        "starting predicate" => "Starting predicate",
        "ending predicate" => "Ending predicate",
        "fixed blocks" => "Fixed-size blocks",
        "skip first" => "Skip first items",
        "first" => "First items",
        "from" => "From position",
        "from to" => "Position range",
        "last" => "Last items",
        "+" => "Add window",
        _ => wanted,
    };
    actual == replacement
        || (wanted.starts_with("Rule ") && actual == wanted.replacen(": ", " · ", 1))
}
