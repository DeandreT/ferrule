# EDI configuration compilation

`format-edi-config` is the design-time compiler for supported external EDI
configurations. `format-edi` contains the runtime codecs and validation for
X12, EDIFACT, HL7 v2, TRADACOMS, IDoc, and input-only SWIFT MT. Dialects stay
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
