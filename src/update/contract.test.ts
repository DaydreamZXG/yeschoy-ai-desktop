import { describe, expect, it } from "vitest";
import {
  decodeUpdateProjection,
  idleUpdateProjection,
  updateBusy,
} from "./contract";

const valid = {
  schemaVersion: 1,
  requestId: "update-check-1",
  phase: "available",
  currentVersion: "0.4.14",
  availableVersion: "0.4.15",
  notes: "修复更新测试",
  downloadedBytes: 0,
  totalBytes: 0,
  reasonCode: "update_available",
};

describe("desktop update contract", () => {
  it("accepts the closed native projection", () => {
    expect(decodeUpdateProjection(valid, "update-check-1")).toEqual(valid);
  });

  it("rejects stale correlations, unknown phases and unbounded notes", () => {
    expect(decodeUpdateProjection(valid, "another-request")).toBeNull();
    expect(
      decodeUpdateProjection({ ...valid, phase: "installing_anything" }),
    ).toBeNull();
    expect(
      decodeUpdateProjection({ ...valid, notes: "x".repeat(601) }),
    ).toBeNull();
    expect(
      decodeUpdateProjection({ ...valid, reasonCode: "raw error /tmp/key" }),
    ).toBeNull();
  });

  it("keeps only updater work busy", () => {
    expect(idleUpdateProjection().phase).toBe("idle");
    expect(updateBusy("checking")).toBe(true);
    expect(updateBusy("downloading")).toBe(true);
    expect(updateBusy("restarting")).toBe(true);
    expect(updateBusy("unavailable")).toBe(false);
  });
});
