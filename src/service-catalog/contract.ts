import type { ConfigurationLineId } from "../configuration/preview";

export const CATALOG_ERROR_CODES = [
  "none",
  "network_error",
  "timed_out",
  "http_error",
  "invalid_response",
  "response_too_large",
] as const;
export type CatalogError = (typeof CATALOG_ERROR_CODES)[number];
export const DESKTOP_CAPABILITIES = [
  "device_authorization",
  "account_read",
  "usage_read",
  "models_read",
  "pricing_read",
  "tool_keys_manage",
] as const;
export type DesktopCapability = (typeof DESKTOP_CAPABILITIES)[number];
export type BillingMode = "ratio" | "per_request" | "tiered_expr" | "unknown";

export interface PublicGroup {
  id: string;
  description: string;
  multiplier: string;
}
export interface PublicModel {
  id: string;
  groups: string[];
  endpoints: string[];
  billingMode: BillingMode;
}
export interface ServiceCatalog {
  requestId: string;
  lineId: ConfigurationLineId;
  observedAtEpochMs: number;
  catalogStatus: "available" | "unavailable";
  catalogError: CatalogError;
  serviceVersion: string;
  backendDisplayExchangeRate: string;
  groups: PublicGroup[];
  models: PublicModel[];
  desktopBackend: {
    status:
      | "not_deployed"
      | "contract_recognized"
      | "incompatible"
      | "unavailable";
    error: CatalogError;
    declaredCapabilities: DesktopCapability[];
  };
  userSpecific: false;
  secretsAccessed: false;
}

type RecordValue = Record<string, unknown>;
function object(value: unknown): value is RecordValue {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
function keys(value: RecordValue, expected: string[]): boolean {
  return (
    Object.keys(value).length === expected.length &&
    expected.every((key) => Object.prototype.hasOwnProperty.call(value, key))
  );
}
function text(
  value: unknown,
  max: number,
  allowEmpty = false,
): value is string {
  return (
    typeof value === "string" &&
    (allowEmpty || value.length > 0) &&
    Array.from(value).length <= max &&
    !/[\u0000-\u001f\u007f-\u009f\u202a-\u202e\u2066-\u2069]/u.test(value)
  );
}
function identifier(value: unknown, max: number): value is string {
  return text(value, max) && value.trim() === value;
}
function numericText(value: unknown): value is string {
  return (
    text(value, 40) &&
    /^[0-9]+(?:\.[0-9]+)?(?:e[+-]?[0-9]+)?$/i.test(value) &&
    Number.isFinite(Number(value)) &&
    Number(value) > 0
  );
}
function stringArray(
  value: unknown,
  maxItems: number,
  maxLength: number,
): value is string[] {
  return (
    Array.isArray(value) &&
    value.length <= maxItems &&
    value.every((item) => identifier(item, maxLength)) &&
    new Set(value).size === value.length
  );
}
function error(value: unknown): value is CatalogError {
  return CATALOG_ERROR_CODES.some((code) => code === value);
}

export function isServiceCatalog(
  value: unknown,
  requestId: string,
  lineId: ConfigurationLineId,
): value is ServiceCatalog {
  if (
    !object(value) ||
    !keys(value, [
      "requestId",
      "lineId",
      "observedAtEpochMs",
      "catalogStatus",
      "catalogError",
      "serviceVersion",
      "backendDisplayExchangeRate",
      "groups",
      "models",
      "desktopBackend",
      "userSpecific",
      "secretsAccessed",
    ])
  )
    return false;
  if (
    value.requestId !== requestId ||
    !/^[A-Za-z0-9_-]{1,64}$/.test(requestId) ||
    value.lineId !== lineId ||
    !["mainland_optimized", "global_accelerated"].includes(lineId)
  )
    return false;
  if (
    !Number.isSafeInteger(value.observedAtEpochMs) ||
    (value.observedAtEpochMs as number) < 0 ||
    value.userSpecific !== false ||
    value.secretsAccessed !== false
  )
    return false;
  if (
    !error(value.catalogError) ||
    !["available", "unavailable"].includes(value.catalogStatus as string)
  )
    return false;
  if (
    !text(value.serviceVersion, 100, true) ||
    !(
      value.backendDisplayExchangeRate === "" ||
      numericText(value.backendDisplayExchangeRate)
    )
  )
    return false;
  if (
    !Array.isArray(value.groups) ||
    value.groups.length > 128 ||
    !Array.isArray(value.models) ||
    value.models.length > 2048
  )
    return false;
  const groups = new Set<string>();
  for (const group of value.groups) {
    if (
      !object(group) ||
      !keys(group, ["id", "description", "multiplier"]) ||
      !identifier(group.id, 128) ||
      !text(group.description, 1000, true) ||
      !numericText(group.multiplier) ||
      groups.has(group.id)
    )
      return false;
    groups.add(group.id);
  }
  const models = new Set<string>();
  for (const model of value.models) {
    if (
      !object(model) ||
      !keys(model, ["id", "groups", "endpoints", "billingMode"]) ||
      !identifier(model.id, 200) ||
      models.has(model.id) ||
      !stringArray(model.groups, 128, 128) ||
      model.groups.length === 0 ||
      !model.groups.every((id) => groups.has(id)) ||
      !stringArray(model.endpoints, 32, 80) ||
      !["ratio", "per_request", "tiered_expr", "unknown"].includes(
        model.billingMode as string,
      )
    )
      return false;
    models.add(model.id);
  }
  if (value.catalogStatus === "available") {
    if (value.catalogError !== "none" || !identifier(value.serviceVersion, 100))
      return false;
  } else if (
    value.catalogError === "none" ||
    value.groups.length ||
    value.models.length ||
    value.serviceVersion !== "" ||
    value.backendDisplayExchangeRate !== ""
  )
    return false;
  const backend = value.desktopBackend;
  if (
    !object(backend) ||
    !keys(backend, ["status", "error", "declaredCapabilities"]) ||
    !error(backend.error) ||
    !stringArray(backend.declaredCapabilities, 6, 80) ||
    !backend.declaredCapabilities.every((name) =>
      DESKTOP_CAPABILITIES.some((known) => known === name),
    )
  )
    return false;
  switch (backend.status) {
    case "contract_recognized":
      return backend.error === "none";
    case "not_deployed":
      return (
        backend.error === "none" && backend.declaredCapabilities.length === 0
      );
    case "incompatible":
      return (
        backend.error === "invalid_response" &&
        backend.declaredCapabilities.length === 0
      );
    case "unavailable":
      return (
        backend.error !== "none" && backend.declaredCapabilities.length === 0
      );
    default:
      return false;
  }
}
