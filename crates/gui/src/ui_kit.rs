//! Shared building blocks for panels, forms and dialogs.
//!
//! Views use these helpers instead of hand-placing spacing, field widths,
//! list heights and window chrome, so every surface follows the tokens in
//! [`crate::theme`]:
//!
//! - [`section`] / [`collapsible_section`]: a titled group of controls.
//! - [`form_grid`]: label/field rows; [`table_grid`] for multi-column rows.
//!   Put [`text_field`] inside either so fields keep their requested width.
//! - [`help`] for a wrapped explanation, [`help_icon`] for one behind an (i).
//! - [`icon_button`]: an icon with a tooltip that also explains why it is
//!   disabled.
//! - [`dialog`] + [`dialog_buttons`]: window chrome with a fixed placement
//!   policy, a scrolling body, a pinned footer and consistent button order.
//! - [`list_area`]: a vertical scroll area with a token height.
//! - [`tab`]: a document tab with an inline close button.
//! - [`plural`] and [`sentence`] for user-facing counts and messages.

use std::borrow::Cow;

use egui::{Align, Align2, Layout, Pos2, Rect, Response, RichText, Ui, Vec2, WidgetText, vec2};
use lucide_icons::Icon;

pub use crate::theme::{DialogWidth, FieldWidth, ListHeight, METRICS, TextRole, space};

/// Formats a count with its noun: `plural(1, "record")` is "1 record",
/// `plural(3, "match")` is "3 matches".
pub fn plural(count: usize, singular: &str) -> String {
    if count == 1 {
        return format!("1 {singular}");
    }
    let lower = singular.to_ascii_lowercase();
    let noun = if ["s", "x", "z", "ch", "sh"]
        .iter()
        .any(|suffix| lower.ends_with(suffix))
    {
        format!("{singular}es")
    } else if lower.ends_with('y')
        && !lower
            .chars()
            .rev()
            .nth(1)
            .is_some_and(|before| "aeiou".contains(before))
    {
        format!("{}ies", &singular[..singular.len() - 1])
    } else {
        format!("{singular}s")
    };
    format!("{count} {noun}")
}

/// The file name of `path` for messages; the full path belongs in a
/// tooltip.
pub fn file_label(path: impl AsRef<std::path::Path>) -> String {
    let path = path.as_ref();
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Capitalizes the first character so lowercase status fragments read as
/// sentences.
pub fn sentence(text: &str) -> Cow<'_, str> {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => {
            Cow::Owned(first.to_uppercase().chain(chars).collect())
        }
        _ => Cow::Borrowed(text),
    }
}

/// A titled group of controls, separated from the previous group by the
/// section gap.
pub fn section<R>(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    section_gap(ui);
    ui.label(TextRole::Section.text(title));
    add(ui)
}

/// A titled group that the user can collapse; returns `None` while closed.
pub fn collapsible_section<R>(
    ui: &mut Ui,
    id_salt: impl egui::AsIdSalt,
    title: &str,
    default_open: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    section_gap(ui);
    egui::CollapsingHeader::new(TextRole::Section.text(title))
        .id_salt(id_salt)
        .default_open(default_open)
        .show(ui, add)
        .body_returned
}

fn section_gap(ui: &mut Ui) {
    if ui.min_rect().height() > 0.0 {
        ui.add_space((METRICS.section_gap - ui.spacing().item_spacing.y).max(0.0));
    }
}

/// Wrapped secondary text that never widens its container.
pub fn help(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add(egui::Label::new(TextRole::Help.text(text)).wrap())
}

/// A small (i) marker that shows `text` on hover; use it instead of a long
/// help paragraph next to compact controls.
pub fn help_icon(ui: &mut Ui, text: impl Into<WidgetText>) -> Response {
    ui.add(
        egui::Label::new(
            crate::icons::text(Icon::Info, METRICS.icon_small)
                .color(ui.visuals().weak_text_color()),
        )
        .sense(egui::Sense::hover()),
    )
    .on_hover_text(text)
}

