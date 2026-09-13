import React from "react";
import { createRoot } from "react-dom/client";
import App from "../../../src/App";
import "../../../src/i18n";
import "../../../src/index.css";
import "../../../src/tokens.css";
import "../../../src/workbench/workbench-v2.css";

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
