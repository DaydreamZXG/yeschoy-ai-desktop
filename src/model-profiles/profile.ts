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

export function modelDisplayName(id: string): string {
  return profiles.get(id)?.displayName ?? id;
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
  const model = profiles.get(id);
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
