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

/**
 * Reason codes the activation pipeline can hand the renderer, tests excluded.
 *
 * Two things this scanner has to get right, because getting either wrong makes
 * the whole test pass while saying nothing:
 *
 * 1. `#[cfg(test)]` also appears as an attribute *inside* production code
 *    (`terminal_launch.rs:125`). Splitting on its first occurrence threw away
 *    600 lines of live code and every reason code raised in them. Only a real
 *    trailing `#[cfg(test)] mod tests` marks the end of production source.
 * 2. Codes rarely reach `AdapterFailure` through a literal constructor call.
 *    Four helpers wrap it — `failure` in two modules, plus `launch_error` and
 *    `io_error`, which carry the code in their *second* argument after a stage
 *    label. Matching only `LaunchError("…")` saw 10 of the 26 live codes.
 */
function pipelineReasonCodes(): string[] {
  const codes = new Set<string>();
  for (const file of sources(resolve(root, "src-tauri/src"), [".rs"])) {
    const text = readFileSync(file, "utf8");
    const testModule = text.search(/^#\[cfg\(test\)\]\s*\r?\nmod tests/m);
    const body = (testModule === -1 ? text : text.slice(0, testModule))
      // Calls wrap across lines; collapse so one regex can span them.
      .replace(/\s+/g, " ");
    for (const pattern of [
      /\bConfigurationFailed\(\s*"([a-z_]+)"/g,
      /\bLaunchError\(\s*"([a-z_]+)"/g,
      /\bfailure\(\s*"([a-z_]+)"/g,
      // `launch_error(stage, reason, code)` / `io_error(stage, reason, error)`
      /\b(?:launch_error|io_error)\(\s*"[a-z_]+"\s*,\s*"([a-z_]+)"/g,
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
