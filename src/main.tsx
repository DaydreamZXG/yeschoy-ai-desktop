import React from "react";
import ReactDOM from "react-dom/client";
// i18n 必须排在 App 前面：模块求值按 import 顺序进行，App 的依赖树里
// 有模块级的 `i18n.t()`（如 `finance.ts` 的 creditUnit 在渲染前就可能被调用），
// 排在后面就会在 init 之前跑，拿到的是键名而不是文案。
import i18n from "./i18n";
import App from "./App";
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
