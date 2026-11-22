import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { TFunction } from "i18next";
import { hostOf } from "./station";

/** 内置的一家云厂商。表在原生侧（provider_presets.rs），这里只是照着画。 */
export interface ProviderPreset {
  id: string;
  /** 同一家的国内站和海外站是同一个 family。 */
  family: string;
  region: "cn" | "global" | null;
  name: string;
  origin: string;
  /** plan：包月套餐，厂商多半只许在编程工具里用。 */
  billing: "plan" | "pay_as_you_go";
  /** 连接时核对不了密钥对不对（它的模型名单不用密钥也能看）。 */
  unverifiable: boolean;
  /** 只有这些应用能接；空的就是都能接。 */
  onlyTools: string[];
  keyPage: string;
  common: boolean;
  /** 别的应用里要填的两个地址。没有 Claude 格式的就是 null。 */
  openaiBase: string;
  anthropicBase: string | null;
  /** 这家要求每个请求带的请求头；别的应用得自己加上。多数没有。 */
  sessionHeader: string | null;
  /** Claude 格式的地址只认 `Authorization: Bearer`，按 `x-api-key` 带密钥会被拒。 */
  anthropicBearer: boolean;
}

const path = (value: unknown): string | null =>
  typeof value === "string" && /^(\/[\w.-]+)*$/.test(value) ? value : null;

const text = (value: unknown): string | null =>
  typeof value === "string" && value.length > 0 ? value : null;

export function decodeProviderPresets(raw: unknown): ProviderPreset[] {
  if (!Array.isArray(raw)) return [];
  const presets: ProviderPreset[] = [];
  for (const item of raw) {
    if (!item || typeof item !== "object") continue;
    const row = item as Record<string, unknown>;
    const id = text(row.id);
    const family = text(row.family);
    const name = text(row.name);
    const origin = text(row.origin);
    const keyPage = text(row.keyPage);
    if (!id || !family || !name || !origin || !keyPage) continue;
    if (!/^https:\/\/[^/]+$/.test(origin) || !keyPage.startsWith("https://"))
      continue;
    const openaiPath = path(row.openaiPath);
    if (openaiPath === null) continue;
    const anthropicPath = path(row.anthropicPath);
    presets.push({
      id,
      family,
      name,
      origin,
      keyPage,
      region:
        row.region === "cn" || row.region === "global" ? row.region : null,
      billing: row.billing === "plan" ? "plan" : "pay_as_you_go",
      unverifiable: row.keyCheck === "unverifiable",
      onlyTools: Array.isArray(row.onlyTools)
        ? row.onlyTools.filter(
            (tool): tool is string => typeof tool === "string",
          )
        : [],
      common: row.common === true,
      openaiBase: origin + openaiPath,
      anthropicBase: anthropicPath === null ? null : origin + anthropicPath,
      sessionHeader:
        typeof row.sessionHeader === "string" &&
        /^[a-z][a-z0-9-]*$/.test(row.sessionHeader)
          ? row.sessionHeader
          : null,
      anthropicBearer: anthropicPath !== null && row.anthropicBearer === true,
    });
  }
  return presets;
}

let loading: Promise<ProviderPreset[]> | null = null;

/** 读不到就当没有：连接屏只是少了「云厂商」那一组，填网址那条路照旧。 */
export function loadProviderPresets(): Promise<ProviderPreset[]> {
  loading ??= invoke<unknown>("provider_presets_v1")
    .then(decodeProviderPresets)
    .catch(() => {
      loading = null;
      return [];
    });
  return loading;
}

export function resetProviderPresetsForTest() {
  loading = null;
}

export function useProviderPresets(): ProviderPreset[] {
  const [presets, setPresets] = useState<ProviderPreset[]>([]);
  useEffect(() => {
    let live = true;
    void loadProviderPresets().then((list) => {
      if (live) setPresets(list);
    });
    return () => {
      live = false;
    };
  }, []);
  return presets;
}

export interface VendorFamily {
  family: string;
  billing: ProviderPreset["billing"];
  common: boolean;
  /** 国内在前。只有一个地区的就一项。 */
  regions: ProviderPreset[];
}

export function vendorFamilies(presets: ProviderPreset[]): VendorFamily[] {
  const families: VendorFamily[] = [];
  for (const preset of presets) {
    const known = families.find((item) => item.family === preset.family);
    if (known) {
      known.regions.push(preset);
      known.common ||= preset.common;
    } else
      families.push({
        family: preset.family,
        billing: preset.billing,
        common: preset.common,
        regions: [preset],
      });
  }
  return families;
}

export function presetOfOrigin(
  origin: string,
  presets: ProviderPreset[],
): ProviderPreset | null {
  return presets.find((preset) => preset.origin === origin) ?? null;
}

/** 这条线路连的是不是一家内置的云厂商。 */
export function presetOfLine(
  lineId: string,
  presets: ProviderPreset[],
): ProviderPreset | null {
  const at = lineId.indexOf(":");
  return at < 0 ? null : presetOfOrigin(lineId.slice(at + 1), presets);
}

export function vendorName(preset: ProviderPreset, t: TFunction): string {
  return t(`kaifan.vendor.${preset.family}`, { defaultValue: preset.name });
}

/** 界面上怎么称呼现在连着的站点：云厂商叫名字，其余叫域名。 */
export function stationLabel(
  lineId: string,
  presets: ProviderPreset[],
  t: TFunction,
): string | null {
  const preset = presetOfLine(lineId, presets);
  if (preset) return vendorName(preset, t);
  const at = lineId.indexOf(":");
  if (at < 0) return null;
  const origin = lineId.slice(at + 1);
  return /^https:\/\//.test(origin) ? hostOf(origin) : null;
}

/**
 * 别的应用里「API 地址」那一栏该填什么。云厂商按表；其余站点按惯例：
 * OpenAI 格式是 `站点/v1`，Claude 格式就是站点本身。
 */
export function addressesOf(
  origin: string | null,
  preset: ProviderPreset | null,
): { openai: string; anthropic: string | null } | null {
  if (preset)
    return { openai: preset.openaiBase, anthropic: preset.anthropicBase };
  return origin ? { openai: `${origin}/v1`, anthropic: origin } : null;
}
