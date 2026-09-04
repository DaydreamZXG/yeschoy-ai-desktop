export type ConfigurationToolId =
  | "claude"
  | "codex"
  | "opencode"
  | "pi"
  | "dsh"
  | "hermes"
  | "openclaw";

export type ConfigurationLineId = "mainland_optimized" | "global_accelerated";

export interface ConfigurationToolDefinition {
  id: ConfigurationToolId;
  displayName: string;
  mark: string;
  targetFile: string;
  ownedFields: readonly string[];
  protocolSuffix: "" | "/v1";
  endpointStatus: "documented_preview";
}

export interface ConfigurationLineDefinition {
  id: ConfigurationLineId;
  displayName: string;
  rootUrl: "https://yeschoy.com" | "https://yeschoy.pro";
}

export interface ConfigurationPreviewRequest {
  requestId: string;
  toolId: ConfigurationToolId;
  lineId: ConfigurationLineId;
}

export interface ConfigurationPreviewProjection {
  requestId: string;
  schemaVersion: 1;
  status: "preview_only";
  toolId: ConfigurationToolId;
  displayName: string;
  lineId: ConfigurationLineId;
  lineName: string;
  rootUrl: ConfigurationLineDefinition["rootUrl"];
  protocolEndpoint: string;
  endpointStatus: "documented_preview";
  targetFile: string;
  ownedFields: readonly string[];
  model: {
    status: "server_catalog_required";
    modelId: "";
  };
  apply: {
    status: "blocked";
    blockers: readonly ConfigurationBlocker[];
  };
  networkAttempted: false;
  configurationRead: false;
  configurationWritten: false;
  credentialAccessed: false;
}

export type ConfigurationBlocker =
  | "desktop_backend_required"
  | "server_model_catalog_required"
  | "secure_credential_helper_required"
  | "exact_version_allowlist_required";

export const CONFIGURATION_TOOLS: readonly ConfigurationToolDefinition[] =
  Object.freeze([
    Object.freeze({
      id: "claude" as const,
      displayName: "Claude Code",
      mark: "C",
      targetFile: "~/.claude/settings.json",
      ownedFields: Object.freeze([
        "env.ANTHROPIC_BASE_URL",
        "apiKeyHelper",
        "env.ANTHROPIC_MODEL",
      ]),
      protocolSuffix: "" as const,
      endpointStatus: "documented_preview" as const,
    }),
    Object.freeze({
      id: "codex" as const,
      displayName: "Codex",
      mark: "X",
      targetFile: "~/.codex/config.toml",
      ownedFields: Object.freeze([
        "model_provider",
        "model",
        "model_providers.yeschoy.base_url",
        "model_providers.yeschoy.wire_api",
        "model_providers.yeschoy.auth.command",
      ]),
      protocolSuffix: "/v1" as const,
      endpointStatus: "documented_preview" as const,
    }),
    Object.freeze({
      id: "opencode" as const,
      displayName: "OpenCode",
      mark: "O",
      targetFile: "~/.config/opencode/opencode.json",
      ownedFields: Object.freeze([
        "provider.yeschoy.npm",
        "provider.yeschoy.options.baseURL",
        "provider.yeschoy.models",
        "model",
      ]),
      protocolSuffix: "/v1" as const,
      endpointStatus: "documented_preview" as const,
    }),
    Object.freeze({
      id: "pi" as const,
      displayName: "Pi",
      mark: "π",
      targetFile: "~/.pi/agent/models.json",
      ownedFields: Object.freeze([
        "providers.yeschoy.baseUrl",
        "providers.yeschoy.api",
        "providers.yeschoy.apiKey",
        "providers.yeschoy.models",
      ]),
      protocolSuffix: "/v1" as const,
      endpointStatus: "documented_preview" as const,
    }),
    Object.freeze({
      id: "dsh" as const,
      displayName: "DSH",
      mark: "D",
      targetFile: "~/.dsh/settings.yaml",
      ownedFields: Object.freeze([
        "llm-pi-ai.providers.yeschoy",
        "agent-default-model.provider",
        "agent-default-model.model",
      ]),
      protocolSuffix: "/v1" as const,
      endpointStatus: "documented_preview" as const,
    }),
    Object.freeze({
      id: "hermes" as const,
      displayName: "Hermes",
      mark: "H",
      targetFile: "~/.hermes/config.yaml",
      ownedFields: Object.freeze([
        "providers.yeschoy",
        "model.provider",
        "model.default",
      ]),
      protocolSuffix: "/v1" as const,
      endpointStatus: "documented_preview" as const,
    }),
    Object.freeze({
      id: "openclaw" as const,
      displayName: "OpenClaw",
      mark: "O",
      targetFile: "~/.openclaw/openclaw.json",
      ownedFields: Object.freeze([
        "models.providers.yeschoy",
        "secrets.providers.yeschoy-keychain",
        "agents.defaults.model.primary",
      ]),
      protocolSuffix: "/v1" as const,
      endpointStatus: "documented_preview" as const,
    }),
  ]);

export const CONFIGURATION_LINES: readonly ConfigurationLineDefinition[] =
  Object.freeze([
    Object.freeze({
      id: "mainland_optimized" as const,
      displayName: "大陆优化",
      rootUrl: "https://yeschoy.com" as const,
    }),
    Object.freeze({
      id: "global_accelerated" as const,
      displayName: "全球加速",
      rootUrl: "https://yeschoy.pro" as const,
    }),
  ]);

const BASE_BLOCKERS: readonly ConfigurationBlocker[] = Object.freeze([
  "desktop_backend_required",
  "server_model_catalog_required",
  "secure_credential_helper_required",
  "exact_version_allowlist_required",
]);

const REQUEST_ID_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;

export function createConfigurationPreview(
  request: ConfigurationPreviewRequest,
): ConfigurationPreviewProjection {
  if (!REQUEST_ID_PATTERN.test(request.requestId)) {
    throw new Error("invalid_request_id");
  }

  const tool = CONFIGURATION_TOOLS.find(
    (candidate) => candidate.id === request.toolId,
  );
  const line = CONFIGURATION_LINES.find(
    (candidate) => candidate.id === request.lineId,
  );

  if (!tool) throw new Error("invalid_tool_id");
  if (!line) throw new Error("invalid_line_id");

  return Object.freeze({
    requestId: request.requestId,
    schemaVersion: 1,
    status: "preview_only",
    toolId: tool.id,
    displayName: tool.displayName,
    lineId: line.id,
    lineName: line.displayName,
    rootUrl: line.rootUrl,
    protocolEndpoint: `${line.rootUrl}${tool.protocolSuffix}`,
    endpointStatus: tool.endpointStatus,
    targetFile: tool.targetFile,
    ownedFields: tool.ownedFields,
    model: Object.freeze({
      status: "server_catalog_required" as const,
      modelId: "" as const,
    }),
    apply: Object.freeze({
      status: "blocked" as const,
      blockers: BASE_BLOCKERS,
    }),
    networkAttempted: false,
    configurationRead: false,
    configurationWritten: false,
    credentialAccessed: false,
  });
}
