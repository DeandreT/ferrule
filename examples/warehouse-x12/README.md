# Warehouse X12 mappings

A warehouse receives an order telling it what to ship. Later, it sends a shipment confirmation telling the order owner what actually shipped. These examples follow those two field-mapping patterns using fictional data.

- [Inbound 940: warehouse shipping order → JSON](inbound-940/). Start here to see how an X12 document becomes an order that an application can use.
- [Outbound 945: shipment JSON → warehouse shipping advice](outbound-945/). Start here to see the reverse direction: application fields populate an X12 document.

In Ferrule GUI, choose **Open project…** and select the `project.json` for the direction you want. Follow a line item from the source to the target, then change its quantity in a copy of the sample input and run it again. For a 940, the quantity is the amount *requested*; for a 945, it is the amount *shipped*. A short shipment is a useful way to see the distinction.

Each example's README includes its CLI command and expected output. Use a fresh output file so you can compare your result with the checked-in reference. The examples run locally; no trading-partner connection or credentials are needed.

If you change the EDI by hand, run `python check_envelope.py 940 <input-file>` or `python check_envelope.py 945 <output-file>` from this directory. It checks the control numbers and counts for this single-transaction layout. For example, adding a detail segment without increasing `SE01` should fail the check.
