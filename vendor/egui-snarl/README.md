# egui-snarl 0.11.0 — Ferrule patch

This is the library source from the crates.io `egui-snarl` 0.11.0 package
(`https://crates.io/crates/egui-snarl/0.11.0`), upstream
`https://github.com/zakarumych/egui-snarl`. The package's VCS metadata identifies
upstream revision `de6140750fcb5d7ccfb74b507e7b88fa1aca14ad`.
It was imported from the matching native Cargo registry source, without changing
the registry cache. Upstream's MIT and Apache notices are retained alongside it.

Only the library's compiled source modules are included. Examples, media,
development dependencies, registry bookkeeping, and the unused `ui/effect.rs`
module are omitted. The manifest targets egui 0.37 and egui-scale 0.6.1
(with the sibling compatibility patch); the optional probe feature retains its
upstream version. Ferrule selects this copy with its root
`[patch.crates-io]` entry. No machine-local source paths are used at build time.

The local patch adds backward-compatible `SnarlViewer` defaults:
`wire_color`, `wire_clicked`, `node_clicked`, `node_geometry`,
`background_clicked`, and `inline_node`.
Each connected edge can override its mixed endpoint color independently.
Primary wire clicks report all actual curve hits in endpoint-ID order using the
existing wire interaction response. Node clicks use the existing outer-frame
response. Native pin dragging, rectangle selection, panning, secondary wire
disconnects, and context menus remain separate. Geometry reports the measured
header frame and pin centers in graph coordinates, matching `final_node_rect`.
Inline cards retain their native individual pins and identities. Small visual
pins retain at least a 12-unit native interaction rectangle. A redundant legacy
`core::f32` import is omitted to use the current primitive associated constants.

To refresh, compare the library source and manifest against the same published
package, retain the licenses, and reapply only the hook additions and their
regression tests. Do not patch a Cargo registry cache or rely on it at runtime.

The egui 0.37 adaptation also uses `Id::unique`, `AsIdSalt` and `Ui::scope_id`.
The canvas accessibility subtree carries the graph-to-parent transform so its
widget bounds match the painted and pointer-interactive controls after pan/zoom.
The application uses the default and serde features; the optional probe UI is
not part of that integration.

`SnarlWidget::paint_layer_id` optionally separates the paint sublayer identity
from the widget state identity. Ferrule reuses bounded canvas layer slots while
retaining generation-specific camera, selection, and interaction state IDs.
The default retains upstream behavior when no override is supplied.
