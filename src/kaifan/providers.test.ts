import { describe, expect, it } from "vitest";
import i18n from "../i18n";
import {
  decodeProviderPresets,
  presetOfLine,
  stationLabel,
  vendorFamilies,
} from "./providers";

const row = (patch: Record<string, unknown> = {}) => ({
  id: "kimi_code_cn",
  family: "kimi_code",
  region: "cn",
  name: "Kimi Code",
  origin: "https://api.kimi.com",
  billing: "plan",
  keyCheck: "models",
  onlyTools: [],
  keyPage: "https://www.kimi.com/code/console",
  common: true,
  openaiPath: "/coding/v1",
  anthropicPath: "/coding",
  ...patch,
});

describe("云厂商预设", () => {
  it("读不懂的行丢掉，不让它出现在界面上", () => {
    const presets = decodeProviderPresets([
      row(),
      row({ id: "bad_origin", origin: "http://api.kimi.com" }),
      row({ id: "with_path", origin: "https://api.kimi.com/coding" }),
      row({ id: "bad_page", keyPage: "javascript:alert(1)" }),
      row({ id: "" }),
      null,
      "nope",
    ]);
    expect(presets.map((preset) => preset.id)).toEqual(["kimi_code_cn"]);
    expect(decodeProviderPresets({})).toEqual([]);
  });

  it("没法提前核对密钥、只认某几个应用，都照表里说的", () => {
    const [bailian, go, openrouter] = decodeProviderPresets([
      row({ id: "a", keyCheck: "unverifiable" }),
      row({ id: "b", onlyTools: ["opencode", 3] }),
      row({ id: "c", billing: "pay_as_you_go", keyCheck: { path: "/x" } }),
    ]);
    expect(bailian.unverifiable).toBe(true);
    expect(go.onlyTools).toEqual(["opencode"]);
    expect(openrouter).toMatchObject({
      billing: "pay_as_you_go",
      unverifiable: false,
    });
  });

  it("同一家的国内站和海外站合成一个，国内在前", () => {
    const families = vendorFamilies(
      decodeProviderPresets([
        row(),
        row({
          id: "kimi_code_global",
          region: "global",
          origin: "https://api.kimi.ai",
        }),
        row({ id: "openai", family: "openai", region: null, common: false }),
      ]),
    );
    expect(families.map((item) => item.family)).toEqual([
      "kimi_code",
      "openai",
    ]);
    expect(families[0].regions.map((item) => item.region)).toEqual([
      "cn",
      "global",
    ]);
    expect(families[1].common).toBe(false);
  });

  it("云厂商叫名字，别的站叫域名", () => {
    const presets = decodeProviderPresets([row()]);
    const t = i18n.getFixedT("zh");
    expect(stationLabel("station_key:https://api.kimi.com", presets, t)).toBe(
      "Kimi Code",
    );
    expect(stationLabel("station_key:https://my.example.com", presets, t)).toBe(
      "my.example.com",
    );
    expect(stationLabel("mainland_optimized", presets, t)).toBeNull();
    expect(presetOfLine("station_key:https://my.example.com", presets)).toBe(
      null,
    );
  });
});
