#!/usr/bin/env node
/**
 * Model capability catalog verification (batch 2 / M1, M5).
 *
 * 1. Schema: every entry must carry the four capability fields
 *    (contextWindow / input / maxOutputTokens / reasoningLevels), a unique id,
 *    a displayName, and at least one parseable http(s) source URL.
 * 2. Reconciliation: optionally compare catalog ids against the live
 *    `/api/pricing` model list. Models missing from the catalog are ALLOWED
 *    (we never guess capabilities, PRD 6.5) but must be listed.
 *
 * Usage:
 *   node scripts/qa/verify-model-catalog.mjs
 *   node scripts/qa/verify-model-catalog.mjs --pricing https://yeschoy.com/api/pricing
 *   node scripts/qa/verify-model-catalog.mjs --pricing-file fixtures/pricing.json
 *   node scripts/qa/verify-model-catalog.mjs --pricing ... --strict-gaps
 *
 * Exit codes: 0 = pass (gaps allowed), 1 = schema violation or fetch/parse
 * failure, 2 = gaps found under --strict-gaps.
 */
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const repoRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);
const catalogPath = path.join(repoRoot, "src/model-profiles/catalog.json");

const args = process.argv.slice(2);
const flag = (name) => {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : undefined;
};
const has = (name) => args.includes(name);
const pricingUrl = has("--pricing") ? flag("--pricing") : undefined;
const pricingFile = has("--pricing-file") ? flag("--pricing-file") : undefined;
const strictGaps = has("--strict-gaps");

const problems = [];
const allowedInputs = new Set(["text", "image"]);
const knownRoutingCompat = new Set([
  // Server-side routing aliases, intentionally not capability-declared.
  "codex-auto-review",
  // Pure image-generation endpoints, not conversational models.
  "gpt-image-1.5",
  "gpt-image-2",
]);

function isHttpUrl(value) {
  try {
    const url = new URL(value);
    return url.protocol === "http:" || url.protocol === "https:";
  } catch {
    return false;
  }
}

let catalog;
try {
  catalog = JSON.parse(readFileSync(catalogPath, "utf8"));
} catch (error) {
  console.error(`✗ catalog.json is not readable JSON: ${error.message}`);
  process.exit(1);
}

if (catalog.schemaVersion !== 1) {
  problems.push(`schemaVersion must be 1, found ${catalog.schemaVersion}`);
}
if (typeof catalog.verifiedAt !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(catalog.verifiedAt)) {
  problems.push(`verifiedAt must be an ISO date, found ${catalog.verifiedAt}`);
}
if (!Array.isArray(catalog.models) || catalog.models.length === 0) {
  problems.push("models must be a non-empty array");
}