/// A single-line text field that keeps `width` even inside a grid column.
///
/// egui offers a non-last grid column only its previous width and clamps a
/// field's desired width to it, so a field needs a minimum size to grow.
pub fn text_field(text: &mut String, width: FieldWidth) -> egui::TextEdit<'_> {
    let field = egui::TextEdit::singleline(text).min_size(vec2(width.min(), 0.0));
    match width {
        FieldWidth::Fill => field.desired_width(f32::INFINITY),
        _ => field.desired_width(width.min()),
    }
}

/// Label/field rows of a form.
pub struct Form<'u> {
    ui: &'u mut Ui,
}

impl Form<'_> {
    /// Adds one row: `label` in the label column and `field` filling the
    /// field column.
    pub fn row<R>(&mut self, label: impl Into<WidgetText>, field: impl FnOnce(&mut Ui) -> R) -> R {
        self.ui.label(label);
        let inner = self.ui.horizontal(field).inner;
        self.ui.end_row();
        inner
    }
}

/// Lays out label/field rows with aligned labels.
pub fn form_grid<R>(
    ui: &mut Ui,
    id_salt: impl egui::AsIdSalt,
    add: impl FnOnce(&mut Form<'_>) -> R,
) -> R {
    grid(id_salt, 2).show(ui, |ui| add(&mut Form { ui })).inner
}

/// Lays out rows of `columns` cells, for example a name/value table. Use
/// [`text_field`] for fields so the columns size to them.
pub fn table_grid<R>(
    ui: &mut Ui,
    id_salt: impl egui::AsIdSalt,
    columns: usize,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    grid(id_salt, columns).show(ui, add).inner
}

fn grid(id_salt: impl egui::AsIdSalt, columns: usize) -> egui::Grid {
    egui::Grid::new(id_salt)
        .num_columns(columns)
        .min_col_width(METRICS.min_grid_col)
        .spacing(vec2(METRICS.inline_gap, METRICS.row_gap))
}

/// A vertical scroll area of a token height that keeps its full width.
pub fn list_area(id_salt: impl egui::AsIdSalt, height: ListHeight) -> egui::ScrollArea {
    egui::ScrollArea::vertical()
        .id_salt(id_salt)
        .max_height(height.px())
        .auto_shrink([false, true])
}

/// An icon button whose tooltip is `label`; when `disabled_reason` is set
/// the button is disabled and the tooltip explains why.
pub fn icon_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    disabled_reason: Option<&str>,
) -> Response {
    let disabled_text = match disabled_reason {
        Some(reason) if !reason.is_empty() => format!("{label}: {reason}"),
        _ => label.to_owned(),
    };
    crate::icons::button_with_disabled_text(
        ui,
        disabled_reason.is_none(),
        icon,
        label,
        disabled_text,
    )
}

/// An icon button that stays highlighted while `selected`, for toolbar
/// toggles such as an open drawer.
pub fn icon_toggle(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    selected: bool,
    disabled_reason: Option<&str>,
) -> Response {
    let button = egui::Button::selectable(selected, crate::icons::text(icon, METRICS.icon_size))
        .frame_when_inactive(true)
        .min_size(Vec2::splat(METRICS.icon_button_size));
    let response = ui.add_enabled(disabled_reason.is_none(), button);
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            disabled_reason.is_none(),
            selected,
            label,
        )
    });
    let disabled_text =
        disabled_reason.map_or_else(|| label.to_owned(), |reason| format!("{label}: {reason}"));
    crate::icons::button_tooltips(ui, response, label, disabled_text)
}

/// Hosts a pane's content in exactly the space it is given.
///
/// Overflowing content is clipped rather than widening a side panel, and
/// the content keeps its widget state (collapsed sections, scroll offsets)
/// when the same `scope` moves between the side dock and the central area.
/// Show each `scope` at most once per frame.
pub fn pane<R>(ui: &mut Ui, scope: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let rect = ui.available_rect_before_wrap();
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .scope_id(egui::Id::unique(("ferrule_pane", scope)))
            .max_rect(rect),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let inner = add(&mut child);
    ui.allocate_rect(rect, egui::Sense::hover());
    inner
}

