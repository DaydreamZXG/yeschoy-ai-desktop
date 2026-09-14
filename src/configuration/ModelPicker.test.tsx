import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import type { AccountModel } from "../account/session";
import { ModelPicker } from "./ModelPicker";
afterEach(cleanup);
const models = ["deepseek-v4-flash", "glm-5.3", "gpt-5.6-sol"].map(
  (id) => ({ id, description: "编程与对话" }) as AccountModel,
);
describe("searchable complete model IDs", () => {
  it("hides image generators without hiding vision-capable chat models", async () => {
    const user = userEvent.setup();
    const choices = [
      "gpt-image-1.5",
      "gpt-image-2",
      "openai/gpt-image-2",
      "gpt-6-astra",
      "deepseek-v4.1-flash",
    ].map((id) => ({ id, description: "" }) as AccountModel);
    render(
      <ModelPicker models={choices} value="gpt-6-astra" onChange={vi.fn()} />,
    );
    await user.click(screen.getByRole("combobox"));
    expect(screen.getAllByRole("option")).toHaveLength(2);
    expect(
      screen.queryByRole("option", { name: /gpt-image/ }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByRole("option", { name: /deepseek-v4.1-flash/ }),
    ).toBeInTheDocument();
  });
  it("keeps the list open when scrollbar interaction blurs the search without a new focus target", async () => {
    const user = userEvent.setup();
    const select = vi.fn();
    render(<ModelPicker models={models} value="glm-5.3" onChange={select} />);
    await user.click(screen.getByRole("combobox"));
    const list = screen.getByRole("listbox");
    fireEvent.pointerDown(list, { pointerType: "mouse", button: 0 });
    fireEvent.blur(screen.getByRole("searchbox"), { relatedTarget: null });
    expect(screen.getByRole("listbox")).toBe(list);
    fireEvent.pointerUp(list, { pointerType: "mouse", button: 0 });
    expect(select).not.toHaveBeenCalled();
    await user.click(screen.getByRole("option", { name: /glm-5.3/ }));
    expect(select).toHaveBeenCalledWith("glm-5.3");
  });

  it("returns typing focus after a mouse scrollbar drag without selecting a model", async () => {
    const user = userEvent.setup();
    const select = vi.fn();
    render(<ModelPicker models={models} value="glm-5.3" onChange={select} />);
    await user.click(screen.getByRole("combobox"));
    const input = screen.getByRole("searchbox");
    const list = screen.getByRole("listbox");
    await user.pointer({ target: list, keys: "[MouseLeft>]" });
    act(() => input.blur());
    expect(input).not.toHaveFocus();
    expect(screen.getByRole("listbox")).toBe(list);
    await user.pointer({ target: list, keys: "[/MouseLeft]" });
    expect(input).toHaveFocus();
    await user.keyboard("gpt");
    expect(input).toHaveValue("gpt");
    expect(select).not.toHaveBeenCalled();
  });

  it.each(["Enter", "ArrowDown", "ArrowUp", "Escape"])(
    "does not treat IME %s as a picker command or filter unfinished composition",
    async (key) => {
      const user = userEvent.setup();
      const select = vi.fn();
      render(<ModelPicker models={models} value="glm-5.3" onChange={select} />);
      await user.click(screen.getByRole("combobox"));
      const input = screen.getByRole("searchbox");
      const active = input.getAttribute("aria-activedescendant");
      fireEvent.compositionStart(input);
      fireEvent.change(input, { target: { value: "g" } });
      fireEvent.keyDown(input, { key, isComposing: true, keyCode: 229 });
      expect(select).not.toHaveBeenCalled();
      expect(screen.getByRole("searchbox")).toHaveValue("g");
      expect(screen.getAllByRole("option")).toHaveLength(3);
      expect(input).toHaveAttribute("aria-activedescendant", active);
      fireEvent.compositionEnd(input, {
        data: "gpt",
        target: { value: "gpt" },
      });
      expect(screen.getByRole("searchbox")).toHaveValue("gpt");
      expect(screen.getAllByRole("option")).toHaveLength(1);
      fireEvent.keyDown(input, { key: "Enter" });
      expect(select).toHaveBeenCalledWith("gpt-5.6-sol");
    },
  );

  it("ignores the IME commit Enter when compositionend precedes keydown (keyCode 229)", async () => {
    const user = userEvent.setup();
    const select = vi.fn();
    render(<ModelPicker models={models} value="glm-5.3" onChange={select} />);
    await user.click(screen.getByRole("combobox"));
    const input = screen.getByRole("searchbox");
    fireEvent.compositionStart(input);
    fireEvent.compositionEnd(input, { data: "gpt", target: { value: "gpt" } });
    fireEvent.keyDown(input, {
      key: "Enter",
      isComposing: false,
      keyCode: 229,
    });
    expect(select).not.toHaveBeenCalled();
    expect(screen.getByRole("listbox")).toBeInTheDocument();
  });

  it("resets a scrolled result list when the query changes or is cleared", async () => {
    const user = userEvent.setup();
    render(<ModelPicker models={models} value="glm-5.3" onChange={vi.fn()} />);
    await user.click(screen.getByRole("combobox"));
    const list = screen.getByRole("listbox");
    list.scrollTop = 200;
    await user.type(screen.getByRole("searchbox"), "g");
    expect(list.scrollTop).toBe(0);
    list.scrollTop = 80;
    await user.clear(screen.getByRole("searchbox"));
    expect(list.scrollTop).toBe(0);
  });

  it("starts a fresh query and active selection when reopened from the keyboard", async () => {
    const user = userEvent.setup();
    render(<ModelPicker models={models} value="glm-5.3" onChange={vi.fn()} />);
    const trigger = screen.getByRole("combobox");
    await user.click(trigger);
    await user.type(screen.getByRole("searchbox"), "gpt");
    await user.keyboard("{Escape}");
    expect(trigger).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("searchbox")).toHaveValue("");
    expect(screen.getAllByRole("option")).toHaveLength(3);
    expect(
      document.getElementById(
        screen.getByRole("searchbox").getAttribute("aria-activedescendant")!,
      ),
    ).toHaveTextContent("glm-5.3");
  });

  it("moves keyboard results inside the list without scrolling the outer page", async () => {
    const user = userEvent.setup();
    const pageScroll = vi.fn();
    const previous = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = pageScroll;
    try {
      render(
        <ModelPicker models={models} value={models[0].id} onChange={vi.fn()} />,
      );
      await user.click(screen.getByRole("combobox"));
      const list = screen.getByRole("listbox");
      vi.spyOn(list, "getBoundingClientRect").mockReturnValue({
        top: 300,
        bottom: 400,
      } as DOMRect);
      Object.defineProperty(list, "clientHeight", {
        configurable: true,
        value: 100,
      });
      vi.spyOn(
        screen.getAllByRole("option")[1],
        "getBoundingClientRect",
      ).mockReturnValue({ top: 420, bottom: 460 } as DOMRect);
      await user.keyboard("{ArrowDown}");
      expect(pageScroll).not.toHaveBeenCalled();
      expect(list.scrollTop).toBe(60);
    } finally {
      Element.prototype.scrollIntoView = previous;
    }
  });

  it("reveals the active option after collision layout shrinks the list", async () => {
    const observers = new Map<
      Element,
      { resize: () => void; disconnect: ReturnType<typeof vi.fn> }
    >();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        disconnect = vi.fn();
        unobserve = vi.fn();
        constructor(private callback: () => void) {}
        observe = (element: Element) => {
          observers.set(element, {
            resize: this.callback,
            disconnect: this.disconnect,
          });
        };
      },
    );
    try {
      const user = userEvent.setup();
      const select = vi.fn();
      const { unmount } = render(
        <ModelPicker models={models} value={models[2].id} onChange={select} />,
      );
      await user.click(screen.getByRole("combobox"));
      const list = screen.getByRole("listbox");
      const observer = observers.get(list);
      expect(observer).toBeDefined();
      vi.spyOn(list, "getBoundingClientRect").mockReturnValue({
        top: 300,
        bottom: 400,
      } as DOMRect);
      Object.defineProperty(list, "clientHeight", {
        configurable: true,
        value: 100,
      });
      vi.spyOn(
        screen.getAllByRole("option")[2],
        "getBoundingClientRect",
      ).mockReturnValue({ top: 440, bottom: 480 } as DOMRect);
      act(() => observer!.resize());
      expect(list.scrollTop).toBe(80);

      vi.spyOn(
        screen.getAllByRole("option")[1],
        "getBoundingClientRect",
      ).mockReturnValue({ top: 420, bottom: 460 } as DOMRect);
      await user.keyboard("{ArrowUp}");
      list.scrollTop = 0;
      act(() => observer!.resize());
      expect(list.scrollTop).toBe(60);
      expect(select).not.toHaveBeenCalled();
      unmount();
      expect(observer!.disconnect).toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("closes on outside pointer or Tab focus, not typing, and does not commit a model", async () => {
    const user = userEvent.setup();
    const select = vi.fn();
    render(
      <>
        <ModelPicker models={models} value="glm-5.3" onChange={select} />
        <button>外部操作</button>
      </>,
    );
    await user.click(screen.getByRole("combobox"));
    expect(screen.getByRole("listbox")).toHaveAttribute("tabindex", "-1");
    await user.type(screen.getByRole("searchbox"), "gpt");
    expect(screen.getByRole("searchbox")).toHaveValue("gpt");
    await user.click(screen.getByRole("button", { name: "外部操作" }));
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    await user.click(screen.getByRole("combobox"));
    await user.tab();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "外部操作" })).toHaveFocus();
    expect(select).not.toHaveBeenCalled();
  });

  it("Shift-Tab returns to the control before the picker, without stealing focus back", async () => {
    const user = userEvent.setup();
    render(
      <>
        <button>前一个控件</button>
        <ModelPicker models={models} value="glm-5.3" onChange={vi.fn()} />
        <button>后一个控件</button>
      </>,
    );
    await user.click(screen.getByRole("combobox"));
    await user.tab({ shift: true });
    expect(screen.getByRole("button", { name: "前一个控件" })).toHaveFocus();
    expect(screen.queryByRole("listbox")).not.toBeInTheDocument();
  });

  it("keeps independent queries and selections across two shared picker instances", async () => {
    const user = userEvent.setup();
    const setupSelection = vi.fn();
    const priceSelection = vi.fn();
    render(
      <>
        <ModelPicker
          models={models}
          value="glm-5.3"
          onChange={setupSelection}
          label="接入模型"
        />
        <ModelPicker
          models={models}
          value="glm-5.3"
          onChange={priceSelection}
          label="价格模型"
        />
      </>,
    );
    await user.click(screen.getByRole("combobox", { name: /接入模型/ }));
    await user.type(screen.getByRole("searchbox"), "gpt");
    await user.click(screen.getByRole("combobox", { name: /价格模型/ }));
    expect(screen.getAllByRole("listbox")).toHaveLength(1);
    expect(screen.getByRole("searchbox")).toHaveValue("");
    await user.click(screen.getByRole("option", { name: /deepseek-v4-flash/ }));
    expect(priceSelection).toHaveBeenCalledWith("deepseek-v4-flash");
    expect(setupSelection).not.toHaveBeenCalled();
    expect(getComputedStyle(document.body).pointerEvents).not.toBe("none");
  });

  it("does not submit a stale active model when account models change during search", async () => {
    const user = userEvent.setup();
    const select = vi.fn();
    const { rerender } = render(
      <ModelPicker models={models} value="glm-5.3" onChange={select} />,
    );
    await user.click(screen.getByRole("combobox"));
    await user.keyboard("{ArrowDown}");
    rerender(
      <ModelPicker models={[models[0]]} value="glm-5.3" onChange={select} />,
    );
    const input = screen.getByRole("searchbox");
    expect(
      document.getElementById(input.getAttribute("aria-activedescendant")!),
    ).toHaveTextContent(models[0].id);
    await user.keyboard("{Enter}");
    expect(select).toHaveBeenCalledWith(models[0].id);
  });

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