const seen = new Set();
for (const [index, model] of (catalog.models ?? []).entries()) {
  const at = `models[${index}]`;
  if (!model || typeof model !== "object") {
    problems.push(`${at}: not an object`);
    continue;
  }
  if (typeof model.id !== "string" || !model.id) {
    problems.push(`${at}: id missing`);
    continue;
  }
  if (seen.has(model.id)) problems.push(`${at}: duplicate id ${model.id}`);
  seen.add(model.id);
  if (typeof model.displayName !== "string" || !model.displayName) {
    problems.push(`${at} (${model.id}): displayName missing`);
  }
  for (const [field, check] of [
    ["contextWindow", (v) => Number.isInteger(v) && v > 0],
    ["maxOutputTokens", (v) => Number.isInteger(v) && v > 0],
  ]) {
    if (!(field in model)) {
      problems.push(`${at} (${model.id}): ${field} missing`);
    } else if (!check(model[field])) {
      problems.push(`${at} (${model.id}): ${field} must be a positive integer`);
    }
  }
  if (!("input" in model)) {
    problems.push(`${at} (${model.id}): input missing`);
  } else if (
    !Array.isArray(model.input) ||
    model.input.length === 0 ||
    !model.input.every((entry) => allowedInputs.has(entry)) ||
    !model.input.includes("text")
  ) {
    problems.push(
      `${at} (${model.id}): input must be a non-empty subset of ["text","image"] containing "text"`,
    );
  }
  if (!("reasoningLevels" in model)) {
    problems.push(`${at} (${model.id}): reasoningLevels missing`);
  } else if (
    !Array.isArray(model.reasoningLevels) ||
    !model.reasoningLevels.every((level) => typeof level === "string" && level)
  ) {
    problems.push(
      `${at} (${model.id}): reasoningLevels must be an array of non-empty strings (may be empty)`,
    );
  }
  if ("defaultReasoning" in model && typeof model.defaultReasoning !== "string") {
    problems.push(`${at} (${model.id}): defaultReasoning must be a string`);
  }
  if ("reasoningMode" in model && typeof model.reasoningMode !== "string") {
    problems.push(`${at} (${model.id}): reasoningMode must be a string`);
  }
  if ("toolUse" in model && typeof model.toolUse !== "boolean") {
    problems.push(`${at} (${model.id}): toolUse must be a boolean`);
  }
  if (
    !Array.isArray(model.sources) ||
    model.sources.length === 0 ||
    !model.sources.every(isHttpUrl)
  ) {
    problems.push(
      `${at} (${model.id}): sources must be a non-empty array of http(s) URLs`,
    );
  }
}

console.log(
  `catalog: ${catalog.models?.length ?? 0} models, verifiedAt ${catalog.verifiedAt}`,
);

async function loadPricingIds() {
  let text;
  if (pricingFile) {
    text = readFileSync(path.resolve(pricingFile), "utf8");
  } else if (pricingUrl) {
    const response = await fetch(pricingUrl, {
      headers: { accept: "application/json" },
      signal: AbortSignal.timeout(15000),
    });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    text = await response.text();
  } else {
    return null;
  }
  const value = JSON.parse(text);
  const list = Array.isArray(value)
    ? value
    : Array.isArray(value?.data)
      ? value.data
      : Array.isArray(value?.models)
        ? value.models
        : null;
  if (!list) throw new Error("unrecognized pricing payload shape");
  return list
    .map((entry) =>
      typeof entry === "string"
        ? entry
        : entry?.model_name ?? entry?.model ?? entry?.id,
    )
    .filter((id) => typeof id === "string" && id);
}

if (pricingUrl || pricingFile) {
  let onlineIds;
  try {
    onlineIds = await loadPricingIds();
  } catch (error) {
    console.error(`✗ pricing reconciliation failed: ${error.message}`);
    process.exit(1);
  }
  const gaps = onlineIds.filter((id) => !seen.has(id));
  const expected = gaps.filter((id) => !knownRoutingCompat.has(id));
  const suppressed = gaps.filter((id) => knownRoutingCompat.has(id));
  console.log(
    `pricing: ${onlineIds.length} online models, ${onlineIds.length - gaps.length} covered by catalog`,
  );
  if (suppressed.length) {
    console.log(
      `intentionally absent (routing/image endpoints): ${suppressed.join(", ")}`,
    );
  }
  if (expected.length) {
    console.log(
      `online but not in catalog (capabilities unknown — allowed, listed per M1):`,
    );
    for (const id of expected) console.log(`  - ${id}`);
    if (strictGaps) {
      console.error("✗ strict-gaps: uncovered models present");
      process.exit(2);
    }
  } else {
    console.log("pricing reconciliation: no capability gaps");
  }
} else {
  console.log("pricing reconciliation: skipped (pass --pricing or --pricing-file)");
}

if (problems.length) {
  console.error(`✗ ${problems.length} schema violation(s):`);
  for (const problem of problems) console.error(`  - ${problem}`);
  process.exit(1);
}
console.log("schema: OK (four capability fields, ids unique, sources parseable)");
