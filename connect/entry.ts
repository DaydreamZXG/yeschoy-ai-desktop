import { createYeschoyConnect } from "./yeschoy-connect";

declare global {
  interface Window {
    YeschoyConnect?: ReturnType<typeof createYeschoyConnect>;
  }
}

const scriptEl = document.currentScript;
const downloadOverride =
  scriptEl instanceof HTMLScriptElement
    ? scriptEl.dataset.downloadUrl?.trim()
    : undefined;

const api = createYeschoyConnect({
  downloadUrl: downloadOverride || undefined,
});

window.YeschoyConnect = api;

function shouldAutoLaunchFromDesktop(): boolean {
  const params = new URLSearchParams(window.location.search);
  return params.get("auto") === "1";
}

function init(): void {
  api.bindAuto();
  if (!shouldAutoLaunchFromDesktop()) return;

  const button = document.querySelector<HTMLElement>("[data-yeschoy-connect]");
  if (button) {
    button.hidden = true;
    button.setAttribute("aria-hidden", "true");
  }
  document.dispatchEvent(new CustomEvent("yeschoy-connect-auto-start"));

  void api.open().catch((err) => {
    if (button) {
      button.hidden = false;
      button.removeAttribute("aria-hidden");
    }
    button?.dispatchEvent(
      new CustomEvent("yeschoy-connect-error", {
        detail: err,
        bubbles: true,
      }),
    );
  });
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}
