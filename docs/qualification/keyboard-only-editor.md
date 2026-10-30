# Keyboard-only editor qualification

The tests in `crates/gui/src/app/keyboard_accessibility_tests.rs` drive the
complete desktop application with key and text events only. They run headless in
an `egui_kittest` harness at 1280 × 800, with no display, on a self-authored
`Order { Name, Note }` project written to a temporary directory. Fixtures load
through the app's own open entry point; every later step uses the keyboard.

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
| Select a canvas node | Tab, F2 | Inaccessible control | `keyboard_accessibility_gap_selects_a_canvas_node_for_f2` |
| Edit a binding property | Tab to the target-field combo box, Enter, Tab, Enter | Supported | `keyboard_accessibility_edits_a_binding_with_undo_and_redo_shortcuts` |
| Edit node settings | Needs a selected node or the node Edit button | Inaccessible control | `keyboard_accessibility_gap_selects_a_canvas_node_for_f2`, `keyboard_accessibility_gap_tab_reaches_every_canvas_control` |
| Add a connection | Tab to Add binding, Enter; or Tab to Mapping, Enter, Tab to Auto-connect fields…, Enter, Tab to Apply connections, Enter | Supported | `keyboard_accessibility_adds_connections_from_the_inspector_and_mapping_menu` |
| Draw a canvas wire | Pointer drag only; pins have no names | Inaccessible control | `keyboard_accessibility_gap_tab_stops_have_readable_names` |
| Undo and redo | Ctrl+Z, Ctrl+Shift+Z, Ctrl+Y | Supported | `keyboard_accessibility_edits_a_binding_with_undo_and_redo_shortcuts` |
| Save and reopen | Ctrl+S; Tab to the recent entry, Enter | Supported (Save As uses the native chooser) | `keyboard_accessibility_saves_and_reopens_a_keyboard_edit` |
| Preview | Tab to the Preview toolbar button, Enter; Tab and type both identities and the input; Escape, Shift+Tab three times to Preview, Enter | Missing shortcut | `keyboard_accessibility_previews_the_unsaved_mapping`, `keyboard_accessibility_gap_preview_has_a_shortcut`, `keyboard_accessibility_gap_tab_leaves_the_preview_input` |

## Focus behaviour

| Check | Result | Test |
| --- | --- | --- |
| Named menus, toolbar buttons, combo boxes, check boxes and buttons are painted in the focused widget style | Supported | `keyboard_accessibility_focused_controls_draw_a_focus_indicator` |
| Every Tab stop has visible bounds | Gap | `keyboard_accessibility_gap_every_tab_stop_is_visible` |
| Tab runs menu bar, toolbar, mapping tabs, source pane, inspector, then canvas; Shift+Tab exactly reverses it | Supported | `keyboard_accessibility_tab_order_is_regional_and_shift_tab_reverses_it` |
| Forward Tab through the canvas | Gap | `keyboard_accessibility_gap_tab_reaches_every_canvas_control` |
| Shift+Tab out of the canvas | Gap | `keyboard_accessibility_gap_shift_tab_leaves_the_canvas` |
| Tab stays inside a modal dialog | Supported | `keyboard_accessibility_open_shortcut_keeps_unsaved_work_when_cancelled` |
| Focus moves into a dialog when it opens: its first body control, otherwise its default action | Supported | `keyboard_accessibility_gap_opening_a_dialog_focuses_it` |
| Focus returns to the trigger when a dialog closes | Supported | `keyboard_accessibility_gap_closing_a_dialog_restores_focus` |
| Dialog footer Tab order matches the visual order | Supported | `keyboard_accessibility_gap_dialog_footer_tab_order_is_visual` |
| Escape closes modal dialogs as Cancel | Supported | `keyboard_accessibility_gap_escape_closes_modal_dialogs` |
| Cancel keeps unsaved work, dirty state and history | Supported | `keyboard_accessibility_open_shortcut_keeps_unsaved_work_when_cancelled` |
| Activating a control keeps focus | Gap (Add binding) | `keyboard_accessibility_gap_add_binding_keeps_focus` |

egui's defaults also apply: arrow keys move focus between neighbouring controls,
Enter or Space activates the focused control, and Escape clears focus. Canvas
commands (F2, Delete, Ctrl+F) run only while no control has keyboard focus.

## Candidate product issues

1. **Select canvas nodes from the keyboard.** Nodes are selected only with the
   pointer, and F2, Delete and Ctrl+F run only when nothing has keyboard focus,
   so node settings and node commands have no keyboard path.
2. **Let forward Tab traverse the whole canvas.** Forward Tab drops focus inside
   the canvas and restarts at the menu bar. The zoom, fit and minimap controls
   are never reached, and the node Edit button only on some passes.
3. **Release Shift+Tab from the canvas.** Shift+Tab cycles among canvas controls
   and never returns to the inspector, toolbar or menu bar.
4. **Give the mapping tab one visible Tab stop.** Focus first lands on the tab's
   clickable container, which reports no bounds or name, before the tab label.
5. **Close modal dialogs with Escape.** Fixed by #1164.
6. **Move focus into dialogs when they open.** Fixed by #1164.
7. **Return focus when a dialog closes.** Fixed by #1164.
8. **Match dialog footer Tab order to the visual order.** Fixed by #1164.
9. **Let Tab leave the Preview input.** The Primary input inserts a tab character
   for Tab; the dialog buttons are reachable only with Escape, then Shift+Tab.
10. **Name icon-only and canvas focus stops.** Add source and the minimap toggle
    expose an icon glyph as their name; canvas pins, node frames, collapse
    buttons and the scope-tree resize corner have no name.
11. **Keep focus on Add binding after activation.** Activating Add binding drops
    keyboard focus, so the next Tab restarts at the menu bar.
12. **Add a Preview shortcut.** Run and Validate have shortcuts; Preview is
    reachable only through the toolbar or the Run menu.