/// Response of a [`tab`].
#[allow(dead_code, reason = "hook for the mapping tab strip (area D)")]
pub struct TabResponse {
    pub response: Response,
    pub close_clicked: bool,
}

/// A document tab: the label and an optional inline close button share one
/// frame; the selected tab is filled with the selection color.
#[allow(dead_code, reason = "hook for the mapping tab strip (area D)")]
pub fn tab(ui: &mut Ui, label: &str, selected: bool, closable: bool) -> TabResponse {
    let visuals = ui.visuals();
    let fill = if selected {
        visuals.selection.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    let stroke = if selected {
        egui::Stroke::NONE
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    let mut close_clicked = false;
    let inner = egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(METRICS.radius)
        .inner_margin(egui::Margin::symmetric(space::MD as i8, space::XS as i8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = space::SM;
                let label = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
                if closable
                    && ui
                        .add(
                            egui::Button::new(crate::icons::text(Icon::X, METRICS.icon_small))
                                .frame(false),
                        )
                        .on_hover_text("Close tab")
                        .clicked()
                {
                    close_clicked = true;
                }
                label
            })
            .inner
        });
    let response = inner
        .inner
        .union(inner.response.interact(egui::Sense::click()));
    TabResponse {
        response,
        close_clicked,
    }
}

/// How a dialog is placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    /// Confirmations and wizards: centred over a backdrop that blocks the
    /// rest of the editor; Escape or a backdrop click requests closing.
    Modal,
    /// Results, settings and inspectors: docked to the right edge of the
    /// workspace below the toolbar, leaving the editor usable.
    Docked,
}

/// Where a dialog goes for a given viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DialogPlacement {
    /// Which point of the dialog sits at `position`.
    pub pivot: Align2,
    pub position: Pos2,
    pub width: f32,
    pub max_height: f32,
}

/// Applies the placement policy: modal dialogs centre in the viewport,
/// docked dialogs hang from the top-right of the workspace; both keep
/// [`Metrics::dialog_margin`](crate::theme::Metrics) from the edges they
/// use and never exceed them.
pub fn dialog_placement(
    kind: DialogKind,
    width: DialogWidth,
    viewport: Rect,
    workspace: Rect,
) -> DialogPlacement {
    let margin = METRICS.dialog_margin;
    let area = match kind {
        DialogKind::Modal => viewport,
        DialogKind::Docked => workspace.intersect(viewport),
    };
    let width = width.px().min(area.width() - 2.0 * margin).max(0.0);
    let max_height = (area.height() - 2.0 * margin).max(0.0);
    match kind {
        DialogKind::Modal => DialogPlacement {
            pivot: Align2::CENTER_CENTER,
            position: area.center(),
            width,
            max_height,
        },
        DialogKind::Docked => DialogPlacement {
            pivot: Align2::RIGHT_TOP,
            position: area.right_top() + vec2(-margin, margin),
            width,
            max_height,
        },
    }
}

fn workspace_rect_id() -> egui::Id {
    egui::Id::unique("ferrule_workspace_rect")
}

/// Records the area between the shell's bars for docked dialogs; call it
/// once per frame after the top and bottom panels.
pub fn set_workspace_rect(ctx: &egui::Context, rect: Rect) {
    ctx.data_mut(|data| data.insert_temp(workspace_rect_id(), rect));
}

fn workspace_rect(ctx: &egui::Context) -> Rect {
    ctx.data(|data| data.get_temp(workspace_rect_id()))
        .unwrap_or_else(|| ctx.content_rect())
}

/// A dialog under construction; see [`dialog`].
#[must_use = "show the dialog with `show`"]
pub struct Dialog<'c> {
    ctx: &'c egui::Context,
    kind: DialogKind,
    title: String,
    id: egui::Id,
    width: DialogWidth,
}

