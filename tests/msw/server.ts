import { setupServer } from "msw/node";

// Global vitest Tauri-mock bridge. Individual test suites mock invoke
// responses via vi.mock; no shared HTTP handlers are currently needed.
export const server = setupServer();
