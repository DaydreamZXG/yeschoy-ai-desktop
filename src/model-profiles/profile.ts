import catalog from "./catalog.json";

// Reference metadata is not a model allowlist or a billing-route guarantee.
const profiles = new Map(catalog.models.map((model) => [model.id, model]));

export function modelDisplayName(id: string): string {
  return profiles.get(id)?.displayName ?? id;
}

export function modelMatchesQuery(id: string, query: string): boolean {
  const needle = query.trim().toLocaleLowerCase();
  return [id, modelDisplayName(id)].some((text) =>
    text.toLocaleLowerCase().includes(needle),
  );
}
