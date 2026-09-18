import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const root = process.cwd();

function sources(directory: string, extensions: string[]): string[] {
  return readdirSync(directory).flatMap((entry) => {
    const full = join(directory, entry);
    if (statSync(full).isDirectory()) return sources(full, extensions);
    return extensions.some((extension) => full.endsWith(extension))
      ? [full]
      : [];
  });
}

/** Reason codes the activation pipeline can hand the renderer, tests excluded. */
function pipelineReasonCodes(): string[] {
  const codes = new Set<string>();
  for (const file of sources(resolve(root, "src-tauri/src"), [".rs"])) {
    const body = readFileSync(file, "utf8").split("#[cfg(test)]")[0];
    for (const pattern of [
      /ConfigurationFailed\("([a-z_]+)"\)/g,
      /LaunchError\("([a-z_]+)"\)/g,
    ]) {
      for (const match of body.matchAll(pattern)) codes.add(match[1]);
    }
  }
  return [...codes].sort();
}

describe("activation failure copy", () => {
  it("gives every reason code the pipeline can emit a message of its own", () => {
    const rendered = sources(resolve(root, "src"), [".ts", ".tsx"])
      .filter((file) => !file.includes(".test."))
      .map((file) => readFileSync(file, "utf8"))
      .join("\n");
    const codes = pipelineReasonCodes();

    expect(codes.length).toBeGreaterThan(0);
    // A code with no branch of its own falls back to the generic "设置未能完整
    // 更新", which names neither a cause nor a next step. The four
    // configuration_* codes are the common Windows failures — antivirus holding
    // the file, no permission, a full disk, a hand-broken config — so a
    // beginner who hits one is left with nothing to act on. That silent
    // fallback, not any wording preference, is what this test prevents.
    expect(codes.filter((code) => !rendered.includes(code))).toEqual([]);
  });
});
