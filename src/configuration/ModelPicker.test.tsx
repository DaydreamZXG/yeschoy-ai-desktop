import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AccountModel } from "../account/session";
import { ModelPicker } from "./ModelPicker";
afterEach(cleanup);
const models = ["deepseek-v4-flash", "glm-5.3", "gpt-5.6-sol"].map(
  (id) => ({ id, description: "编程与对话" }) as AccountModel,
);
describe("searchable complete model IDs", () => {
  it("ru043 searches friendly names while selecting the exact API ID", () => {
    const select = vi.fn();
    render(
      <ModelPicker models={models} value="gpt-5.6-sol" onChange={select} />,
    );
    expect(screen.getByRole("combobox")).toHaveTextContent("GPT-5.6 Sol");
    expect(screen.getByRole("combobox")).toHaveTextContent("gpt-5.6-sol");
    fireEvent.click(screen.getByRole("combobox"));
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "GPT-5.6 Sol" },
    });
    fireEvent.click(screen.getByRole("option"));
    expect(select).toHaveBeenCalledWith("gpt-5.6-sol");
  });
  it("ru043 unknown prefixed models keep their complete identity", () => {
    render(
      <ModelPicker
        models={[
          {
            id: "org/gpt-6-astra",
            description: "private upstream",
          } as AccountModel,
        ]}
        value="org/gpt-6-astra"
        onChange={vi.fn()}
      />,
    );
    expect(screen.getByRole("combobox")).toHaveTextContent("org/gpt-6-astra");
    expect(screen.queryByText("GPT-6 Astra")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("combobox"));
    expect(screen.getByRole("option")).not.toHaveTextContent(
      "private upstream",
    );
  });
  it("ru043 exact IDs remain searchable when a friendly label is shown", () => {
    render(
      <ModelPicker models={models} value="gpt-5.6-sol" onChange={vi.fn()} />,
    );
    fireEvent.click(screen.getByRole("combobox"));
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "deepseek-v4-flash" },
    });
    expect(screen.getAllByRole("option")).toHaveLength(1);
    expect(screen.getByRole("option")).toHaveTextContent("DeepSeek V4 Flash");
    expect(screen.getByRole("option")).toHaveTextContent("deepseek-v4-flash");
  });
  it("neither displays nor searches unreviewed upstream descriptions", () => {
    const select = vi.fn();
    const privateModels = models.map((model) => ({
      ...model,
      description: "Synthetic upstream via private connector",
    }));
    render(
      <ModelPicker models={privateModels} value="glm-5.3" onChange={select} />,
    );
    fireEvent.click(screen.getByRole("combobox"));
    expect(screen.getAllByRole("option")).toHaveLength(3);
    expect(screen.queryByText(/Synthetic upstream/)).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "private connector" },
    });
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    fireEvent.change(screen.getByRole("searchbox"), {
      target: { value: "glm-5.3" },
    });
    fireEvent.click(screen.getByRole("option"));
    expect(select).toHaveBeenCalledWith("glm-5.3");
  });
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
