import { createConfigurationPreview } from "../configuration/preview";
import type {
  ConfigurationLineId,
  ConfigurationToolId,
} from "../configuration/preview";
import { isServiceCatalog } from "./contract";
import type { BillingMode, ServiceCatalog } from "./contract";

const TOOL_PROTOCOL = {
  claude: "anthropic",
  codex: "openai-response",
  opencode: "openai",
  pi: "openai",
  dsh: null,
} as const;

export interface ToolAccessPlan {
  toolId: ConfigurationToolId;
  lineId: ConfigurationLineId;
  groupId: string;
  modelId: string;
  baseUrl: string;
  requiredProtocol: string;
  status: "protocol_declared" | "protocol_not_declared" | "dsh_unverified";
  billingMode: BillingMode;
  accountAccess: "unverified";
  groupRestrictions: "unverified";
  exactToolVersion: "unverified";
  applyAllowed: false;
}

export function createToolAccessPlan(
  catalog: ServiceCatalog,
  toolId: ConfigurationToolId,
  lineId: ConfigurationLineId,
  groupId: string,
  modelId: string,
): ToolAccessPlan {
  if (
    !isServiceCatalog(catalog, catalog.requestId, lineId) ||
    catalog.catalogStatus !== "available"
  )
    throw new Error("invalid_catalog");
  const group = catalog.groups.find((group) => group.id === groupId);
  const model = catalog.models.find(
    (model) => model.id === modelId && model.groups.includes(groupId),
  );
  if (
    !group ||
    !model ||
    !Object.prototype.hasOwnProperty.call(TOOL_PROTOCOL, toolId)
  )
    throw new Error("invalid_selection");
  const local = createConfigurationPreview({
    requestId: "catalog-access-preview",
    toolId,
    lineId,
  });
  const protocol = TOOL_PROTOCOL[toolId];
  return {
    toolId,
    lineId,
    groupId: group.id,
    modelId: model.id,
    baseUrl: local.protocolEndpoint,
    requiredProtocol: protocol ?? "",
    status:
      protocol === null
        ? "dsh_unverified"
        : model.endpoints.includes(protocol)
          ? "protocol_declared"
          : "protocol_not_declared",
    billingMode: model.billingMode,
    accountAccess: "unverified",
    groupRestrictions: "unverified",
    exactToolVersion: "unverified",
    applyAllowed: false,
  };
}
