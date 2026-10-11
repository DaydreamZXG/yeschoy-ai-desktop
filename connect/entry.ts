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

function init(): void {
  api.bindAuto();
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", init);
} else {
  init();
}