/// What happened in a dialog this frame.
pub struct DialogResponse<B, F> {
    #[allow(
        dead_code,
        reason = "dialogs that return a value from their body (areas B–E)"
    )]
    pub body: B,
    pub footer: F,
    /// The close button, Escape or a backdrop click asked to close.
    pub close_requested: bool,
}

/// Starts a dialog titled `title`. Its id derives from the title unless
/// [`Dialog::id_salt`] sets one.
pub fn dialog<'c>(
    ctx: &'c egui::Context,
    kind: DialogKind,
    title: impl Into<String>,
) -> Dialog<'c> {
    let title = title.into();
    Dialog {
        ctx,
        kind,
        id: egui::Id::unique(("ferrule_dialog", &title)),
        title,
        width: DialogWidth::Narrow,
    }
}

impl Dialog<'_> {
    pub fn id_salt(mut self, salt: impl egui::AsIdSalt) -> Self {
        self.id = egui::Id::unique(("ferrule_dialog", salt));
        self
    }

    pub fn width(mut self, width: DialogWidth) -> Self {
        self.width = width;
        self
    }

    /// Shows the title row, the scrolling `body` and the pinned `footer`
    /// (usually [`dialog_buttons`]).
    pub fn show<B, F: Default>(
        self,
        body: impl FnOnce(&mut Ui) -> B,
        footer: impl FnOnce(&mut Ui) -> F,
    ) -> DialogResponse<B, F> {
        self.show_parts(body, footer, true)
    }

    /// Shows a dialog without a footer, such as a settings drawer whose
    /// changes apply immediately.
    pub fn show_body<B>(self, body: impl FnOnce(&mut Ui) -> B) -> DialogResponse<B, ()> {
        self.show_parts(body, |_| (), false)
    }

    fn show_parts<B, F: Default>(
        self,
        body: impl FnOnce(&mut Ui) -> B,
        footer: impl FnOnce(&mut Ui) -> F,
        has_footer: bool,
    ) -> DialogResponse<B, F> {
        let Self {
            ctx,
            kind,
            title,
            id,
            width,
        } = self;
        let placement = dialog_placement(kind, width, ctx.content_rect(), workspace_rect(ctx));
        let frame = egui::Frame::window(&ctx.global_style())
            .inner_margin(METRICS.window_padding)
            .corner_radius(METRICS.radius_window);
        let content_width = (placement.width - frame.total_margin().sum().x).max(0.0);
        let content_height = (placement.max_height - frame.total_margin().sum().y).max(0.0);
        let mut close_requested = false;
        let contents = |ui: &mut Ui| {
            ui.set_width(content_width);
            ui.set_max_height(content_height);
            let footer = has_footer.then_some(footer);
            dialog_contents(
                ui,
                id,
                &title,
                content_height,
                &mut close_requested,
                body,
                footer,
            )
        };
        let (body, footer) = match kind {
            DialogKind::Modal => {
                let modal = egui::Modal::new(id).frame(frame).show(ctx, contents);
                close_requested |= modal.should_close();
                modal.inner
            }
            DialogKind::Docked => {
                egui::Area::new(id)
                    .kind(egui::UiKind::Window)
                    .order(egui::Order::Middle)
                    .pivot(placement.pivot)
                    .fixed_pos(placement.position)
                    .show(ctx, |ui| frame.show(ui, contents).inner)
                    .inner
            }
        };
        DialogResponse {
            body,
            footer,
            close_requested,
        }
    }
}

