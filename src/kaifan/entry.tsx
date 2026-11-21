import React from "react";
import ReactDOM from "react-dom/client";
// i18n 要排在界面前面：有的模块一加载就取文案，晚了拿到的是键名。
import "../i18n";
import "./kaifan.css";
import { KaifanApp } from "./KaifanApp";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <KaifanApp />
  </React.StrictMode>,
);
