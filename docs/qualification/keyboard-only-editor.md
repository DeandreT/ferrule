# Keyboard-only editor qualification

The tests in `crates/gui/src/app/keyboard_accessibility_tests.rs` and its
`canvas_navigation` module drive the complete desktop application with key and
text events only. They run headless in an `egui_kittest` harness at 1280 × 800,
with no display, on self-authored projects written to a temporary directory:
`Order { Name, Note }`, the same with a concat node, and an `Order` with 20
fields, more than an endpoint shows at once. Fixtures load through the app's own
open entry point; every later step uses the keyboard.

The native Open and Save As choosers belong to the operating system and are not
covered. The tests press Ctrl+O only with unsaved work, so the shortcut stops at
Ferrule's own confirmation dialog.

Supported paths are ordinary tests. Each gap is an ignored test that asserts the
desired behaviour, so it fails until the product is fixed:

```sh
cargo nextest run --profile ci --locked -p gui -E 'test(keyboard_accessibility)'
cargo nextest run --profile ci --locked -p gui -E 'test(keyboard_accessibility)' --run-ignored only
```

When a fix lands, remove that test's `#[ignore]` and update this page.

## Workflows

Result classes: **supported**, **missing shortcut** (works by Tab but has no
direct key), **inaccessible control** (no keyboard path).

| Workflow | Keys | Result | Test |
| --- | --- | --- | --- |
| Open a recent project | Tab to the entry on the start view, Enter | Supported | `keyboard_accessibility_opens_a_recent_project_from_the_start_view` |
| Open with unsaved work | Ctrl+O, Tab within the dialog, Tab to Cancel, Enter | Supported up to the native chooser | `keyboard_accessibility_open_shortcut_keeps_unsaved_work_when_cancelled` |
| Select a canvas node | Tab to the node or one of its controls, F2; Enter or Space on the node selects it | Supported | `keyboard_accessibility_gap_selects_a_canvas_node_for_f2` |
| Edit a binding property | Tab to the target-field combo box, Enter, Tab, Enter | Supported | `keyboard_accessibility_edits_a_binding_with_undo_and_redo_shortcuts` |
| Edit node settings | Tab to the node, F2; or Tab to its Edit button, Enter | Supported | `keyboard_accessibility_gap_selects_a_canvas_node_for_f2`, `keyboard_accessibility_gap_tab_reaches_every_canvas_control` |
| Add a connection | Tab to Add binding, Enter; or Tab to Mapping, Enter, Tab to Auto-connect fields…, Enter, Tab to Apply connections, Enter | Supported | `keyboard_accessibility_adds_connections_from_the_inspector_and_mapping_menu` |
| Draw a canvas wire | Tab to a pin, Enter; Tab to a pin of the other direction, Enter; Escape cancels | Supported | `keyboard_accessibility_connects_two_fields_on_the_canvas`, `keyboard_accessibility_escape_cancels_a_canvas_connection`, `keyboard_accessibility_gap_canvas_tab_stops_have_readable_names` |
| Undo and redo | Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y | Supported | `keyboard_accessibility_edits_a_binding_with_undo_and_redo_shortcuts` |
| Save and reopen | Ctrl+S; Tab to the recent entry, Enter | Supported (Save As uses the native chooser) | `keyboard_accessibility_saves_and_reopens_a_keyboard_edit` |
| Preview | Ctrl+Shift+P, or Tab to the Preview toolbar button, Enter; Tab and type both identities and the input; Tab to Preview, Enter | Supported | `keyboard_accessibility_previews_the_unsaved_mapping`, `keyboard_accessibility_gap_preview_has_a_shortcut`, `keyboard_accessibility_preview_shortcut_opens_the_preview`, `keyboard_accessibility_gap_tab_leaves_the_preview_input` |
| Type a tab character in the Preview input | Ctrl+Tab | Supported | `keyboard_accessibility_ctrl_tab_types_a_tab_in_the_preview_input` |
| Move between canvas nodes and pins | Arrow keys | Supported | `keyboard_accessibility_arrows_move_between_canvas_nodes_in_layout_order`, `keyboard_accessibility_arrows_move_between_pins_of_a_node` |
| Reach endpoint fields scrolled out of view | Arrow keys on a field pin; or Tab to the endpoint scrollbar, arrow keys, Page Up/Down, Home, End | Supported | `keyboard_accessibility_arrows_scroll_hidden_endpoint_fields_into_view`, `keyboard_accessibility_endpoint_scrollbar_takes_scroll_keys` |
| Resize an endpoint | Tab to its width or height handle, arrow keys | Supported | `keyboard_accessibility_endpoint_resize_handles_take_arrow_keys` |
| Collapse or expand a node | Tab to its collapse button, Enter or Space | Supported | `keyboard_accessibility_enter_and_space_toggle_collapse` |
| Bring an off-screen node into view | Tab or an arrow key to it | Supported | `keyboard_accessibility_focus_pans_to_an_off_screen_node` |

## Focus behaviour

