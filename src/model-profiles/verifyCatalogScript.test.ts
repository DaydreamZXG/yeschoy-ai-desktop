import { describe, expect, it } from "vitest";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

const script = path.resolve(
  __dirname,
  "../../scripts/qa/verify-model-catalog.mjs",
);

function verify(models: unknown[]) {
  const dir = mkdtempSync(path.join(tmpdir(), "verify-catalog-"));
  const file = path.join(dir, "catalog.json");
  writeFileSync(
    file,
    JSON.stringify({ schemaVersion: 1, verifiedAt: "2026-10-20", models }),
  );
  return spawnSync(process.execPath, [script, "--catalog", file], {
    encoding: "utf8",
  });
}

const entry = (id: string) => ({
  id,
  displayName: id,
  contextWindow: 500000,
  input: ["text"],
  reasoningLevels: [],
  sources: ["https://example.com"],
});

describe("verify-model-catalog maxOutputTokens", () => {
  it("allows the documented unlimited-output models to omit it", () => {
    const result = verify([entry("grok-4.6"), entry("grok-4.7")]);
    expect(result.status).toBe(0);
  });

  it("still fails any other model missing it", () => {
    const result = verify([entry("grok-4.6"), entry("some-other-model")]);
    expect(result.status).toBe(1);
    expect(result.stdout + result.stderr).toContain(
      "some-other-model): maxOutputTokens missing",
    );
    expect(result.stdout + result.stderr).not.toContain(
      "grok-4.6): maxOutputTokens",
    );
  });
});
