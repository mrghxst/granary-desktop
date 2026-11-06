# Contract fixtures

- `openapi-subset.json`: the operations Granary uses and every schema they reference, copied from
  `silo-server/contracts/api/v2/openapi.json@6791aac` (AGPL-3.0-or-later) by `node scripts/extract-oas-subset.mjs`.
  Regenerate after re-pinning the upstream SHA; keep the operation list in sync with docs/research/silo-api.md.
- `*.json`: sanitized response bodies. Values are synthetic (built from the OAS `examples`), never recorded from a real
  server with real tokens. `tests/contract.rs` validates every fixture against the subset schema
  (`required`, `additionalProperties: false`, types, enums), then serves it from a local mock server to the real
  HTTP client.
