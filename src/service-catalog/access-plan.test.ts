import { describe, expect, it } from "vitest";
import { CONFIGURATION_LINES } from "../configuration/preview";
import type { ConfigurationToolId } from "../configuration/preview";
import { createToolAccessPlan } from "./access-plan";
import { catalogFixture } from "./test-fixtures";

describe("tool access preview", () => {
  it("derives the documented protocol profiles on each fixed line", () => {
    for (const line of CONFIGURATION_LINES)
      for (const toolId of [
        "claude",
        "codex",
        "opencode",
        "pi",
        "dsh",
        "hermes",
        "openclaw",
      ] as ConfigurationToolId[]) {
        const plan = createToolAccessPlan(
          catalogFixture("test-read", line.id),
          toolId,
          line.id,
          "test-group",
          "test/model",
        );
        expect(plan.baseUrl).toBe(
          line.rootUrl + (toolId === "claude" ? "" : "/v1"),
        );
        expect(plan.requiredProtocol).toBe(
          toolId === "claude"
            ? "anthropic"
            : toolId === "codex"
              ? "openai-response"
              : "openai",
        );
        expect(plan.status).toBe("protocol_declared");
        expect(plan.applyAllowed).toBe(false);
        expect(plan.accountAccess).toBe("unverified");
        expect(plan.groupRestrictions).toBe("unverified");
        expect(plan.exactToolVersion).toBe("unverified");
      }
  });
  it("does not equate Chat and Responses or infer a protocol from model names", () => {
    const sample = catalogFixture();
    sample.models[0].endpoints = ["openai"];
    expect(
      createToolAccessPlan(
        sample,
        "codex",
        sample.lineId,
        "test-group",
        "test/model",
      ).status,
    ).toBe("protocol_not_declared");
    expect(
      createToolAccessPlan(
        sample,
        "claude",
        sample.lineId,
        "test-group",
        "test/model",
      ).status,
    ).toBe("protocol_not_declared");
    sample.models[0].endpoints = [];
    expect(
      createToolAccessPlan(
        sample,
        "pi",
        sample.lineId,
        "test-group",
        "test/model",
      ).status,
    ).toBe("protocol_not_declared");
  });
  it("declares the DSH endpoint and refuses invalid model-group selections", () => {
    const sample = catalogFixture();
    const dsh = createToolAccessPlan(
      sample,
      "dsh",
      sample.lineId,
      "test-group",
      "test/model",
    );
    expect(dsh.status).toBe("protocol_declared");
    expect(dsh.baseUrl).toBe("https://yeschoy.com/v1");
    expect(dsh.requiredProtocol).toBe("openai");
    expect(() =>
      createToolAccessPlan(
        sample,
        "pi",
        sample.lineId,
        "missing",
        "test/model",
      ),
    ).toThrow("invalid_selection");
    expect(() =>
      createToolAccessPlan(
        sample,
        "pi",
        sample.lineId,
        "test-group",
        "invented-model",
      ),
    ).toThrow("invalid_selection");
    expect(() =>
      createToolAccessPlan(
        sample,
        "pi",
        "global_accelerated",
        "test-group",
        "test/model",
      ),
    ).toThrow("invalid_catalog");
  });
  it("does not turn recognized backend capabilities into authorization or price", () => {
    const sample = catalogFixture();
    sample.desktopBackend = {
      status: "contract_recognized",
      error: "none",
      declaredCapabilities: ["device_authorization", "tool_keys_manage"],
    };
    const plan = createToolAccessPlan(
      sample,
      "pi",
      sample.lineId,
      "test-group",
      "test/model",
    );
    expect(plan.applyAllowed).toBe(false);
    expect(plan.accountAccess).toBe("unverified");
    expect(plan).not.toHaveProperty("price");
    expect(plan).not.toHaveProperty("key");
  });
});
