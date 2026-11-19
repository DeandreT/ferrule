# Outbound 945: two cartons leave Morrow Depot

Open `project.json` in Ferrule's **Open Project** dialog. Run it with the supplied `shipment.json` to write `actual.945.x12`. The project embeds the source schema and a small, independently authored X12 004010 layout; the sample needs no separately installed EDI catalog.

From this directory, the equivalent CLI command is:

```sh
ferrule run project.json shipment.json actual.945.x12
```

Compare the result byte-for-byte with `expected.945.x12` (`cmp expected.945.x12 actual.945.x12`, or `fc.exe /b expected.945.x12 actual.945.x12` on Windows). The wire uses `*` between elements, `~` followed by LF after every segment, and significant spaces in ISA fields.

The fictional warehouse sends one completed shipment to Skyvale. `W06` carries the order reference, ship date, shipment ID and purchase order. Each carton holds one SKU and drives one `LX` / `MAN` / `W12` group in input order. `MAN*CP` identifies the carton's carrier tracking number. `W12` maps ordered quantity, shipped quantity, unit of measure and SKU. The two cartons ship six individual flasks and two cases of mugs.

The trailer counts **segments**, not cartons: `SE01` is `3 + 3 × carton count` (`ST`, `W06` and `SE`, plus three segments per carton). The input supplies three separate control numbers, reused in `ST/SE`, `GS/GE` and `ISA/IEA`.

To see the mapping respond, copy the input, remove the first carton and change the remaining carton's `tracking`, `sku`, `ordered`, `shipped` and `uom`. Run against the copy. Its values should appear together in one detail group, with `SE*6*0741~` instead of `SE*9*0741~`. `sequence` is supplied by the warehouse and is preserved even if other cartons are removed.
