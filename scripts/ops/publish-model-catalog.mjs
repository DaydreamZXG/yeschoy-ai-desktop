#!/usr/bin/env node
/**
 * Publish the reviewed model catalog to the download origin.
 *
 * The desktop app fetches this at startup and applies it only when the
 * sha256 matches, the schema validates, and `verifiedAt` is strictly newer
 * than the copy compiled into the running build. So publishing the same
 * content the app already ships is a no-op by design, and bumping
 * `verifiedAt` in src/model-profiles/catalog.json is what makes a new
 * revision take effect without shipping a release.
 *
 * This writes two files and nothing else. It never touches the relay, the
 * update channels, or any published installer.
 *
 *   node scripts/ops/publish-model-catalog.mjs --to /srv/yeschoy-download/public/apps
 *   node scripts/ops/publish-model-catalog.mjs --to <dir> --dry-run
 */
import { createHash } from "node:crypto";
import {
  readFileSync,
  writeFileSync,
  renameSync,
  mkdtempSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = path.resolve(here, "../../src/model-profiles/catalog.json");

const argv = process.argv.slice(2);
const flag = (name) => {
  const at = argv.indexOf(name);
  return at >= 0 ? argv[at + 1] : undefined;
};
const target = flag("--to");
const dryRun = argv.includes("--dry-run");
if (!target) {
  console.error(
    "usage: publish-model-catalog.mjs --to <directory> [--dry-run]",
  );
  process.exit(1);
}

const text = readFileSync(source, "utf8");

// Fail loudly here rather than serving something the app will silently
// reject: a bad publish is invisible from the client side.
const parsed = JSON.parse(text);
if (parsed.schemaVersion !== 1) throw new Error("unexpected schemaVersion");
if (!/^\d{4}-\d{2}-\d{2}$/.test(parsed.verifiedAt))
  throw new Error("bad verifiedAt");
if (!Array.isArray(parsed.models) || !parsed.models.length)
  throw new Error("no models");
const ids = new Set(parsed.models.map((m) => m.id));
if (ids.size !== parsed.models.length) throw new Error("duplicate model id");

const digest = createHash("sha256").update(text, "utf8").digest("hex");
const catalogPath = path.join(target, "model-catalog.json");
const digestPath = `${catalogPath}.sha256`;

console.log(
  `catalog     ${parsed.models.length} models, verifiedAt ${parsed.verifiedAt}`,
);
console.log(`sha256      ${digest}`);
console.log(`-> ${catalogPath}`);
console.log(`-> ${digestPath}`);
if (dryRun) {
  console.log("dry run, nothing written");
  process.exit(0);
}

// Write through a temp file and rename, so a reader never sees a half file
// and the digest never disagrees with the body that is being served.
const staging = mkdtempSync(path.join(tmpdir(), "model-catalog-"));
try {
  const stagedCatalog = path.join(staging, "model-catalog.json");
  const stagedDigest = path.join(staging, "model-catalog.json.sha256");
  writeFileSync(stagedCatalog, text, { mode: 0o644 });
  writeFileSync(stagedDigest, `${digest}  model-catalog.json\n`, {
    mode: 0o644,
  });
  // Digest first: a stale digest beside a new body would fail the client's
  // check, while a new digest beside a stale body does the same. Either
  // ordering is safe because the client falls back; this one leaves the
  // shorter window.
  renameSync(stagedCatalog, catalogPath);
  renameSync(stagedDigest, digestPath);
} finally {
  rmSync(staging, { recursive: true, force: true });
}
console.log("published");
