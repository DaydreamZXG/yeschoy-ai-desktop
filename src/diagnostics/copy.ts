const messages = {
  zh: {
    invoke: {
      title: "暂时无法运行检查",
      body: "检查未能完成，请重试。尚未判断线路状态。",
    },
    invalid: {
      title: "暂时无法读取结果",
      body: "未获得可用的检查结果，请重新检查。",
    },
    partial: {
      title: "部分结果未能读取",
      body: "已保留可用的线路结果，可重新检查。",
    },
    unavailable: "本次结果不可用",
    noResult: "未获得本次结果，请重新检查。",
    previous: "上次结果，尚未更新",
    previousCheckedAt: "上次完成于 {{time}}",
  },
  en: {
    invoke: {
      title: "The check could not run",
      body: "Try again. The line status has not been determined.",
    },
    invalid: {
      title: "The result could not be read",
      body: "No usable result was received. Run the check again.",
    },
    partial: {
      title: "Some results could not be read",
      body: "Usable line results are retained. You can check again.",
    },
    unavailable: "Current result unavailable",
    noResult: "No current result was received. Check again.",
    previous: "Previous result, not yet updated",
    previousCheckedAt: "Previously completed at {{time}}",
  },
} as const;

export function diagnosticsCopy(language: string | undefined) {
  return messages[language?.toLowerCase().startsWith("zh") ? "zh" : "en"];
}
