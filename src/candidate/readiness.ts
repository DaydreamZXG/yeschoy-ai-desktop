export const CANDIDATE_VERSION = "0.1.0" as const;

export type CandidateCapabilityId =
  | "tool_discovery"
  | "configuration_preview"
  | "line_connectivity"
  | "account_and_billing"
  | "model_and_pricing"
  | "secure_tool_credentials"
  | "configuration_apply"
  | "telemetry_upload"
  | "automatic_update";

export type CandidateCapabilityStatus =
  | "available_client_side"
  | "backend_required"
  | "security_evidence_required"
  | "disabled_in_candidate";

export interface CandidateCapability {
  id: CandidateCapabilityId;
  status: CandidateCapabilityStatus;
}

export interface CandidateReadiness {
  schemaVersion: 1;
  version: typeof CANDIDATE_VERSION;
  releaseStage: "client_candidate";
  productionReady: false;
  retainedBackupHistory: false;
  telemetryUploadEnabled: false;
  automaticUpdateEnabled: false;
  supportedToolCount: 5;
  supportedLineCount: 2;
  capabilities: CandidateCapability[];
}

const CAPABILITIES: CandidateCapability[] = [
  { id: "tool_discovery", status: "available_client_side" },
  { id: "configuration_preview", status: "available_client_side" },
  { id: "line_connectivity", status: "available_client_side" },
  { id: "account_and_billing", status: "backend_required" },
  { id: "model_and_pricing", status: "backend_required" },
  {
    id: "secure_tool_credentials",
    status: "security_evidence_required",
  },
  { id: "configuration_apply", status: "security_evidence_required" },
  { id: "telemetry_upload", status: "disabled_in_candidate" },
  { id: "automatic_update", status: "disabled_in_candidate" },
];

export function createCandidateReadiness(): CandidateReadiness {
  return {
    schemaVersion: 1,
    version: CANDIDATE_VERSION,
    releaseStage: "client_candidate",
    productionReady: false,
    retainedBackupHistory: false,
    telemetryUploadEnabled: false,
    automaticUpdateEnabled: false,
    supportedToolCount: 5,
    supportedLineCount: 2,
    capabilities: CAPABILITIES.map((capability) => ({ ...capability })),
  };
}
