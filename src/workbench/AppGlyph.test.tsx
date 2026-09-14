import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AppGlyph } from "./AppGlyph";
import { WORKBENCH_APPS, COMING_SOON_APPS } from "./appCatalog";

describe("bundled application artwork", () => {
  it("uses real images without a recoloring mask", () => {
    const { container } = render(<AppGlyph source="/assets/brand.svg" />);
    const image = container.querySelector("img");
    expect(image?.getAttribute("src")).toBe("/assets/brand.svg");
    expect(image?.getAttribute("alt")).toBe("");
    expect(image?.style.maskImage).toBeFalsy();
  });
  it("provides local artwork for every visible tool, including coming soon", () => {
    for (const app of [...WORKBENCH_APPS, ...COMING_SOON_APPS]) {
      expect(app.icon).toBeTruthy();
      expect(app.icon).not.toMatch(/^https?:/);
      expect(app.mark).toBeUndefined();
    }
  });
});
