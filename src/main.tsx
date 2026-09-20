import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import i18n from "./i18n";
import "./index.css";
import "./tokens.css";
import "./workbench/workbench-v2.css";

const applyTitle = () => {
  document.title = i18n.t("workbench.appTitle");
};
applyTitle();
i18n.on("languageChanged", applyTitle);

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
