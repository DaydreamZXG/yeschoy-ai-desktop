/**
 * 首页的九个角色：一个角色代表一个模型牌子。
 *
 * 这里只放**不需要翻译**的东西：id、代表色、立绘里脸的位置、怎么从模型 id
 * 认出牌子。名字和台词在 i18n 的 `roster.*` 里。
 *
 * 站点目录里认不出牌子的模型归「转校生」（`new`）：照常能用，只是还没有立绘。
 */
import type { AccountModel } from "../account/session";

export const ROSTER_IDS = [
  "cla",
  "gpt",
  "gem",
  "grk",
  "ds",
  "qw",
  "kimi",
  "glm",
  "dou",
] as const;
export type RosterId = (typeof ROSTER_IDS)[number] | "new";
export type Mood = "idle" | "happy" | "hungry" | "panic" | "sleep";

export interface RosterCharacter {
  id: RosterId;
  /** 牌子名是专有名词，不翻译。 */
  brand: string;
  color: string;
  /** 脸在立绘里的位置（0–1）。窄条靠它对准脸。 */
  face: [number, number];
}

export const ROSTER: Record<RosterId, RosterCharacter> = {
  cla: { id: "cla", brand: "Claude", color: "#ff9a4d", face: [0.56, 0.15] },
  gpt: { id: "gpt", brand: "GPT", color: "#36d68a", face: [0.55, 0.14] },
  gem: { id: "gem", brand: "Gemini", color: "#b58cff", face: [0.56, 0.15] },
  grk: { id: "grk", brand: "Grok", color: "#e6e9f0", face: [0.55, 0.16] },
  ds: { id: "ds", brand: "DeepSeek", color: "#4fb8ff", face: [0.56, 0.2] },
  qw: { id: "qw", brand: "Qwen", color: "#9b6bff", face: [0.57, 0.2] },
  kimi: { id: "kimi", brand: "Kimi", color: "#5aa2ff", face: [0.58, 0.17] },
  glm: { id: "glm", brand: "GLM", color: "#3b6bff", face: [0.42, 0.19] },
  dou: { id: "dou", brand: "Doubao", color: "#ff9ec4", face: [0.5, 0.17] },
  new: { id: "new", brand: "", color: "#aab4d4", face: [0.5, 0.2] },
};

// 顺序有意义：先认更具体的牌子。`o1`/`o3` 这类名字只在整段开头才算 GPT，
// 免得把别家带 `o3` 字样的模型认错。
const BRAND_RULES: [Exclude<RosterId, "new">, RegExp][] = [
  ["cla", /claude/i],
  ["gem", /gemini/i],
  ["grk", /grok/i],
  ["ds", /deepseek/i],
  ["qw", /qwen|qwq|tongyi/i],
  ["kimi", /kimi|moonshot/i],
  ["glm", /glm|zhipu/i],
  ["dou", /doubao/i],
  [
    "gpt",
    /gpt|codex|dall-e|whisper|text-embedding|^(o[134])(-|$)|(^|\/)o[134](-|$)/i,
  ],
];

export function characterOfModel(modelId: string): RosterId {
  return BRAND_RULES.find(([, rule]) => rule.test(modelId))?.[0] ?? "new";
}

/** 每个角色在这个站上有哪些模型。目录里没有的角色不出现在结果里。 */
export function groupModels(
  models: Pick<AccountModel, "id">[],
): Partial<Record<RosterId, string[]>> {
  const out: Partial<Record<RosterId, string[]>> = {};
  for (const { id } of models) (out[characterOfModel(id)] ??= []).push(id);
  return out;
}

const PICK_KEY = "kaifan.character";

export function readPickedCharacter(): RosterId | null {
  try {
    const value = localStorage.getItem(PICK_KEY);
    return value && value in ROSTER ? (value as RosterId) : null;
  } catch {
    return null;
  }
}

export function storePickedCharacter(id: RosterId) {
  try {
    localStorage.setItem(PICK_KEY, id);
  } catch {
    /* 存不了就不记，不影响使用。 */
  }
}

/**
 * 首页现在该由谁出场。
 *
 * 1. 用户自己点过的人，只要这个站还有她；
 * 2. 已接入的应用正在用的模型对应的人；
 * 3. 这个站有的第一个人；
 * 4. 目录还不知道（没登录、没读到）时给 `fallback`。
 */
export function chooseCharacter(input: {
  picked: RosterId | null;
  available: Partial<Record<RosterId, string[]>> | null;
  connectedModelIds: string[];
  fallback: RosterId;
}): RosterId {
  const { picked, available, connectedModelIds, fallback } = input;
  if (!available) return picked ?? fallback;
  if (picked && available[picked]) return picked;
  for (const modelId of connectedModelIds) {
    const id = characterOfModel(modelId);
    if (available[id]) return id;
  }
  return (
    [...ROSTER_IDS, "new" as const].find((id) => available[id]) ?? fallback
  );
}

const portraits = import.meta.glob<string>("../assets/roster/portrait/*.webp", {
  eager: true,
  query: "?url",
  import: "default",
});
const chibis = import.meta.glob<string>("../assets/roster/chibi/*.webp", {
  eager: true,
  query: "?url",
  import: "default",
});

export function portraitOf(id: RosterId): string {
  return portraits[`../assets/roster/portrait/${id}.webp`] ?? "";
}

export function chibiOf(id: RosterId, mood: Mood): string {
  // 转校生只有一张剪影，没有表情。
  const name = id === "new" ? "new" : `${id}-${mood}`;
  return chibis[`../assets/roster/chibi/${name}.webp`] ?? "";
}
