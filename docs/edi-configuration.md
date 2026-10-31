# EDI configuration compilation

`format-edi-config` is the design-time compiler for supported external EDI
configurations. `format-edi` contains the runtime codecs and validation for
X12, EDIFACT, HL7 v2, TRADACOMS, IDoc, and SWIFT MT. Dialects stay
together rather than becoming separate packages.

## Ownership and dependencies

The shared model has one owner: `ir` defines schemas and values, and `mapping`
defines serialized EDI constraints, lexical formats, IDoc descriptors/layouts,
and SWIFT layouts. The compiler returns those types in `CompiledConfig`,
`CompiledIdoc`, and `CompiledSwift`; the result containers and compiler errors
belong to `format-edi-config`. Runtime execution needs only the embedded schema
and `FormatOptions`, not those containers or the original configuration files.

The normal/build dependency direction is:

```text
mfd -> format-edi-config -> mapping -> ir
mfd -> format-edi        -> mapping -> ir
engine, codegen, codegen-runtime -> format-edi
```

Neither `format-edi` nor those runtime consumers depend on the compiler. The
compiler's test-only codec dependency exercises the embedded contract without
adding a runtime edge. Check the graph with `cargo tree -p format-edi -e normal,build`
and `cargo tree -p codegen-runtime -e normal,build`; the compiler must not appear.
This boundary makes no build-speed claim.

## Public API migration

Design-time callers must add `format-edi-config` to their Cargo dependencies and
replace the `format_edi::config` prefix with `format_edi_config`:

| Previous API | Current API |
| --- | --- |
| `format_edi::config::{import_config, import_config_confined, import_config_with_byte_reader}` | `format_edi_config::{import_config, import_config_confined, import_config_with_byte_reader}` |
| `format_edi::config::{CompiledConfig, ConfigError, EdiConfigByteReader}` | `format_edi_config::{CompiledConfig, ConfigError, EdiConfigByteReader}` |
| `format_edi::config::idoc::*` | `format_edi_config::idoc::*` |
| `format_edi::config::swift::*` | `format_edi_config::swift::*` |

There is no compiler compatibility re-export from the runtime package. Runtime
paths such as `format_edi::{x12, edifact, hl7, tradacoms, idoc, swift, document}`,
runtime error types, and serialized project/schema/layout representations are
unchanged.

## Resource policy

Filesystem compilation retains canonical-root confinement for transitive
includes and selected messages. The host-supplied `EdiConfigByteReader` retains
immutable byte identities, cached reads, host reference validation, and no
filesystem reopen or content-discovery fallback. Its 32-file/8 MiB budget and
the compiler's existing depth, expansion, and message-scan limits are unchanged.
IDoc and SWIFT keep their separate existing budgets and APIs; they do not gain
host-byte-reader support. Explicit bindings to those unsupported backends still
refuse rather than falling back to filesystem access.

MFD imports compile configurations before saving the embedded runtime model.
Removing a source configuration after compilation does not authorize or require
any runtime lookup. See [resource bindings](resource-bindings.md) for host trust
and digest policy.

## SWIFT MT output

SWIFT MT targets keep the compiled `swift_mt` layout and add
`swift_mt_output`, which the configuration does not supply:

```json
"swift_mt_output": {
  "direction": "input",
  "messages": [
    { "basic_header": "F01BANKBEBBAXXX0000000001", "application_header": "BANKDEFFXXXXN" }
  ]
}
```

- `basic_header` is the complete block 1 content. `application_header` is
  the block 2 content after the direction letter and message type: the
  receiver address and optional priority, monitoring and obsolescence for
  `input`, or the input time, message input reference, output date and time
  and optional priority for `output`. Both are checked against their FIN
  character positions when the project is validated.
- The list holds one header set per written message, in order, up to 10,000.
  A FIN stream never repeats a basic header or an output message input
  reference, so validation refuses repeats, and the mapped output must
  produce exactly as many messages as there are header sets; any other count
  is refused when the output is written.
- The headers are fixed project settings. Running a project again writes the
  same session and sequence numbers again; update them before producing a
  new stream. For the same reason a target whose mapping produces separately
  named documents cannot use `swift_mt_output`, because every document would
  repeat the same headers.
- `direction` selects the `Input` or `Output` application-header group for
  every message. A message may leave that group unmapped, or map both empty
  groups; the configured direction is written. A mapping that constructs
  only the other direction's group is refused when the project is
  validated.
- `edi_implied_decimals` must name decimal captures of the layout. Output
  moves the decimal point back without rounding and refuses a value whose
  scaled wire text would not re-read to exactly the mapped number.

Validation also applies the writer's layout checks to every message of the
layout, including message types a run never writes: each message type is `MT`
followed by three digits, every field tag is uppercase letters and digits,
every non-repeating field captures a value, and the schema's `Message` group
repeats. The reader reads only the first field with a given tag, so a field
the target mapping binds or constructs is refused when an earlier layout
field uses its tag. A layout that reuses a tag, such as `:86:` at line and
statement level, stays valid while the mapping fills only the first one.
Fields filled through copies or computed names are checked when the output
is written.

The writer's own limits still apply: user-header and trailer blocks are not
written, occurrences of different repeating fields cannot be interleaved, and
values outside a field's grammar or character set are refused. Nothing is
written unless the whole output re-reads to the expected instance. Primary
filename document runs refuse SWIFT MT, and `.mfd` export refuses a boundary
with output headers. Generated mappings exchange JSON instances, so their hosts
write SWIFT MT themselves.
