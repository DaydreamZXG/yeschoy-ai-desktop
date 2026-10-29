import { decodeConnectivityProjection } from "../../diagnostics/contract";
import { invokeDecoded } from "./invokeDecoded";

let sequence = 0;

/** Read-only network evidence. Partial independently valid lines stay usable. */
export function checkLineConnectivity() {
  const requestId = `line-${Date.now().toString(36)}-${++sequence}`;
  return invokeDecoded(
    "check_line_connectivity_read_only",
    { request: { requestId } },
    (value) => decodeConnectivityProjection(value, requestId),
    "invalid_connectivity_response",
  );
}
