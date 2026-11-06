// Extract the operations Granary uses (and every schema they reference) from the upstream Silo OpenAPI document
// into crates/silo-api/tests/fixtures/openapi-subset.json, so contract tests run without the gitignored .ref/ clone.
//
// Usage (repo root): node scripts/extract-oas-subset.mjs [path/to/openapi.json] [upstream-sha]
// Defaults: .ref/silo-server/contracts/api/v2/openapi.json and the SHA pinned in docs/research/silo-api.md.
// The upstream document is data: this script only copies JSON, it never evaluates it.
import { readFileSync, writeFileSync } from "node:fs";

const src = process.argv[2] ?? ".ref/silo-server/contracts/api/v2/openapi.json";
const sha = process.argv[3] ?? "6791aac867664c41c55ee82909fe063d145911a6";
const out = "crates/silo-api/tests/fixtures/openapi-subset.json";

// Keep in sync with the endpoint table in docs/research/silo-api.md (every op here must be listed there).
const OPERATIONS = [
  "getSystemInfo",
  "getServerIdentity",
  "login",
  "refreshSession",
  "logout",
  "getCurrentUser",
  "listProfiles",
  "verifyProfilePIN",
  "listUserLibraries",
];

const oas = JSON.parse(readFileSync(src, "utf8"));
const schemas = {};
const visit = (node) => {
  if (Array.isArray(node)) return node.forEach(visit);
  if (node === null || typeof node !== "object") return;
  if (typeof node.$ref === "string" && node.$ref.startsWith("#/components/schemas/")) {
    const name = node.$ref.slice("#/components/schemas/".length);
    if (!(name in schemas)) {
      schemas[name] = oas.components.schemas[name];
      visit(schemas[name]);
    }
  }
  Object.values(node).forEach(visit);
};

const operations = {};
for (const [path, item] of Object.entries(oas.paths)) {
  for (const [method, op] of Object.entries(item)) {
    if (!op || !OPERATIONS.includes(op.operationId)) continue;
    const body = op.requestBody?.content ? Object.values(op.requestBody.content)[0].schema : null;
    const responses = {};
    for (const [code, r] of Object.entries(op.responses ?? {})) {
      const c = r.content ? Object.entries(r.content)[0] : null;
      responses[code] = c ? { contentType: c[0], schema: c[1].schema } : null;
    }
    const params = (op.parameters ?? []).map((p) => ({ in: p.in, name: p.name, required: !!p.required }));
    operations[op.operationId] = {
      method: method.toUpperCase(),
      path,
      security: op.security ?? null,
      params,
      requestBody: body,
      responses,
    };
    visit(body);
    visit(responses);
  }
}
const missing = OPERATIONS.filter((o) => !(o in operations));
if (missing.length) {
  console.error(`missing operations: ${missing.join(", ")}`);
  process.exit(1);
}
const sorted = Object.fromEntries(Object.keys(schemas).sort().map((k) => [k, schemas[k]]));
const doc = {
  source: `silo-server contracts/api/v2/openapi.json@${sha} (AGPL-3.0-or-later; subset extracted by scripts/extract-oas-subset.mjs)`,
  operations,
  schemas: sorted,
};
writeFileSync(out, JSON.stringify(doc, null, 2) + "\n");
console.log(`wrote ${out}: ${Object.keys(operations).length} operations, ${Object.keys(sorted).length} schemas`);
