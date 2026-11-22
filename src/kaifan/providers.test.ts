import { describe, expect, it } from "vitest";
import i18n from "../i18n";
import {
  addressesOf,
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

  it("别的应用要填的地址：云厂商按表，别的站按惯例", () => {
    const [kimi] = decodeProviderPresets([row()]);
    expect(addressesOf("https://api.kimi.com", kimi)).toEqual({
      openai: "https://api.kimi.com/coding/v1",
      anthropic: "https://api.kimi.com/coding",
    });
    const [openai] = decodeProviderPresets([
      row({
        origin: "https://api.openai.com",
        openaiPath: "/v1",
        anthropicPath: null,
      }),
    ]);
    expect(addressesOf("https://api.openai.com", openai)?.anthropic).toBeNull();
    expect(openai.sessionHeader).toBeNull();
    // 要求带请求头的那家：只收像请求头名字的，别的当没有。
    const header = (sessionHeader: unknown) =>
      decodeProviderPresets([row({ sessionHeader })])[0].sessionHeader;
    expect(header("x-opencode-session")).toBe("x-opencode-session");
    expect(header("x bad\r\nheader")).toBeNull();
    expect(header(7)).toBeNull();
    // Claude 格式只认 Bearer 的那家：照后端说的来，没说就是不用。
    expect(openai.anthropicBearer).toBe(false);
    expect(
      decodeProviderPresets([row({ anthropicBearer: true })])[0]
        .anthropicBearer,
    ).toBe(true);
    expect(addressesOf("https://my.example.com", null)).toEqual({
      openai: "https://my.example.com/v1",
      anthropic: "https://my.example.com",
    });
    expect(addressesOf(null, null)).toBeNull();
  });

  it("路径不像路径的行丢掉：不把来路不明的东西拼进地址给用户复制", () => {
    expect(
      decodeProviderPresets([
        row({ openaiPath: "/v1?x=1" }),
        row({ openaiPath: "//evil.example" }),
        row({ openaiPath: undefined }),
      ]),
    ).toEqual([]);
  });
});