fn dialog_contents<B, F>(
    ui: &mut Ui,
    id: egui::Id,
    title: &str,
    content_height: f32,
    close_requested: &mut bool,
    body: impl FnOnce(&mut Ui) -> B,
    footer: Option<impl FnOnce(&mut Ui) -> F>,
) -> (B, F)
where
    F: Default,
{
    let header_top = ui.cursor().top();
    ui.horizontal(|ui| {
        ui.label(TextRole::Title.text(title));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if icon_button(ui, Icon::X, "Close", None).clicked() {
                *close_requested = true;
            }
        });
    });
    ui.separator();
    let header_height = ui.cursor().top() - header_top;
    // The footer is drawn after the body, so its height comes from the
    // previous frame; the first frame assumes one row of buttons.
    let footer_id = id.with("footer_height");
    let footer_height = if footer.is_some() {
        ui.ctx()
            .data(|data| data.get_temp::<f32>(footer_id))
            .unwrap_or(METRICS.control_height + space::MD * 2.0)
    } else {
        0.0
    };
    let body_height = (content_height - header_height - footer_height).max(METRICS.control_height);
    let body = egui::ScrollArea::vertical()
        .id_salt(id.with("body"))
        .max_height(body_height)
        .auto_shrink([false, true])
        .show(ui, body)
        .inner;
    let Some(footer) = footer else {
        return (body, F::default());
    };
    let footer_top = ui.cursor().top();
    ui.separator();
    let footer = footer(ui);
    let measured = ui.min_rect().bottom() - footer_top;
    ui.ctx()
        .data_mut(|data| data.insert_temp(footer_id, measured));
    (body, footer)
}

/// One button of [`dialog_buttons`].
#[derive(Clone, Copy, Debug)]
pub struct DialogButton<'a> {
    pub label: &'a str,
    /// Why the button is disabled; `None` enables it.
    pub disabled_reason: Option<&'a str>,
}

impl<'a> DialogButton<'a> {
    pub const fn new(label: &'a str) -> Self {
        Self {
            label,
            disabled_reason: None,
        }
    }

    /// Disables the button with `reason` as its tooltip when `disabled`.
    pub const fn disabled_if(mut self, disabled: bool, reason: &'a str) -> Self {
        if disabled {
            self.disabled_reason = Some(reason);
        }
        self
    }
}

impl<'a> From<&'a str> for DialogButton<'a> {
    fn from(label: &'a str) -> Self {
        Self::new(label)
    }
}

/// Which dialog button was clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogAction {
    Primary,
    Secondary,
    Destructive,
}

/// The standard dialog footer: the destructive action alone on the left in
/// the error color, then the secondary action and the filled primary action
/// right-aligned, primary last.
pub fn dialog_buttons(
    ui: &mut Ui,
    primary: DialogButton<'_>,
    secondary: Option<DialogButton<'_>>,
    destructive: Option<DialogButton<'_>>,
) -> Option<DialogAction> {
    let palette = crate::theme::active_palette(ui.ctx());
    let mut action = None;
    ui.horizontal(|ui| {
        if let Some(destructive) = destructive {
            let button = egui::Button::new(RichText::new(destructive.label).color(palette.error))
                .stroke(egui::Stroke::new(1.0, palette.error));
            if add_dialog_button(ui, button, destructive).clicked() {
                action = Some(DialogAction::Destructive);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let button = egui::Button::new(RichText::new(primary.label).color(palette.on_action))
                .fill(palette.action);
            if add_dialog_button(ui, button, primary).clicked() {
                action = Some(DialogAction::Primary);
            }
            if let Some(secondary) = secondary
                && add_dialog_button(ui, egui::Button::new(secondary.label), secondary).clicked()
            {
                action = Some(DialogAction::Secondary);
            }
        });
    });
    action
}

fn add_dialog_button(ui: &mut Ui, button: egui::Button<'_>, spec: DialogButton<'_>) -> Response {
    let response = ui.add_enabled(
        spec.disabled_reason.is_none(),
        button.min_size(Vec2::new(
            METRICS.min_grid_col * 1.5,
            METRICS.control_height,
        )),
    );
    match spec.disabled_reason {
        Some(reason) => response.on_disabled_hover_text(reason),
        None => response,
    }
}

#[cfg(test)]
#[path = "ui_kit_tests.rs"]
mod tests;
