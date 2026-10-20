import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { ACTIVATION_TOOL_IDS } from "./activation";
import { TERMINAL_TOOLS } from "./launchApi";
import {
  connectionLifecycleMode,
  usesLocalBridge,
  writtenEndpoint,
} from "./ConfigurationPreviewView";
import zh from "../i18n/locales/zh.json";
import en from "../i18n/locales/en.json";

const bridge = readFileSync(
  resolve(__dirname, "../../src-tauri/src/opencode_bridge.rs"),
  "utf8",
);

describe("OpenCode one-click connection", () => {
  it("is a real activation target opened in a terminal", () => {
    expect(ACTIVATION_TOOL_IDS).toContain("opencode");
    expect(TERMINAL_TOOLS).toContain("opencode");
    expect(connectionLifecycleMode("opencode")).toBe("new_terminal_session");
  });

  it("goes through the local bridge whose address matches Rust", () => {
    expect(usesLocalBridge("opencode")).toBe(true);
    const base = /PROXY_BASE: &str = "([^"]+)"/.exec(bridge)?.[1];
    expect(writtenEndpoint("opencode", "https://example.test")).toBe(base);
  });
});

describe("Codex desktop + CLI share one connection", () => {
  it("explains the shared settings file in both languages", () => {
    expect(zh.configuration.codexSharedSettings).toContain("同一份设置");
    expect(en.configuration.codexSharedSettings).toContain("share one");
    expect(zh.configuration.opencodeKeyNote).toContain("钥匙串");
  });
});
