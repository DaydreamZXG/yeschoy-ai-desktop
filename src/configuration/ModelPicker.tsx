import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import { Check, ChevronDown, Search } from "lucide-react";
import { Content as PopoverContent } from "@radix-ui/react-popover";
import type { AccountModel } from "../account/session";
import { ModelCapabilityBadges } from "../model-profiles/ModelCapabilityBadges";
import {
  isImageGenerationModel,
  modelDisplayName,
  modelMatchesQuery,
} from "../model-profiles/profile";
import { Popover, PopoverTrigger } from "../components/ui/popover";

function revealOption(list: HTMLDivElement | null, index: number) {
  const option = list?.querySelectorAll<HTMLElement>('[role="option"]')[index];
  if (!list || !option) return;
  // scrollIntoView also scrolls the workbench page. Only move this list.
  const viewportTop = list.getBoundingClientRect().top + list.clientTop;
  const bounds = option.getBoundingClientRect();
  if (bounds.top < viewportTop) list.scrollTop += bounds.top - viewportTop;
  else if (bounds.bottom > viewportTop + list.clientHeight)
    list.scrollTop += bounds.bottom - viewportTop - list.clientHeight;
}

export function ModelPicker({
  models: suppliedModels,
  value,
  onChange,
  disabled = false,
  label,
  disabledReason,
}: {
  models: AccountModel[];
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  label?: string;
  // #13 「查看全部模型」：返回文案的模型不可选（置灰+原因），返回 undefined 可选。
  disabledReason?: (model: AccountModel) => string | undefined;
}) {
  const { t } = useTranslation();
  const models = useMemo(
    () => suppliedModels.filter((model) => !isImageGenerationModel(model.id)),
    [suppliedModels],
  );
  const id = useId();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [filterQuery, setFilterQuery] = useState("");
  const [activeId, setActiveId] = useState("");
  const button = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement | null>(null);
  const listObserver = useRef<ResizeObserver | null>(null);
  const composing = useRef(false);
  const tabbingAway = useRef(false);
  const options = useMemo(
    () => models.filter((model) => modelMatchesQuery(model.id, filterQuery)),
    [models, filterQuery],
  );
  const active = Math.max(
    0,
    options.findIndex((model) => model.id === activeId),
  );
  const activeOption = useRef(active);
  useLayoutEffect(() => {
    activeOption.current = active;
  }, [active]);
  const observeList = useCallback((element: HTMLDivElement | null) => {
    listObserver.current?.disconnect();
    listObserver.current = null;
    list.current = element;
    if (!element || typeof ResizeObserver === "undefined") return;
    // Collision placement can shrink the list after autofocus. Reveal the
    // current keyboard option once its real viewport size is known. Scrolling
    // alone does not resize the list, so dragging remains under user control.
    listObserver.current = new ResizeObserver(() =>
      revealOption(element, activeOption.current),
    );
    listObserver.current.observe(element);
  }, []);
  const changeOpen = (next: boolean) => {
    if (next) {
      if (disabled || !models.length) return;
      composing.current = false;
      tabbingAway.current = false;
      setQuery("");
      setFilterQuery("");
      setActiveId(value);
    }
    setOpen(next);
  };
  const filter = (text: string) => {
    setFilterQuery(text);
    setActiveId("");
  };
  useLayoutEffect(() => {
    if (list.current) list.current.scrollTop = 0;
  }, [filterQuery]);
  useEffect(() => {
    if (disabled || !models.length) setOpen(false);
  }, [disabled, models.length]);
  const choose = (model: AccountModel) => {
    if (disabled || disabledReason?.(model)) return;
    onChange(model.id);
    setOpen(false);
    button.current?.focus({ preventScroll: true });
  };
  return (
    <div className="search-model-picker">
      <span className="field-caption" id={`${id}-label`}>
        {label ?? t("modelPicker.label")}{" "}
        <small>{t("modelPicker.fullId")}</small>
      </span>
      <Popover open={open} onOpenChange={changeOpen} modal={false}>
        <PopoverTrigger asChild>
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
            tabIndex={open ? -1 : 0}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                e.preventDefault();
                changeOpen(true);
              }
            }}
          >
            <span className="model-picker-identity">
              {value && modelDisplayName(value) !== value && (
                <strong>{modelDisplayName(value)}</strong>
              )}
              <code>
                {value ||
                  t(
                    models.length
                      ? "modelPicker.choose"
                      : "modelPicker.noModels",
                  )}
              </code>
            </span>
            <ChevronDown />
          </button>
        </PopoverTrigger>
        {/* Keep search in document tab order. Radix still owns positioning and
            outside interaction; a body portal would separate it from the field. */}
        <PopoverContent
          className="model-picker-popover"
          aria-label={label}
          sideOffset={7}
          collisionPadding={16}
          onOpenAutoFocus={(event) => {
            event.preventDefault();
            search.current?.focus({ preventScroll: true });
            revealOption(list.current, active);
          }}
          onCloseAutoFocus={(event) => {
            if (tabbingAway.current) event.preventDefault();
          }}
          onEscapeKeyDown={(event) => {
            // Radix handles Escape at document capture, before the input.
            if (composing.current || event.isComposing || event.keyCode === 229)
              event.preventDefault();
          }}
        >
          <label className="model-search">
            <Search />
            <input
              ref={search}
              type="search"
              value={query}
              placeholder={t("modelPicker.searchPlaceholder")}
              autoComplete="off"
              autoCapitalize="none"
              spellCheck={false}
              aria-label={t("modelPicker.searchLabel")}
              aria-controls={`${id}-options`}
              aria-activedescendant={
                options[active] ? `${id}-option-${active}` : undefined
              }
              onChange={(e) => {
                setQuery(e.target.value);
                if (!composing.current) filter(e.target.value);
              }}
              onCompositionStart={() => {
                composing.current = true;
              }}
              onCompositionEnd={(e) => {
                composing.current = false;
                setQuery(e.currentTarget.value);
                filter(e.currentTarget.value);
              }}
              onBlur={() => {
                // Only an explicit Tab intent may dismiss on blur. A scrollbar
                // can blur with relatedTarget=null and must remain draggable.
                if (tabbingAway.current) setOpen(false);
              }}
              onKeyDown={(e) => {
                if (
                  composing.current ||
                  e.nativeEvent.isComposing ||
                  e.keyCode === 229
                ) {
                  e.stopPropagation();
                  return;
                }
                if (e.key === "Tab") {
                  // Let native Tab advance before closing. Do not unmount the
                  // focused input during keydown or loop Radix's focus scope.
                  e.stopPropagation();
                  tabbingAway.current = true;
                  return;
                }
                if (e.key === "Escape") {
                  e.preventDefault();
                  setOpen(false);
                  button.current?.focus({ preventScroll: true });
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
                  setActiveId(options[next]?.id ?? "");
                  revealOption(list.current, next);
                }
                if (e.key === "Enter") {
                  e.preventDefault();
                  if (options[active]) choose(options[active]);
                }
              }}
            />
          </label>
          <div
            ref={observeList}
            role="listbox"
            tabIndex={-1}
            id={`${id}-options`}
            aria-label={label}
            className="model-options"
            onPointerUp={(event) => {
              // A native scrollbar may take focus without focusing another
              // input. Restore typing after a mouse drag, not a touch scroll.
              if (event.pointerType === "mouse" && event.button === 0)
                search.current?.focus({ preventScroll: true });
            }}
          >
            {options.map((model, index) => {
              const reason = disabledReason?.(model);
              return (
                <button
                  type="button"
                  role="option"
                  tabIndex={-1}
                  id={`${id}-option-${index}`}
                  aria-selected={model.id === value}
                  aria-disabled={reason ? true : undefined}
                  className={
                    (index === active ? "is-active " : "") +
                    (reason ? "is-incompatible" : "")
                  }
                  key={model.id}
                  disabled={!!reason}
                  onPointerMove={(event) => {
                    if (event.pointerType === "mouse" && !event.buttons)
                      setActiveId(model.id);
                  }}
                  onClick={() => choose(model)}
                >
                  <span>
                    {modelDisplayName(model.id) !== model.id && (
                      <strong>{modelDisplayName(model.id)}</strong>
                    )}
                    <code>{model.id}</code>
                    <ModelCapabilityBadges id={model.id} />
                    {reason && (
                      <small className="option-disabled-note">{reason}</small>
                    )}
                  </span>
                  {model.id === value && <Check />}
                </button>
              );
            })}
            {!options.length && (
              <p className="model-search-empty">{t("modelPicker.noMatch")}</p>
            )}
          </div>
          <footer role="status">
            {t("modelPicker.footer", { count: options.length })}
          </footer>
        </PopoverContent>
      </Popover>
    </div>
  );
}
