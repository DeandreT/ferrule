# egui-scale 0.6.1 — egui 0.37 compatibility

The library source is from the published `egui-scale` 0.6.1 crate, upstream
<https://github.com/zakarumych/egui-scale>, revision
`37a8647d5e3af5fddf4407f673d2d7f4c58dc253`. The downloaded crate's SHA256 is
`16194efd2800275df80448afa0ee11d3e23ce0220f6754e1af53fec8e9b7467a`.

This copy targets egui 0.37 and omits the removed `Visuals::clip_rect_margin`.
Ferrule selects it with a workspace Cargo patch so the graph component uses
the same egui types as the application. Refresh from the published upstream
package when it supports that egui release.

Upstream declares `MIT OR Apache-2.0`; the published archive contains no separate
license files or copyright notice. The Apache-2.0 license text is included at
[`../egui-snarl/LICENSE-APACHE`](../egui-snarl/LICENSE-APACHE).
