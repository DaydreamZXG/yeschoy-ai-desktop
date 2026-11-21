// 入口只决定加载哪一套界面。两套界面各带各的样式，互不加载（见 uiChoice.ts）。
import { readUiChoice } from "./uiChoice";

if (readUiChoice() === "classic") void import("./classicEntry");
else void import("./kaifan/entry");
