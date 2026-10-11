import { describe, expect, it, beforeEach } from "vitest";
import {
  catalogLineFromSnapshot,
  initialAccountLineId,
  readAccountLinePreference,
  writeAccountLinePreference,
} from "./accountLinePreference";

beforeEach(() => {
  localStorage.clear();
});

describe("accountLinePreference", () => {
  it("restores the saved relay line on startup", () => {
    writeAccountLinePreference("global_accelerated");
    expect(initialAccountLineId()).toBe("global_accelerated");
  });

  it("maps stored session line ids to catalog relay lines", () => {
    expect(catalogLineFromSnapshot("global_accelerated")).toBe(
      "global_accelerated",
    );
    expect(
      catalogLineFromSnapshot("station_connect:https://api.yeschoy.io"),
    ).toBe("global_accelerated");
    expect(readAccountLinePreference()).toBeNull();
  });
});
