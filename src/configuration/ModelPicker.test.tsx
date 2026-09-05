import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AccountModel } from "../account/session";
import { ModelPicker } from "./ModelPicker";
afterEach(cleanup);
const models = ["deepseek-v4-flash", "glm-5.3", "gpt-5.6-sol"].map(
  (id) => ({ id, description: "编程与对话" }) as AccountModel,
);
describe("searchable complete model IDs", () => {
  it("filters models, selects by keyboard and returns focus to its trigger", () => {
    const select = vi.fn();
    render(
      <ModelPicker
        models={models}
        value="deepseek-v4-flash"
        onChange={select}
      />,
    );
    const trigger = screen.getByRole("combobox");
    fireEvent.click(trigger);
    const search = screen.getByRole("searchbox");
    expect(search).toHaveFocus();
    fireEvent.change(search, { target: { value: "gpt" } });
    expect(screen.getAllByRole("option")).toHaveLength(1);
    expect(screen.getByRole("option")).toHaveTextContent("gpt-5.6-sol");
    fireEvent.keyDown(search, { key: "Enter" });
    expect(select).toHaveBeenCalledWith("gpt-5.6-sol");
    expect(trigger).toHaveFocus();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });
  it("supports cancel, no matches and disabled state without choosing anything", () => {
    const select = vi.fn();
    const { rerender } = render(
      <ModelPicker models={models} value="glm-5.3" onChange={select} />,
    );
    fireEvent.click(screen.getByRole("combobox"));
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "does-not-exist" },
    });
    expect(screen.getByText(/没有匹配的模型/)).toBeInTheDocument();
    fireEvent.keyDown(screen.getByRole("searchbox"), { key: "Escape" });
    expect(select).not.toHaveBeenCalled();
    rerender(
      <ModelPicker
        models={models}
        value="glm-5.3"
        onChange={select}
        disabled
      />,
    );
    expect(screen.getByRole("combobox")).toBeDisabled();
  });
});
