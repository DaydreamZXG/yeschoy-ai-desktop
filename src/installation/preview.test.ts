import { describe, expect, it, vi } from "vitest";
import {
  decodeInstallationPreview,
  installationPreview,
  type InstallationPreview,
} from "./api";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const valid = (id: string): InstallationPreview => ({
  schemaVersion: 1,
  requestId: id,
  toolId: "codex_desktop",
  platform: "macos",
  architecture: "arm64",
  mode: "automatic",
  packageAvailable: true,
  mirrorAvailable: false,
  guideAvailable: true,
});

describe("installation preview", () => {
  it("decodes a well-formed preview", () => {
    expect(decodeInstallationPreview(valid("r1"), "r1")).toEqual(valid("r1"));
  });

  it("rejects a response for another request", () => {
    expect(decodeInstallationPreview(valid("r1"), "r2")).toBeNull();
  });

  it("rejects an extra field: the two sides must agree on the payload", () => {
    expect(
      decodeInstallationPreview({ ...valid("r1"), source: "official" }, "r1"),
    ).toBeNull();
  });

  it("rejects a missing field", () => {
    const { mirrorAvailable: _omitted, ...rest } = valid("r1");
    expect(decodeInstallationPreview(rest, "r1")).toBeNull();
  });

  it("rejects values outside the closed vocabulary", () => {
    for (const patch of [
      { schemaVersion: 2 },
      { toolId: "not_a_tool" },
      { mode: "manual" },
      { platform: "haiku" },
      { architecture: "ia32" },
      { packageAvailable: "yes" },
    ])
      expect(
        decodeInstallationPreview({ ...valid("r1"), ...patch }, "r1"),
      ).toBeNull();
  });

  it("rejects a preview that offers neither a package nor a guide", () => {
    expect(
      decodeInstallationPreview(
        { ...valid("r1"), packageAvailable: false, guideAvailable: false },
        "r1",
      ),
    ).toBeNull();
  });

  it("rejects non-objects", () => {
    for (const value of [null, [], "preview", 7])
      expect(decodeInstallationPreview(value, "r1")).toBeNull();
  });

  it("calls only the read-only command and validates what comes back", async () => {
    const mocked = vi.mocked(invoke);
    mocked.mockImplementation(async (_command, args) => {
      const request = (args as { request: { requestId: string } }).request;
      return valid(request.requestId);
    });
    const preview = await installationPreview("codex_desktop");
    expect(preview.mode).toBe("automatic");
    expect(mocked).toHaveBeenCalledTimes(1);
    expect(mocked.mock.calls[0][0]).toBe("installation_preview_read_only");
    expect(mocked.mock.calls[0][1]).toMatchObject({
      request: { toolId: "codex_desktop" },
    });
  });

  it("fails loudly instead of half-reading a bad payload", async () => {
    vi.mocked(invoke).mockResolvedValue({ schemaVersion: 1 });
    await expect(installationPreview("codex_desktop")).rejects.toThrow(
      "invalid_installation_preview",
    );
  });
});
