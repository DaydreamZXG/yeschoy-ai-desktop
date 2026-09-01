// Visual-test fixture only. Not reachable from the production entry graph.
import React from "react";
import ReactDOM from "react-dom/client";
import App from "../../../src/App";
import "../../../src/i18n";
import "../../../src/index.css";
import { TOOL_CATALOG } from "../../../src/tool-discovery/contract";

Object.defineProperty(window, "__TAURI_INTERNALS__", {value: {
  invoke: async (command: string, args: {request: {requestId: string}}) => {
    if (command === "set_window_appearance") return {};
    if (command !== "scan_tools_read_only_v2") throw Error("Not available in visual fixture");
    return {requestId: args.request.requestId, platform:"macos", startedAtEpochMs:1, completedAtEpochMs:2,
      tools: TOOL_CATALOG.map(tool => tool.id === "codex" ? {
        toolId:tool.id,displayName:tool.displayName,status:"detected_unverified",version:"0.146.0",
        candidateCount:1,bundledCount:1,selection:"single_installation",locationHint:"path",compatibility:"unverified_read_only",reasonCode:"exact_version_not_allowlisted"
      } : {toolId:tool.id,displayName:tool.displayName,status:"not_found",version:"",candidateCount:0,bundledCount:0,selection:"not_found",locationHint:"none",compatibility:"not_applicable",reasonCode:"tool_not_found"})};
  }
}});
ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><App /></React.StrictMode>);
