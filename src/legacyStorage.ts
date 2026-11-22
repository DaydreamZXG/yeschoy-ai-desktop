/**
 * 早先的版本用 `yeschoy` 开头的键名存在这台电脑上的几样小东西，换到现在的键名下面。
 *
 * 必须在别的模块读它们之前跑完，所以它是 `main.tsx` 的第一个 import。
 * 现在的键名下已经有值就不动；旧值抄过去之后才删，抄不过去（存储满了、被禁用）
 * 就留着旧的，下次启动再试。
 */
const RENAMED: ReadonlyArray<readonly [old: string, current: string]> = [
  ["yeschoy-language", "kaifan.language"],
  ["yeschoy.accountLinePreference", "kaifan.accountLinePreference"],
  [
    "yeschoy.account.lastSignedInProjection.v1",
    "kaifan.account.lastSignedInProjection.v1",
  ],
  ["yeschoy.account.sessionAge.v1", "kaifan.account.sessionAge.v1"],
  ["yeschoy.stationConnectOriginDraft", "kaifan.stationConnectOriginDraft"],
  ["yeschoy.announcements.seen.v1", "kaifan.announcements.seen.v1"],
];

export function adoptRenamedStorage(
  storage: Pick<Storage, "getItem" | "setItem" | "removeItem">,
): void {
  for (const [old, current] of RENAMED) {
    try {
      const value = storage.getItem(old);
      if (value === null) continue;
      if (storage.getItem(current) === null) {
        storage.setItem(current, value);
        if (storage.getItem(current) !== value) continue;
      }
      storage.removeItem(old);
    } catch {
      // 存储用不了：什么都不删，下次再试。
    }
  }
}

try {
  adoptRenamedStorage(window.localStorage);
} catch {
  // 连 localStorage 都拿不到（隐私模式之类）：没有东西可搬。
}
