# Memory v2 protocol fixtures

These fixtures are executable protocol assets for RFC 0.10. They define the
wire/storage contract; they do not activate Memory v2 in a real Vault.

## Schemas and valid examples

`schemas/` contains strict JSON Schema Draft 2020-12 documents for bootstrap,
protocol revision, authority revision, and Claim revision records. Unknown
top-level and nested properties are rejected. The Claim schema models
`kind_data` as a one-member tagged union and keeps subject, assertor, recorder,
human decision, temporal semantics, epistemic state, risk, salience, consent,
evidence, and lineage separate.

`valid/` and `canonical/claim-payload.yaml` are non-private synthetic examples.
The SHA-256 stored in the Claim example matches its canonical semantic payload.

## Canonical hash vectors

`canonical/claim-payload.canonical.json` is the UTF-8 RFC 8785 input with no
trailing newline. Its source YAML's top-level `payload_sha256` is excluded from
the semantic payload. The NoteMD v2 normalization profile additionally:

- converts Unicode text to NFC;
- converts CRLF/CR to LF, strips per-line trailing whitespace, and removes
  outer blank lines from prose fields;
- writes UUIDs and enums in lowercase and timestamps in canonical UTC form;
- sorts protocol-declared set arrays by their normalized canonical item bytes;
- preserves sequence order for arrays not declared as sets;
- preserves required nulls and omits absent optional fields; and
- rejects duplicate YAML keys, anchors/aliases, custom tags, invalid UTF-8,
  unknown fields, and schema-invalid scalar coercions before hashing.

The equivalent patch reverses every exercised set and injects NFD, CRLF, outer
blank lines, and trailing whitespace. It MUST produce the same canonical bytes
and hash. The changed patch modifies a semantic field and MUST produce a
different hash.
