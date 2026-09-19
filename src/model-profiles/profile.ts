import catalog from "./catalog.json";

// Reference metadata is not a model allowlist or a billing-route guarantee.
let profiles = new Map(catalog.models.map((model) => [model.id, model]));

/**
 * Swap the lookup table for a validated remote catalog revision (M5).
 * Passing null restores the bundled catalog. Only remoteCatalog.ts calls
 * this, after its sha256 + schema + freshness checks passed.
 */
export function applyModelCatalogOverride(
  models: typeof catalog.models | null,
): void {
  profiles = new Map(
    (models ?? catalog.models).map((model) => [model.id, model]),
  );
}

/**
 * 中转把推理档位烧进了模型 ID 里：`gemini-3.7-flash-high` 就是
 * `gemini-3.7-flash` 跑在 `high` 档。目录里收的是基础型号，所以精确查不到的
 * 时候要把后缀摘掉再查一次，否则这些型号在界面上什么标注都没有。
 *
 * 后缀表和适用范围都照抄中转的 `ParseOpenAIReasoningEffortFromModelSuffix`
 * （`relaykit/relayconvert/reasoning/suffix.go`）：它只对 `gpt-*`、`o<n>*`、
 * `claude-*`、`gemini-*` 这几族拆后缀。这一条限制不是可有可无的 —— 目录里
 * `qwen3.8-max` 是一个完整型号名，不在这几族里，所以它的 `-max` 不会被误拆。
 */
const EFFORT_SUFFIXES = [
  "-max",
  "-xhigh",
  "-high",
  "-medium",
  "-low",
  "-minimal",
  "-none",
] as const;

const EFFORT_SUFFIXED_FAMILIES = /^(?:gpt-[a-z0-9]|o[1-9]|claude-|gemini-)/;

function splitEffortSuffix(
  id: string,
): { base: string; effort: string } | null {
  const suffix = EFFORT_SUFFIXES.find((candidate) => id.endsWith(candidate));
  if (!suffix) return null;
  const base = id.slice(0, -suffix.length);
  // 命名空间前缀保持不透明，只看最后一段是不是这几族 —— 跟中转的
  // `lastModelPathSegment` 一致。
  const bare = base.toLowerCase().split("/").at(-1) ?? "";
  if (!EFFORT_SUFFIXED_FAMILIES.test(bare)) return null;
  return { base, effort: suffix.slice(1) };
}

/**
 * 精确命中优先。只有查不到时才退一步拆后缀，这样将来目录里直接收了某个带
 * 后缀的完整型号名，它仍然说了算。
 */
function resolveProfile(id: string) {
  const exact = profiles.get(id);
  if (exact) return exact;
  const split = splitEffortSuffix(id);
  if (!split) return undefined;
  const base = profiles.get(split.base);
  if (!base) return undefined;
  // ID 已经把档位钉死了，所以这个型号的档位阶梯只有一级，也没有「默认档」
  // 可言 —— 照搬基础型号的整条阶梯会让人以为还能选。
  return {
    ...base,
    displayName: `${base.displayName} (${split.effort})`,
    reasoningLevels: [split.effort],
    defaultReasoning: undefined,
  };
}

export function modelDisplayName(id: string): string {
  return resolveProfile(id)?.displayName ?? id;
}

// Image generation is not a chat/agent model. Do not hide vision-capable chat.
export function isImageGenerationModel(id: string): boolean {
  return /^(?:gpt-image(?:-|$)|dall-e(?:-|$))/.test(
    id.toLowerCase().split("/").at(-1) ?? "",
  );
}

export interface ModelCapabilities {
  contextWindow?: number;
  maxOutputTokens?: number;
  input?: readonly string[];
  reasoningLevels?: readonly string[];
  defaultReasoning?: string;
  toolUse?: boolean;
}

/**
 * Read-only capability lookup against the reviewed reference catalog.
 * The catalog is metadata, not an allowlist: unknown models return an empty
 * object — capabilities are never guessed (PRD 6.5), and missing fields are
 * simply omitted so callers render no badge instead of "unsupported".
 */
export function modelCapabilities(id: string): ModelCapabilities {
  const model = resolveProfile(id);
  if (!model) return {};
  return {
    ...(model.contextWindow !== undefined
      ? { contextWindow: model.contextWindow }
      : {}),
    ...(model.maxOutputTokens !== undefined
      ? { maxOutputTokens: model.maxOutputTokens }
      : {}),
    ...(model.input !== undefined ? { input: model.input } : {}),
    ...(model.reasoningLevels !== undefined
      ? { reasoningLevels: model.reasoningLevels }
      : {}),
    ...(model.defaultReasoning !== undefined
      ? { defaultReasoning: model.defaultReasoning }
      : {}),
    ...(model.toolUse !== undefined ? { toolUse: model.toolUse } : {}),
  };
}

export function modelMatchesQuery(id: string, query: string): boolean {
  const normalized = query.normalize("NFKC").trim().toLowerCase();
  if (!normalized) return true;
  // Search tolerates display/API separators, but never rewrites the selected ID
  // or indexes private upstream descriptions. Keep version dots and namespaces.
  const compact = (text: string) =>
    text
      .normalize("NFKC")
      .toLowerCase()
      .replace(/[\s_\-\u2010-\u2015]+/g, "");
  const terms = normalized.split(/\s+/).map(compact).filter(Boolean);
  if (!terms.length) return false;
  const identities = [compact(id), compact(modelDisplayName(id))];
  return terms.every((term) => {
    if (/^\d+(?:\.\d+)*$/.test(term)) {
      // "GPT 6" must not suggest GPT-5.6 merely because its minor version is 6.
      const version = new RegExp(
        `(?:^|[^\\d.])${term.replace(/\./g, "\\.")}(?=$|[^\\d])`,
      );
      return identities.some((text) => version.test(text));
    }
    return identities.some((text) => text.includes(term));
  });
}
