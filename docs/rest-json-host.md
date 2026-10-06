# Explicit JSON REST host execution

`run-rest-json` is an opt-in native host operation for one HTTP response and one
strict JSON mapping result. Existing captured-response files and `run` keep
their behavior. A project's `external_source` describes captured values; it
does not authorize a live request, supply credentials, or define this request.

The route accepts a strict JSON source and target with no named inputs or
outputs and no document-list scopes. It validates the loaded project and
logical JSON identities before connecting. The response then enters the same
payload parser, interpreter and writer as an offline JSON payload. The loaded
project is used once; it is not reopened after the remote operation.

Create a host request description separate from the project:

```json
{"url":"https://api.example.invalid/data","method":"POST","body_file":"request-body.json","timeout_seconds":30}
```

`body_file` is relative to the description's directory, or an absolute
host-selected path. GET forbids it; POST requires a nonempty strict UTF-8 JSON
document. The original bytes are sent without JSON reserialization. The only
description keys are `url`, `method`, `body_file`, and `timeout_seconds`.

Header values belong in a separate host-protected file:

```json
[{"name":"Authorization","value":"Bearer HOST_SUPPLIED_TOKEN"}]
```

The host manages that file's permissions and lifecycle. Do not put credential
values in project files or command-line arguments.

```text
ferrule run-rest-json --project mapping.json --request request.json \
  --header-values protected-headers.json --allow-live-rest \
  --response-identity response.json --output-identity result.json
```

Without `--allow-live-rest`, the command refuses before reading request files
or connecting. HTTPS is the default; `--allow-insecure-http` is an explicit
additional grant for cleartext HTTP, useful for local endpoints. Logical paths
identify the payload format and returned artifact; no configured output file
is written. Stdout is untouched until the complete mapping result succeeds.
An OS stdout-write failure can still leave a partial stream; stdout is not an
atomic publication destination. `--diagnostics json` retains the ordinary
versioned stderr diagnostics. `--param NAME=VALUE` supplies existing typed host
parameters; parameter error messages retain their existing behavior.

The library exposes `RestJsonRequest`, `RestJsonHeader`, `RestExecutionPolicy`,
`fetch_rest_json`, `RestJsonMappingOptions`, and
`run_project_value_rest_json_payloads`. `RestExecutionPolicy::default()` is
`Deny`. Hosts supply their own URL, headers, raw body and bounded timeout.
Successful fetches own complete response bytes. Failed fetches or mappings
return no response/result value. Mapping options can supply the existing typed
parameters, trace sink and debug hook. A cooperative continuation callback is
checked before the request, after the response and before returning artifacts;
it cannot interrupt a synchronous in-flight HTTP call. The request timeout
still bounds that call.

Each request and response body is capped at 8 MiB. URL length is capped at 4096
UTF-8 bytes. Host headers are limited to 64 entries and a conservative 64 KiB
wire ledger; response headers are capped at 64 KiB. Timeout defaults to 30
seconds and must be 1–300 seconds. Protocol headers cannot be overridden,
duplicate names compare without ASCII case, header values forbid controls,
and endpoint user information and fragments are rejected. Header values must
be printable ASCII; non-ASCII bytes and controls reject.

Requests send `Accept: application/json` and `Accept-Encoding: identity`; POST
also sends `Content-Type: application/json`. A response must be 2xx and have
exactly one `application/json` or `application/*+json` media type, with absent
or UTF-8 charset. Other parameters, encodings and non-identity content
compression reject. Empty responses, invalid UTF-8, JSON5 and trailing JSON
data reject. The body limit applies independently of Content-Length, including
chunked responses. These byte caps are not an RSS or streaming guarantee:
complete JSON values and the mapped output are materialized.

A fresh HTTP agent performs one request with no redirect following, request
retry, environment proxy, or ambient credentials. A timeout or disconnect may
occur after a remote POST took effect. Response parsing, mapping, output errors
and later cancellation cannot roll back that effect. Retrying is a new explicit
host decision, not automatic behavior.

The deadline bounds the caller's wait. A timed-out OS DNS lookup can finish
later in a library worker; it does not perform the HTTP request itself.

The repository resolves ureq with only its rustls family of features, excluding
gzip, brotli, charset and cookies. That resolved feature set is part of this
transport contract: enabling these features elsewhere can change response
handling before the host API sees the headers. Builds must verify the resolved
features and the compressed-response refusal test. Request and response DTO
debug surfaces omit URLs, headers and body contents. Transport errors omit the
underlying HTTP-library cause so query strings, credentials and remote bodies
are absent from their diagnostics. A host-installed dependency Trace logger
can expose URL paths/queries and library-classified nonsensitive headers
separately; this API does not control that logger.
Native parser/interpreter/writer mapping errors retain their original causes
and may quote application data. They have no blanket data-redaction guarantee.

This first route does not infer imported HTTP POST or GraphQL execution,
perform authentication discovery, or implement SOAP, OpenAPI, XML HTTP
transport, streaming, named boundaries, filesystem publication or trace-file
publication. Generated Rust and C# mapping libraries remain pure payload
libraries. Their existing JSON entry points can map a captured response, but
they do not make a live request through this native host API.
