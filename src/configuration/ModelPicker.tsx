import { useEffect, useId, useRef, useState } from "react";
import { Check, ChevronDown, Search } from "lucide-react";
import type { AccountModel } from "../account/session";
import { modelDisplayName, modelMatchesQuery } from "../model-profiles/profile";

export function ModelPicker({
  models,
  value,
  onChange,
  disabled = false,
  label = "选择模型",
}: {
  models: AccountModel[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  label?: string;
}) {
  const id = useId();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const root = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const options = models.filter((m) => modelMatchesQuery(m.id, query));
  useEffect(() => {
    if (!open) return;
    search.current?.focus();
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);
  const choose = (model: string) => {
    onChange(model);
    setOpen(false);
    button.current?.focus();
  };
  return (
    <div
      className="search-model-picker"
      ref={root}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setOpen(false);
      }}
    >
      <span className="field-caption" id={`${id}-label`}>
        {label} <small>完整模型 ID</small>
      </span>
      <button
        ref={button}
        type="button"
        className="model-picker-trigger"
        role="combobox"
        aria-labelledby={`${id}-label`}
        aria-expanded={open}
        aria-controls={`${id}-options`}
        aria-haspopup="listbox"
        disabled={disabled || !models.length}
        onClick={() => {
          setOpen(!open);
          setQuery("");
          setActive(
            Math.max(
              0,
              models.findIndex((m) => m.id === value),
            ),
          );
        }}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            setOpen(true);
          }
        }}
      >
        <span className="model-picker-identity">
          {value && modelDisplayName(value) !== value && (
            <strong>{modelDisplayName(value)}</strong>
          )}
          <code>
            {value || (models.length ? "选择一个模型" : "暂无可用模型")}
          </code>
        </span>
        <ChevronDown />
      </button>
      {open && (
        <div className="model-picker-popover">
          <label className="model-search">
            <Search />
            <input
              ref={search}
              type="search"
              value={query}
              placeholder="搜索模型名称…"
              aria-label="搜索模型"
              aria-controls={`${id}-options`}
              aria-activedescendant={
                options[active] ? `${id}-option-${active}` : undefined
              }
              onChange={(e) => {
                setQuery(e.target.value);
                setActive(0);
              }}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  e.preventDefault();
                  setOpen(false);
                  button.current?.focus();
                }
                if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                  e.preventDefault();
                  const next = Math.max(
                    0,
                    Math.min(
                      options.length - 1,
                      active + (e.key === "ArrowDown" ? 1 : -1),
                    ),
                  );
                  setActive(next);
                  document
                    .getElementById(`${id}-option-${next}`)
                    ?.scrollIntoView({ block: "nearest" });
                }
                if (e.key === "Enter" && options[active]) {
                  e.preventDefault();
                  choose(options[active].id);
                }
              }}
            />
          </label>
          <div
            role="listbox"
            id={`${id}-options`}
            aria-label={label}
            className="model-options"
          >
            {options.map((model, index) => (
              <button
                type="button"
                role="option"
                tabIndex={-1}
                id={`${id}-option-${index}`}
                aria-selected={model.id === value}
                className={index === active ? "is-active" : ""}
                key={model.id}
                onPointerMove={() => setActive(index)}
                onClick={() => choose(model.id)}
              >
                <span>
                  {modelDisplayName(model.id) !== model.id && (
                    <strong>{modelDisplayName(model.id)}</strong>
                  )}
                  <code>{model.id}</code>
                </span>
                {model.id === value && <Check />}
              </button>
            ))}
            {!options.length && (
              <p className="model-search-empty">
                没有匹配的模型，试试其他关键词。
              </p>
            )}
          </div>
          <footer>{options.length} 个模型 · 价格和分组在选择后显示</footer>
        </div>
      )}
    </div>
  );
}
