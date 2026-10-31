//! Keyboard focus around modal dialogs: a dialog takes focus once it is
//! drawn, and returns it to the control that opened it when it closes.
//!
//! Callers show a dialog only while it is open, so closing is detected as a
//! pass that no longer shows it.

use egui::{Context, Id, LayerId, Ui, ViewportId};

/// A modal dialog shown in the current or previous pass.
#[derive(Clone, Copy, Debug)]
struct OpenModal {
    id: Id,
    viewport: ViewportId,
    /// The dialog's layer, once drawn.
    layer: Option<LayerId>,
    /// The control focused when the dialog opened, and that control's layer.
    restore: Option<Id>,
    restore_layer: Option<LayerId>,
    opened_pass: u64,
    shown_pass: u64,
    /// A control of the dialog received the initial focus.
    entered: bool,
}

#[derive(Clone, Default)]
struct OpenModals(Vec<OpenModal>);

fn open_modals_id() -> Id {
    Id::unique("ferrule_open_modals")
}

/// Registers that modal `id` is shown in this pass and reports whether it
/// should move focus to its first control now. That starts in the pass after
/// it opens, so the key press that opened it cannot also activate a control.
pub(super) fn shown(ctx: &Context, id: Id) -> bool {
    if ctx.with_plugin(|_: &mut RestoreFocus| ()).is_none() {
        ctx.add_plugin(RestoreFocus);
    }
    let viewport = ctx.viewport_id();
    let pass = ctx.cumulative_pass_nr();
    let restore = ctx.memory(|memory| memory.focused());
    let restore_layer = restore
        .and_then(|focused| ctx.read_response(focused))
        .map(|response| response.layer_id);
    let opened = OpenModal {
        id,
        viewport,
        layer: None,
        restore,
        restore_layer,
        opened_pass: pass,
        shown_pass: pass,
        entered: false,
    };
    ctx.data_mut(|data| {
        let modals = &mut data
            .get_temp_mut_or_default::<OpenModals>(open_modals_id())
            .0;
        let index = match modals
            .iter()
            .position(|modal| modal.id == id && modal.viewport == viewport)
        {
            // Hidden for a whole pass without being restored: opened again.
            Some(index) if modals[index].shown_pass + 1 < pass => {
                modals[index] = opened;
                index
            }
            Some(index) => index,
            None => {
                modals.push(opened);
                modals.len() - 1
            }
        };
        let modal = &mut modals[index];
        modal.shown_pass = pass;
        !modal.entered && pass > modal.opened_pass
    })
}

/// Records the drawn dialog's layer and whether it placed its initial focus.
pub(super) fn record(ctx: &Context, id: Id, layer: LayerId, entered: bool) {
    let viewport = ctx.viewport_id();
    ctx.data_mut(|data| {
        if let Some(modal) = data
            .get_temp_mut_or_default::<OpenModals>(open_modals_id())
            .0
            .iter_mut()
            .find(|modal| modal.id == id && modal.viewport == viewport)
        {
            modal.layer = Some(layer);
            modal.entered |= entered;
        }
    });
}

/// Restores focus after each modal dialog that this pass no longer showed.
///
/// That is one pass after the dialog's last frame: egui keeps blocking the
/// layers below a modal for one more pass and drops focus given to them.
struct RestoreFocus;

impl egui::Plugin for RestoreFocus {
    fn debug_name(&self) -> &'static str {
        "ferrule_modal_focus"
    }

    fn on_end_pass(&mut self, ui: &mut Ui) {
        restore_closed(ui.ctx());
    }
}

fn restore_closed(ctx: &Context) {
    let viewport = ctx.viewport_id();
    let pass = ctx.cumulative_pass_nr();
    let closed = ctx.data_mut(|data| {
        let modals = data.get_temp_mut_or_default::<OpenModals>(open_modals_id());
        let (closed, open) = std::mem::take(&mut modals.0)
            .into_iter()
            .partition(|modal| modal.viewport == viewport && modal.shown_pass != pass);
        modals.0 = open;
        closed
    });
    for modal in closed {
        // A dialog opened from this one returns focus to this one's opener.
        if modal.layer.is_some() {
            ctx.data_mut(|data| {
                for opened in data
                    .get_temp_mut_or_default::<OpenModals>(open_modals_id())
                    .0
                    .iter_mut()
                    .filter(|opened| opened.restore_layer == modal.layer)
                {
                    opened.restore = modal.restore;
                    opened.restore_layer = modal.restore_layer;
                }
            });
        }
        let Some(restore) = modal.restore else {
            continue;
        };
        // Keep focus that the dialog's action moved elsewhere, and skip an
        // opener that is gone, such as an item of a closed menu.
        let in_dialog = ctx.memory(|memory| memory.focused()).is_none_or(|focused| {
            ctx.read_response(focused)
                .is_none_or(|response| Some(response.layer_id) == modal.layer)
        });
        if in_dialog && ctx.read_response(restore).is_some() {
            ctx.memory_mut(|memory| memory.request_focus(restore));
        }
    }
}
