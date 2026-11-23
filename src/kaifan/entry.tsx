import React from "react";
import ReactDOM from "react-dom/client";
// i18n 要排在界面前面：有的模块一加载就取文案，晚了拿到的是键名。
import "../i18n";
import "./kaifan.css";
import { HandoffStage, handoffCast } from "./HandoffStage";
import { KaifanApp } from "./KaifanApp";
import { UpdateProvider } from "../update/UpdateProvider";

// 打开应用时的那块暗场是另一个窗口，装的是同一份页面，地址里带着由谁登场。
// 它只画那一页：不登录、不查更新、不碰账户。
const cast = handoffCast(window.location.search);
if (cast) document.documentElement.classList.add("handoff");

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    {cast ? (
      <HandoffStage {...cast} />
    ) : (
      <UpdateProvider>
        <KaifanApp />
      </UpdateProvider>
    )}
  </React.StrictMode>,
);
