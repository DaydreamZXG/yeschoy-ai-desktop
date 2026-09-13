import type { ActivationToolId } from "./activation";

export type SetupAction = "configure" | "change-model" | "repair";
export interface SetupIntent {
  appId: ActivationToolId;
  action: SetupAction;
  revision: number;
}