| Check | Result | Test |
| --- | --- | --- |
| Named menus, toolbar buttons, combo boxes, check boxes and buttons are painted in the focused widget style | Supported | `keyboard_accessibility_focused_controls_draw_a_focus_indicator` |
| Every Tab stop has visible bounds | Supported | `keyboard_accessibility_gap_every_tab_stop_is_visible` |
| Every Tab stop outside the canvas has a readable name | Supported | `keyboard_accessibility_gap_tab_stops_have_readable_names` |
| Every canvas Tab stop has a readable name | Supported | `keyboard_accessibility_gap_canvas_tab_stops_have_readable_names` |
| Tab runs menu bar, toolbar, mapping tabs, source pane, inspector, then canvas; Shift+Tab exactly reverses it | Supported | `keyboard_accessibility_tab_order_is_regional_and_shift_tab_reverses_it` |
| Forward Tab through the canvas | Supported | `keyboard_accessibility_gap_tab_reaches_every_canvas_control` |
| Shift+Tab out of the canvas | Supported | `keyboard_accessibility_gap_shift_tab_leaves_the_canvas` |
| Tab stays inside a modal dialog | Supported | `keyboard_accessibility_open_shortcut_keeps_unsaved_work_when_cancelled` |
| Focus moves into a dialog when it opens: its first body control, otherwise its default action | Supported | `keyboard_accessibility_gap_opening_a_dialog_focuses_it` |
| Focus returns to the trigger when a dialog closes | Supported | `keyboard_accessibility_gap_closing_a_dialog_restores_focus` |
| Dialog footer Tab order matches the visual order | Supported | `keyboard_accessibility_gap_dialog_footer_tab_order_is_visual` |
| Escape closes modal dialogs as Cancel | Supported | `keyboard_accessibility_gap_escape_closes_modal_dialogs` |
| Cancel keeps unsaved work, dirty state and history | Supported | `keyboard_accessibility_open_shortcut_keeps_unsaved_work_when_cancelled` |
| Activating Add binding moves focus to the new binding's target field | Supported | `keyboard_accessibility_gap_add_binding_keeps_focus` |

egui's defaults also apply: Enter or Space activates the focused control, and
Escape clears focus. Outside the canvas, arrow keys move focus between
neighbouring controls.

## Canvas keys

Forward Tab runs from the canvas surface through each node (its frame, input
pins, output pins, collapse button and header controls such as Edit or the
endpoint field filters, then an endpoint's scrollbar and its width and height
handles), then the zoom and fit buttons and the minimap, and then leaves the
canvas. Shift+Tab reverses it. A control focused by Tab or an arrow key outside
the view pans the canvas to it.

Arrow keys follow the graph layout rather than egui's spatial search:

- On a node frame, collapse button or Edit button they move to the frame of the
  nearest node in that direction, within 45° of it. Nodes whose extents overlap
  across the arrow count as in line, so a tall endpoint is level with every
  node beside it. Without a node in that direction focus stays.
- On a pin, Up and Down step through the pins on its side of the node. Inputs
  are on the left and outputs on the right: Right moves from an input to the
  output nearest its height, and Left from an output to the nearest input.
- Stepping from a visible endpoint field onto its "N hidden fields" pin scrolls
  the list by one row and focuses the field that comes into view, so arrow keys
  reach every field. The hidden-fields pins remain Tab stops.
- On the canvas surface arrow keys do nothing.

- Enter or Space on a node frame selects the node, as a click does. On a
  collapse button it collapses or expands the node.
- F2, Delete and Ctrl+F act on the node owning the focused frame, pin, collapse
  or Edit button. With another canvas control or nothing focused they act on
  the pointer selection as before. They never run while a text field has
  focus.
- Enter on a pin starts a connection and marks the pin. Enter on a pin of the
  other direction completes it through the same connection rules as a pointer
  drag, including type checks and undo. Enter on another pin of the same
  direction moves the start there, and Enter on the start pin cancels.
  Escape cancels and returns focus to the start pin.
- Canvas focus stops are named: pins as "Input Name · Target: Order" or
  "Output Name · Source: Order", node frames by the node title, and collapse
  buttons as "Collapse" or "Expand" with the node title.
- Where one output has several connections, Tab reaches its connection count
  button; Enter opens the connection picker, which takes the arrow keys, Enter
  and Escape.
- An endpoint's scrollbar ("Scroll fields · Source: Order") appears when its
  fields overflow. Up and Down scroll one row, Page Up and Page Down one page,
  Home and End to either end.
- An endpoint's width handle ("Resize width · Source: Order") takes Left and
  Right, moving its edge 10 units as a drag would. Its height handle ("Resize
  visible fields · Source: Order") takes Up and Down, showing one fewer or one
  more field. The corner grip combines both and is not a Tab stop.

## Candidate product issues

1. **Select canvas nodes from the keyboard.** Fixed by #1163.
2. **Let forward Tab traverse the whole canvas.** Fixed by #1163.
3. **Release Shift+Tab from the canvas.** Fixed by #1163.
4. **Give the mapping tab one visible Tab stop.** Fixed by #1165.
5. **Close modal dialogs with Escape.** Fixed by #1164.
6. **Move focus into dialogs when they open.** Fixed by #1164.
7. **Return focus when a dialog closes.** Fixed by #1164.
8. **Match dialog footer Tab order to the visual order.** Fixed by #1164.
9. **Let Tab leave the Preview input.** Fixed by #1165. Ctrl+Tab types a tab
   character.
10. **Name icon-only and canvas focus stops.** Icon-only buttons and the
    scope-tree resize corner: fixed by #1165. Canvas pins, node frames and
    collapse buttons: fixed by #1163.
11. **Keep focus on Add binding after activation.** Fixed by #1165. Focus moves
    to the new binding's target field.
12. **Add a Preview shortcut.** Fixed by #1165 with Ctrl+Shift+P.
13. **Follow the canvas layout with arrow keys; operate endpoint scrollbars and
    resize handles from the keyboard.** Fixed by #1211.
