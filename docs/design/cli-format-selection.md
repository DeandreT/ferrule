# CLI format selection

The private `format_selection` module in the CLI derives a typed codec selection
from the existing `FormatOptions` and logical path. Filesystem reads and writes,
payload reads and serialization, and pipeline preview use this boundary. It does
not change the saved project format or the public host API.

Explicit format declarations own precedence and conflict checking. Filesystem
input alone can fall back to HTTP XML before fixed-width and extension dispatch.
Recognized extensions still take precedence over tabular fallback identity.
Explicit versus fallback XML/JSON identity remains distinguishable because their
codec behavior and diagnostic contexts differ. In particular, an explicit JSON
document at a `.jsonl` path remains a document unless JSON Lines is selected.

Selection does not acquire data, parse embedded layouts, inspect EDI schemas, or
authorize a route. `RoutePlan` and prepared runs remain the admission authority;
each consumer retains its transport, input-only and persistent-state restrictions.
Inferred EDI dialect detection stays at the codec call site, after output lexical
formatting and value validation. Preview also retains its declaration-only checks
before a concrete output identity is available.

Common metadata validation and route-specific refusals deliberately precede
selection where they did before: filesystem authorization comes before input
metadata checks, while payload profile identity and single-payload file-set
refusals stay ahead of selected-format conflicts. Do not move schema validation,
layout parsing or transport work into the selector; that would change error order.
Metadata ignored by a legacy fallback remains ignored rather than being subjected
to a new blanket validation rule.
