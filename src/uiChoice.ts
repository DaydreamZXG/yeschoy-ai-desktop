/**
 * 现在有两套界面：新的「开饭」界面，和还没重做完的旧界面。
 *
 * 打开客户端看到的永远是新界面。新界面里还没重做的页（接入、账本、公告、
 * 设置）点进去时临时换到旧界面的那一页；这个选择只记在这一次运行里
 * （sessionStorage），关掉再开又回到新界面。两套界面各自加载各自的样式，
 * 所以换的时候整页重载，而不是在一个页面里混着渲染。
 */
const CHOICE_KEY = "kaifan.ui";
const VIEW_KEY = "kaifan.classicView";
const APP_KEY = "kaifan.classicApp";
const RETURN_KEY = "kaifan.returning";

export const CLASSIC_VIEWS = [
  "setup",
  "account",
  "announcements",
  "settings",
] as const;
export type ClassicView = (typeof CLASSIC_VIEWS)[number];

function storage(): Storage | null {
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
}

export function readUiChoice(): "kaifan" | "classic" {
  return storage()?.getItem(CHOICE_KEY) === "classic" ? "classic" : "kaifan";
}

/**
 * 旧界面这次该先打开哪一页、哪个应用。模块加载时读一次就清掉，之后的导航
 * 归旧界面自己管（放在模块顶层，是因为严格模式下组件的初始化函数会跑两遍）。
 */
export const CLASSIC_START: { view: ClassicView | null; app: string | null } =
  (() => {
    const store = storage();
    const view = store?.getItem(VIEW_KEY);
    const app = store?.getItem(APP_KEY) ?? null;
    store?.removeItem(VIEW_KEY);
    store?.removeItem(APP_KEY);
    return {
      view: CLASSIC_VIEWS.find((candidate) => candidate === view) ?? null,
      app,
    };
  })();

/** 是从旧界面的某一页回来的，不是刚打开客户端：这时不再放一遍开机动画。 */
export const RETURNING: boolean = (() => {
  const store = storage();
  const returning = store?.getItem(RETURN_KEY) === "1";
  store?.removeItem(RETURN_KEY);
  return returning;
})();

export function openClassic(view: ClassicView, app?: string) {
  const store = storage();
  if (!store) return;
  store.setItem(CHOICE_KEY, "classic");
  store.setItem(VIEW_KEY, view);
  if (app) store.setItem(APP_KEY, app);
  window.location.reload();
}

export function openKaifan() {
  storage()?.removeItem(CHOICE_KEY);
  storage()?.setItem(RETURN_KEY, "1");
  window.location.reload();
}
