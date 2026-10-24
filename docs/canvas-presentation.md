# Canvas density and connection tools

Choose **Standard**, **Compact**, or **Dense** in **Appearance → Canvas**.
These preferences change canvas text, padding and pin presentation, not menus,
dialogs, the inspector or mapping data. Compact is the default. A small painted
pin retains a larger native hit target.

Constants fit measured text, with a bounded single-line preview and access to the
complete literal in the editor and tooltip. Empty strings are quoted, and escapes
make significant whitespace visible. The constant editor labels its type and value,
offers multiline text, and closes with **Done**, the close button, or Escape.
Exists and Not remain separate operations;
If / else retains its condition, then and else input order. Hover a card or focus
it to reveal its header pencil, or select a node and press F2. The node context
menu offers a manual card width and **Reset to auto size**.

## Camera and navigation

Opening a canvas centers its endpoints and connected mapping at a readable 60–100% zoom rather
than shrinking a large graph to unreadable text. **Fit all nodes** explicitly
shows the complete graph, with up to 125% magnification. Neither operation moves
nodes. The corner controls offer small zoom steps and a percentage readout;
click the percentage to return to 100%. Ctrl+wheel zooms around the pointer.
Each tab keeps its camera when switching tabs or resizing panels.

The minimap stays on the canvas, below dialogs. Its chevron collapses it to a
small map button. Click the map to focus a location, drag to pan, or drag its
top-left grip to resize it.

## Focus and trace

Click a wire to focus that exact source/destination connection. Click a node to
focus only its directly incident connections. Click the focused item again,
click empty canvas, press Escape, or use **Clear focus** to clear it. Popups and
text editing take priority over canvas shortcuts. Wiring, dragging, resizing and
panning remain separate gestures.

The breadcrumb identifies the source and destination. **Inputs**, **Outputs** and
**Both** expand focus through static directed wiring, with cycle protection.
All connected conditional branches are included. This is not an execution trace
and does not identify a branch that ran or a value that was produced.

When several curves overlap, choose a connection instead of guessing which one
was clicked. A fan-out badge also opens the connection picker. Arrow keys move
through its entries and keep the selected row visible, including wraparound.
Enter focuses one, and Escape closes the picker first.
Choosing focus never disconnects an edge. A focused connection hidden behind an
endpoint proxy is revealed so it can be distinguished from the other connections.

## Endpoint fields

**All fields** and **Connected fields** are view filters. Counts identify displayed
fields relative to the complete schema. Field identity and schema order remain
unchanged; filtered visual row numbers are not semantic port indices.

Ctrl+F searches the selected endpoint's complete schema, including unconnected
fields. A matching unconnected field is temporarily revealed; clearing or closing
the search, or switching endpoints, clears that exception. Connecting that field
makes it a normal connected field.

Scrollbar paint, thumb and hit bounds use the measured real-field list, below
the complete header and above proxy/footer controls. Tiny or invalid geometry
does not create an inverted thumb-size clamp or an unbounded hit area.

## Layout

**Layout → Auto-arrange · preview** stages outlined positions without changing the saved
layout. It uses current measured node and pin geometry and the chosen density;
it does not shrink text or zoom out to make a layout fit. It extends the existing
deterministic, wire-aware arranger with measured lane alignment, routing gutters
and collision handling.

Choose the whole canvas or **Selected region**, and use **Pin / unpin selected**
to keep anchors fixed. **Apply layout** changes positions together. **Cancel** changes
nothing. **Undo layout** and **Redo layout** restore each complete placement in
one step without changing mapping nodes, ports, argument order or wires. Focus,
pinning and manual card widths are editor view state; density is a persisted user
preference. Normal edits and loading an existing layout do not invoke auto-arrange.
Disconnected cards use a compact shelf below the connected graph on initial
arrangement; saved sidecar positions are restored unchanged.

Preview waits for valid measured geometry and is bounded to 2,000 nodes and
8,000 displayed edges. Graph edits invalidate a stale proposal; changed card
geometry, manual dragging or changed anchors require another preview. Cycles and disconnected components remain
separate, real connections. Arbitrary graphs have no optimal-layout or
zero-crossing guarantee.

## Dependency boundary

The root Cargo override selects the licensed `vendor/egui-snarl` source. Its
viewer hooks extend native per-curve hit testing and rendering; the GUI does not
overlay a competing full-canvas hit target. `node_geometry` reports the complete
header frame and actual pin centers in graph coordinates before `final_node_rect`.
Do not patch a Cargo registry cache. See the vendor README for provenance and
the reproducible patch boundary.

egui, eframe and egui_kittest use the 0.37 release family (Rust 1.95 or later).
The workspace explicitly enables egui's `default_fonts`. `vendor/egui-scale`
adapts the graph's scaling dependency to the same egui types; its README records
the upstream source and compatibility changes.

Use `examples/canvas-density/project.json` as a self-authored visual fixture.

## Window-free visual checks

The opt-in `canvas_offscreen_release_interactions` test renders the authored
canvas through egui_kittest and egui-wgpu into an offscreen texture. It creates no native window
or window surface and sends no operating-system input. Each scenario uses a
fresh in-memory editor, so it does not load or store desktop preferences or open
a user's mapping.

Set `FERRULE_OFFSCREEN_DIR` to a new absolute directory beneath an existing
parent, then run this through the normal native Cargo-slot wrapper:

```text
cargo test --release --locked --offline -p gui --bin ferrule-gui canvas_offscreen_release_interactions -- --ignored --nocapture --test-threads=1
```

The test requires a compatible offscreen graphics adapter. It writes new PNGs
for each density at 100% and 150% scale, including the real connection picker,
exact focus, static tracing, layout preview/apply and connected-field filtering.
It also checks cancellation, layout undo/redo and mapping preservation. Inspect
the rendered images for clipping and readability; a nonblank image alone is not
visual acceptance. These checks exercise the canvas and renderer independently
of native window activation, dialogs, compositor behavior and desktop input.
