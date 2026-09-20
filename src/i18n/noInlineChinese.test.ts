import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = resolve(process.cwd(), "src");
const CJK = /[一-鿿]/;

/**
 * 注释里的中文要留着。
 *
 * 这个仓库的注释记着「为什么是一个 section 不是三个」「为什么任何一条退出路径
 * 都不删」这类结论 —— 它们比被守的那些字符串更值钱。所以判定前先剥注释。
 *
 * `//` 的剥法要避开 `https://`：URL 后面同一行如果还有中文，一刀切会把它连同
 * 注释一起吃掉，变成漏报。下面 `catches what it should` 那条用一段合成源码
 * 验证这个剥法本身 —— 不然这条守卫可能自己就是坏的。
 */
export function stripComments(source: string): string {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/(?<!:)\/\/[^\n]*/g, "");
}

/**
 * 允许留中文的文件，每条都有理由。**加新条目前先问一遍它是不是界面文案。**
 */
const ALLOWED: Record<string, string> = {
  "configuration/billing.ts":
    "计费档位名是内部推导值（grep 过：没有任何 .tsx 渲染 BillingTier.name），" +
    "外加一条匹配服务端中文分组名的正则 —— 那条必须含中文才能匹配。",
  "configuration/preview.ts":
    "线路 displayName 属于 IPC 形状；界面读的是 yeschoyConfiguration.lines。",
  "diagnostics/contract.ts":
    "同上，而且 displayName 的类型就是那两个中文字面量的联合，用来校验原生侧载荷。",
  "dev/mockIpc.ts": "开发期 IPC 桩的夹具数据，不进生产构建。",
};

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) return sources(full);
    return /\.tsx?$/.test(full) && !full.includes(".test.") ? [full] : [];
  });
}

describe("界面里不许再出现内联中文", () => {
  // 迁移做完了，这条就是「做完了」的可验证定义，也防止以后有人顺手写回去。
  it("每个非测试的 .ts/.tsx 都不含注释之外的中文", () => {
    const offenders = sources(SRC)
      .map((file) => ({
        file: relative(SRC, file).replaceAll("\\", "/"),
        lines: stripComments(readFileSync(file, "utf8"))
          .split("\n")
          .map((text, index) => ({ line: index + 1, text: text.trim() }))
          .filter(({ text }) => CJK.test(text)),
      }))
      .filter(({ file, lines }) => lines.length > 0 && !(file in ALLOWED))
      .map(({ file, lines }) => `${file}:${lines[0].line} ${lines[0].text}`);
    expect(offenders).toEqual([]);
  });

  it("允许清单里的每一项都还真的需要", () => {
    // 一条规则最容易烂掉的方式，是它豁免的东西早就不存在了还挂在那儿。
    const stale = Object.keys(ALLOWED).filter((file) => {
      const full = join(SRC, file);
      return !CJK.test(stripComments(readFileSync(full, "utf8")));
    });
    expect(stale).toEqual([]);
  });

  it("剥注释这一步本身是对的", () => {
    // 守卫依赖这个剥法。它要是把整份源码都吃掉，上面那条会永远绿。
    const sample = [
      "// 这句在行注释里，不算",
      "/* 这句在块注释里，也不算 */",
      'const ok = "plain english";',
      'const url = "https://example.com/a"; // 这句仍是注释',
      'const bad1 = "这句在字符串里";',
      "  <p>这句是 JSX 文本</p>",
      'const bad3 = "https://example.com/a" + "这句跟在 URL 后面";',
    ].join("\n");
    const kept = stripComments(sample)
      .split("\n")
      .filter((line) => CJK.test(line))
      .map((line) => line.trim());
    expect(kept).toEqual([
      'const bad1 = "这句在字符串里";',
      "<p>这句是 JSX 文本</p>",
      'const bad3 = "https://example.com/a" + "这句跟在 URL 后面";',
    ]);
  });
});
