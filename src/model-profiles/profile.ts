import catalog from "./catalog.json";

// Reference metadata is not a model allowlist or a billing-route guarantee.
const profiles = new Map(catalog.models.map((model) => [model.id, model]));

export function modelDisplayName(id: string): string {
  return profiles.get(id)?.displayName ?? id;
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
