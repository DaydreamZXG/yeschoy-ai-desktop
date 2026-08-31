import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { DESKTOP_CAPABILITIES } from "./contract";

const read = (path: string) =>
  readFileSync(resolve(process.cwd(), path), "utf8");
describe("backend handoff and client authority", () => {
  it("ships a closed bootstrap example matching the renderer capability vocabulary", () => {
    const schema = JSON.parse(
      read("contracts/desktop-bootstrap.v1.schema.json"),
    );
    const example = JSON.parse(
      read("contracts/fixtures/desktop-bootstrap/recognized.json"),
    );
    const bad = JSON.parse(
      read("contracts/fixtures/desktop-bootstrap/incompatible.json"),
    );
    expect(schema.additionalProperties).toBe(false);
    expect(schema.properties.data.additionalProperties).toBe(false);
    expect(Object.keys(example.data).sort()).toEqual(
      [...schema.properties.data.required].sort(),
    );
    expect(example.data.schema_version).toBe(
      schema.properties.data.properties.schema_version.const,
    );
    expect(example.data.contract_id).toBe("desktop-bootstrap-v1");
    expect(Object.keys(example.data.capabilities).sort()).toEqual(
      [...DESKTOP_CAPABILITIES].sort(),
    );
    expect(
      Object.values(example.data.capabilities).every(
        (value) => value === false,
      ),
    ).toBe(true);
    expect(bad.data.schema_version).not.toBe(example.data.schema_version);
    expect(read("src-tauri/src/service_catalog_core.rs")).toMatch(
      /include_str!\(\s*"\.\.\/\.\.\/contracts\/fixtures\/desktop-bootstrap\/recognized\.json"\s*\)/,
    );
  });
  it("keeps transport paths and native credential authority closed", () => {
    const native = read("src-tauri/src/service_catalog.rs");
    for (const path of [
      "/api/status",
      "/api/pricing",
      "/api/desktop/v1/bootstrap",
    ])
      expect(native).toContain(path);
    expect(native).toContain(".https_only(true)");
    expect(native).toContain(".redirect(Policy::none())");
    expect(native).toContain(".no_proxy()");
    expect(native).toContain("MAX_RESPONSE_BYTES");
    expect(native).toContain("try_acquire()");
    const implementation = native.split("#[cfg(test)]")[0];
    for (const forbidden of [
      ".post(",
      ".put(",
      ".delete(",
      "bearer_auth(",
      "cookie_store(",
      "Command::new",
      "File::create",
      "danger_accept_invalid_certs",
    ])
      expect(implementation).not.toContain(forbidden);
    expect(read("src-tauri/tauri.conf.json")).toContain(
      "connect-src 'self' ipc: http://ipc.localhost",
    );
    expect(read("src/service-catalog/ServiceCatalogPanel.tsx")).not.toContain(
      "dangerouslySetInnerHTML",
    );
  });
  it("hands server work to the technical owner without claiming full integration", () => {
    const guide = read("outputs/野菜API-后端技术交接-客户端接入.md");
    for (const expected of [
      "eb48396d5fe97d27772d0cd5e3ca8aa5caa4f3e9",
      "P0-A",
      "P0-B",
      "P0-C",
      "pricing_version",
      "6.75",
      "GroupGroupRatio",
      "tool-keys",
      "设备撤销",
      "OpenAPI/JSON Schema",
      "本页不是生产上线批准",
      "没有完成桌面会话",
      "不是本轮已经完成的客户端调用器",
    ])
      expect(guide).toContain(expected);
    expect(read("outputs/野菜API-NewAPI-未来改造清单.md")).toContain(
      "野菜API-后端技术交接-客户端接入.md",
    );
  });
  it("keeps all four locale shapes complete and runtime free of synthetic fixtures", () => {
    const flatten = (value: Record<string, unknown>, prefix = ""): string[] =>
      Object.entries(value)
        .flatMap(([key, child]) =>
          child && typeof child === "object"
            ? flatten(child as Record<string, unknown>, `${prefix}${key}.`)
            : [`${prefix}${key}`],
        )
        .sort();
    const locales = ["zh", "zh-TW", "en", "ja"].map(
      (language) =>
        JSON.parse(read(`src/i18n/locales/${language}.json`)).yeschoyCatalog,
    );
    for (const locale of locales) {
      expect(flatten(locale)).toEqual(flatten(locales[0]));
      expect(locale.publicNotice).toBeTruthy();
      expect(locale.compatibilityNotice).toBeTruthy();
    }
    for (const path of [
      "src/service-catalog/contract.ts",
      "src/service-catalog/access-plan.ts",
      "src/service-catalog/ServiceCatalogPanel.tsx",
    ])
      expect(read(path)).not.toContain("test-fixtures");
  });
});
