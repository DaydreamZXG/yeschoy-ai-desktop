import { useState } from "react";
import { createRoot } from "react-dom/client";
import { ModelPicker } from "../../../src/configuration/ModelPicker";
import type { AccountModel } from "../../../src/account/session";
import "../../../src/index.css";
import "../../../src/workbench/workbench-v2.css";

function Preview() {
  const [value, setValue] = useState("gpt-6-astra");
  const models = [
    "gpt-6-astra",
    "gpt-5.6-sol",
    "deepseek-v4-flash",
    "claude-sonnet-4-6",
    "org/new-model",
  ].map((id) => ({ id }) as AccountModel);
  return (
    <main style={{ maxWidth: 540, margin: "48px auto", padding: 24 }}>
      <h2>选择模型</h2>
      <p>本地组件预览 · 示例模型 · 不连接真实账户</p>
      <ModelPicker models={models} value={value} onChange={setValue} />
    </main>
  );
}
createRoot(document.getElementById("root")!).render(<Preview />);
