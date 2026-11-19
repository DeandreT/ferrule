# Inbound 940: warehouse shipping order to JSON

Violet Mill asks Cedar Quay to pick 36 individual glass ferns and four cases of paper wrens. The sample is a fictional X12 004010 test interchange with one 940 transaction. Its two `LX`/`W01` detail loops become two JSON line items in wire order.

Open `project.json` in the Ferrule GUI to inspect the source, target and field mappings. The project contains both schemas. From this directory, run:

```sh
ferrule validate --project project.json
ferrule run project.json input.x12 order.json
```

Compare `order.json` with `expected.json` as JSON. The header order number comes from `W05-02`; the purchase order comes from `W05-03`. Each output line takes its number from `LX-01`, quantity from `W01-01`, unit from `W01-02`, and buyer's item number from `W01-05` (`W01-04` is `BP` in this sample). Quantities and line numbers are JSON integers.

To try a change, edit **only the second** `W01`: change `W01*4*CA` to `W01*9*EA`, then rerun. The second line should show `"quantity": 9` and `"uom": "EA"`; the first line and order header should stay the same.
