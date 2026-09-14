import { useTranslation } from "react-i18next";

const zh = {
  defaultGroup: "标准分组",
  defaultGroupBillingNote: "已按标准分组计费（{{ratio}}×）",
  chooseGroupLegend: "选择价格方案",
  planPriceUnavailable: "价格待确认",
  planPriceUnit: "每 1 亿 Token 预计费用",
  planSelected: "当前选择",
  planLowest: "价格最低",
  planDetails: "详情",
  planRatio: "计费倍率",
  planDescription: "后台方案说明",
  groupIntro:
    "同一个模型有不同价格方案，选好后按该方案计费。以下费用使用相同的 Token 占比估算。",
  ratioPending: "倍率待查询",
  groupsMissing: "暂未读到这个模型的可用分组，请刷新账户数据。",
  groupsEmptyHint: "选择模型后，可查看对应计费分组。",
  pricesLabel: "所选分组价格",
  estimateSummary: "1 亿 Token 费用参考",
  estimateExampleNote: "费用参考，实际费用随使用情况变化",
  estimateSaving: "约省 {{percent}}%",
  estimateFormula: "约 0.69% 新输入 + 99.14% 缓存读取 + 0.17% 输出",
  estimateOfficialLabel: "使用官网预计",
  estimateYeschoyLabel: "使用野菜预计",
  estimateNote:
    "官网按 {{referenceFx}}、野菜按 {{siteFx}} 换算为人民币，再应用 {{group}} 的倍率{{tiered}}。固定参考汇率，非实时汇率。{{cacheNote}}",
  estimateTieredNote: "；不同请求档位会形成以上区间",
  estimateCacheFallbackNote:
    "该模型没有单独的缓存读取价，缓存部分按输入价保守估算。",
  estimateCacheExcludedNote:
    "不含另行发生的缓存写入，实际费用以请求命中的计费档位为准。",
  estimateUnavailable:
    "当前规则无法可靠换算为 Token 费用，暂不展示估算；实际费用以网站账单为准。",
  perMillionTokens: "每百万 tokens",
  officialPrice: "官网参考价",
  actualPrice: "野菜API实际价",
  inputPrice: "输入",
  outputPrice: "输出",
  priceUnavailable: "暂不可用",
  restoreAction: "恢复原设置",
  revokeAction: "撤销野菜设置",
  restoreFailed:
    "恢复没有完成，现有记录已保留。请关闭目标应用后重试；不会强行覆盖你的修改。",
  tokenCleanupPending:
    "本机设置已恢复。远端专用 Key 暂未撤销——请联网并登录同一账户后，再次点击「{{action}}」即可重试撤销，不会改动已恢复的设置。",
  tokenKept:
    "本机设置已恢复。远端专用 Key 已按你的选择保留：它仍然有效且可能产生计费，可稍后在网站的令牌管理页手动撤销。",
  restoredWithChanges:
    "已恢复可还原的设置，你之后修改的内容已保留。重新打开应用后生效。",
  restoredOriginal: "已恢复接入前的设置。重新打开应用后生效。",
  revokedLegacy:
    "已撤销野菜接入。旧版本没有保存原值，请在应用中选择你要用的账户或服务商。",
  restoreError: "暂时无法恢复，请重试。恢复记录仍保留在这台电脑上。",
  closeLabel: "关闭",
  introOriginal:
    "恢复这个应用接入野菜前的模型、服务商和相关连接设置。你后来修改过的内容会保留。",
  introLegacy:
    "这个接入来自旧版本，没有保存接入前的设置。只能撤销仍属于野菜的连接项，无法找回原来的值。",
  noteNoUninstall: "不会卸载应用，也不会删除聊天记录。",
  noteIsolated: "不影响其他应用、网站账户或余额。",
  noteReopen: "恢复后请重新打开 {{app}}。",
  revokeOptionTitle: "同时撤销此应用的专用 Key",
  revokeOnHint: "撤销后该 Key 立即失效，不再产生任何计费。",
  revokeOffHint:
    "保留后该 Key 仍然有效且可能继续计费；可稍后在网站的令牌管理页手动撤销。",
  cancelRestore: "先不恢复",
  restoring: "正在恢复…",
  sectionLabel: "最近连接结果",
  recentTitle: "最近野菜中转记录",
  refreshResult: "刷新结果",
  outcomeOk: "已确认经野菜中转完成",
  outcomeTimeout: "等待模型回复超时",
  outcomeNetworkError: "未能连接模型服务",
  outcomeUpstreamError: "模型服务未完成请求",
  outcomeInvalidResponse: "模型回复格式异常",
  outcomeStreamInterrupted: "回复在完成前中断",
  outcomeUnknownModel: "这个模型尚未加入常用列表",
  outcomePayloadTooLarge: "请求内容超过本机安全上限",
  outcomeLocalBusy: "本机正在处理另一条大请求",
  noModelSpecified: "未指定模型",
  advicePayloadTooLarge:
    "单次请求超过 200 MiB，未发送到上游。请减少一次附带的文件或图片后重试。",
  adviceLocalBusy:
    "为避免桌面助手卡死，本机一次只缓冲一条大请求。请等待当前请求完成后重试。",
  adviceUnauthorized: "请检查账户与这个模型的使用权限。",
  adviceRateLimited: "请求较多或额度受限，请稍后重试并检查账户。",
  adviceUnknownModel: "请选用已配置的模型，或将新模型加入列表后更新接入。",
  adviceRetry:
    "请先重试；若持续失败，可手动更换线路。不会替你更换模型或计费分组。",
  emptyRequestState:
    "尚未收到该应用经野菜中转的请求。发送一条消息后，可在这里刷新确认。",
  codexAccountNote:
    "Codex 显示的官方账号是登录身份，不是本次模型请求线路或计费方的证明；这里出现中转记录后，才说明请求确实经过了野菜中转。",
  attributionNote:
    "记录来自服务端用量日志，按该工具自己的密钥归因，显示实际转发的完整模型 ID；不依据应用缩写或 AI 的自我介绍判断。发送消息后稍等几秒再刷新即可看到。",
  checking: "正在检查这台电脑…",
  checkAgain: "重新检查应用",
  scanningHint: "正在读取这台电脑上已安装的应用，请稍候。",
  readingConnection: "正在读取接入状态…",
  retryConnectionRead: "重新读取接入状态",
  refreshingAccount: "正在刷新账户…",
  syncingSelection: "正在同步选择…",
  updateConnection: "更新接入设置",
  installAndConnect: "安装并接入",
  applyRunningHint: "接入正在进行，完成或安全取消后会自动恢复操作。",
  targetScanRetryHint:
    "这次没有完成本机应用检查。点击按钮重新检查，不会修改任何设置。",
  connectionReadRetryHint:
    "没有读到上次的接入状态。点击按钮重新读取，不会覆盖现有设置。",
  connectionReadingHint: "正在读取这台电脑上的现有接入状态，请稍候。",
  accountRefreshingHint: "正在刷新账户、模型和计费分组，请稍候。",
  selectionSyncHint: "正在同步当前账户的模型选择，请稍候。",
  installed: "已找到 · 可接入",
  installedShort: "已安装",
  chooseInstall: "选择要使用的安装位置",
  chooseInstallHint: "发现了多个安装。助手已优先选中可用版本，你也可以更换。",
  missing: "未在这台电脑找到该应用，请先安装后重新检查。",
  unsupported:
    "找到了应用，但缺少启动所需的组件。请确认应用安装完整后重新检查。",
  scanFailed: "暂时无法检查本机应用，请重新检查。",
  unavailable: "等待检查",
  version: "版本",
  verifying: "正在安全保存设置并启动本地连接，不会发送测试消息…",
  verifyingCodex:
    "正在安全保存 Codex 设置与密钥，并准备本地路由，完成后会自动打开应用…",
  verifyingDesktop:
    "正在安全保存 Claude Desktop 设置并准备本地连接，不会等待模型回复。",
  verifyingDsh: "正在保存 DSH 设置并启动本地工作台，不会发送测试消息…",
  readyTitle: "接入完成",
  firstActivationTitle: "第一次接入成功",
  applySucceeded: "接入成功",
  readyBody:
    "{{app}} 的设置已保存，本地连接已就绪。首次使用后的真实结果会显示在这里。",
  selectInstallFirst: "先选择安装位置",
  installFirst: "请先安装应用",
  updateFirst: "缺少运行组件",
  connectionFailed: "没有完成接入，所有本机改动已恢复。请重新检查后再试。",
  credentialHelperFailed:
    "Codex 无法从系统安全存储读取工具密钥，设置已恢复。请退出后重新打开野菜 API 再试。",
  authenticationFailed:
    "所选线路没有接受工具密钥，设置已恢复。请刷新账户后重试。",
  endpointUnavailable:
    "所选线路暂时无法使用这个模型接口，设置已恢复。可以换一条线路或稍后重试。",
  providerTimedOut:
    "所选线路响应超时，设置已恢复。可以换一条线路或稍后重试。",
  providerBusy: "当前模型请求较多，设置已恢复。请稍后重试或选择其他模型。",
  modelRequestRejected:
    "所选模型没有接受测试请求，设置已恢复。请刷新模型列表后重新选择。",
  invalidProviderResponse:
    "线路返回了无法识别的模型回复，设置已恢复。请稍后重试。",
  desktopTimedOut:
    "没有收到 Claude Desktop 的测试消息，接入未确认，本机改动已恢复。",
  missingDuringSetup: "刚才选择的应用已找不到，请重新检查。",
  selectionRequired: "发现多个安装，请明确选择要使用的一个。",
  secureStoreFailed:
    "系统安全存储暂时不可用。请解锁钥匙串或凭据管理器，并检查本机接入状态后重试。",
  externalOverride:
    "检测到系统环境变量（如 ANTHROPIC_BASE_URL 等）的优先级高于助手写入的设置，本次未改动原设置。请检查并移除相关环境变量后重试。",
  unsupportedProfile: "这个应用的运行方式暂不能自动配置，原设置没有改动。",
  launchFailed:
    "设置已经恢复，因为应用未能正常启动。请确认应用可以手动打开。",
  builderKicker: "模型与计费分组",
  builderTitle: "选好，就能用",
  builderIntro: "选模型、比较分组价格，再一键完成接入。网络线路单独选择。",
  modelChoice: "选择模型",
  modelQuestion: "想用哪个 AI？",
  lineChoice: "选择连接线路",
  lineQuestion: "按你所在的位置选择，价格不会因此改变",
  yeschoyPrice: "野菜 API 价",
  saveInputOutput: "输入省 {{input}} · 输出省 {{output}}",
  finishChoice: "完成接入",
  finishHint: "安全写入并读回应用设置；不会发送收费的测试消息。",
  connectionDetails: "查看连接详情",
  endpointReferenceNote: "文档参考值，实际以写入应用设置的端点为准。",
  directConnection: "直接连接",
  automaticCompatibility: "自动兼容",
  surfaceClaudeCode: "命令行与编辑器工作区",
  surfaceClaudeDesktop: "Claude 桌面应用",
  surfaceCodexDesktop: "ChatGPT 桌面应用中的 Codex",
  surfacePi: "Pi 编程助手",
  surfaceDsh: "DeepSeek Harness 浏览器工作台",
  lifecycleDesktopRestart:
    "若 {{name}} 正在运行，更新接入时会先提醒你保存；确认后由助手先请求应用正常退出，写入设置并重新打开。Windows 若只剩后台进程，会仅结束这个安装路径对应的进程。",
  lifecycleBrowserLaunch:
    "更新接入会保存 DSH 配置；“打开使用”只会启动本地服务并在浏览器中打开，不会发送模型测试消息。",
  lifecycleTerminalSession:
    "更新接入不会关闭正在使用的命令行会话；新设置从新开的终端会话生效。",
  retryRollbackFailed:
    "自动恢复上次未完成的设置时遇到问题，部分设置尚未恢复。可直接点击“自动修复并重试”，无需手动修改配置。",
  retryCredentialRestore:
    "自动恢复时密钥设置尚未恢复。请解锁系统钥匙串或凭据管理器，再点击“自动修复并重试”，无需手动修改配置。",
  retryRecoveryPending:
    "另一项接入或恢复操作正在进行。本次没有修改应用，请稍后直接重试；若一直出现，可使用“恢复原设置”。",
  readyTerminal:
    "{{app}} 的设置和本地连接已经就绪。正在运行的命令行会话不会被中断；请新开一个会话，或点击“打开终端使用”。第一次真实请求的结果会显示在“最近连接结果”里。",
  readyCodex:
    "{{app}} 的设置和野菜本地路由已经就绪。{{favorites}}Codex 仍可显示你的官方登录账号，那只是登录身份，不代表模型请求走官方计费。第一次真实请求的结果会显示在“最近野菜中转记录”里；看到完整模型 ID，才表示这次请求确实经过野菜中转。",
  favoritesConfigured: "常用模型已一起配置。",
  readyDefault:
    "{{app}} 的设置和本地连接已经就绪。{{favorites}}请在应用中正常使用；第一次真实请求的结果会显示在“最近连接结果”里。",
  restartBlocked:
    "系统未能关闭 {{app}}，本次没有修改设置。请确认没有系统弹窗拦截，或手动退出后重试。",
  appRunning:
    "{{app}} 正在运行。请先保存未完成内容，再确认由助手关闭并重新打开。Windows 若只剩后台进程，只会结束已识别安装路径对应的进程。",
  unsupportedGroup: "所选分组已不可用，请刷新账户数据后重新选择。",
  launchStateUnavailable:
    "暂时无法安全确认应用是否正在运行，本次没有修改设置。请手动退出应用后再试。",
  launchAccessDenied:
    "设置已经保存，但系统阻止了自动打开。请从系统菜单手动打开应用。",
  launchTargetChanged:
    "设置已经保存，但应用安装位置刚刚发生变化。请重新检查应用后手动打开。",
  launchStoreApp:
    "设置已经保存，但暂时无法自动打开这个商店应用。请先从开始菜单打开一次。",
  launchNotOpened:
    "设置已经保存，但没有自动打开应用。请手动打开；不需要重新接入。",
  verifyStartFailed:
    "应用没有启动成功，接入设置已恢复。请确认应用安装完整且可以手动打开。",
  verifyTimedOut: "应用的连接测试超时，接入设置已恢复。请稍后重试。",
  verifyReadFailed:
    "未能读取应用的测试结果，接入设置已恢复。请重新打开助手后再试。",
  verifyInvalidReply:
    "应用没有完成有效的模型回复，接入设置已恢复。请检查所选模型与分组后再试。",
  bridgeUnavailable:
    "野菜本机网关没有在限定时间内就绪，接入设置已恢复。请重新打开野菜助手后再试。",
  bridgeAuthFailed:
    "野菜本机网关的安全令牌不一致，接入设置已恢复。请重新接入，助手会自动生成新令牌。",
  bridgeCatalogInvalid:
    "野菜本机网关没有加载完整模型列表，接入设置已恢复。请重新检查模型后再试。",
  installConfirmRequired:
    "安装已完成，但当前选择需要重新确认。请确认账户和模型后再点接入；这次没有改动应用设置。",
  assistantShuttingDown:
    "正在退出助手，本次接入已停止；已写入的设置会先恢复。",
  activationCancelled:
    "已取消本次接入；如果设置写入已经开始，助手已先恢复原设置。",
  activationTimedOut:
    "接入等待超过 90 秒，页面已恢复操作并通知后台安全取消。请先查看接入状态；若仍显示处理中，请等待片刻后再试，不要连续重复提交。",
  activationAlreadyRunning:
    "已有一项接入正在安全收尾，请稍等片刻后再试；本次没有重复提交。",
  accountChanged: "账户已切换，本次接入已停止。请确认当前账户后重新接入。",
  invalidResponseResult:
    "暂时无法确认接入结果。请先检查本机接入状态，避免连续重复提交。",
  recoveryStorageUnavailable:
    "暂时无法安全保存原设置，这次没有修改应用。请确认系统钥匙串或凭据管理器可用后重试。",
  recoveryReceiptFailed:
    "接入记录保存未完成，请检查本地状态并恢复后重试。",
  progressQueued: "正在等待安全配置锁…",
  progressCheckingApp: "正在检查应用状态和原设置…",
  progressAuthenticating: "正在确认账户和线路…",
  progressCheckingModels: "正在核对模型与计费分组…",
  progressSecuringAccess: "正在创建仅限所选模型的应用密钥…",
  progressPreparingSettings: "正在准备可恢复的配置…",
  progressApplyingSettings: "正在安全写入并复核设置…",
  progressRestoring: "操作未完成，正在恢复原设置…",
  progressComplete: "接入完成。",
  versionUnreadNote: "版本未读取，不影响尝试接入",
  signInHint: "登录后即可扫描本机应用并一键接入。",
  refreshAccountLabel: "重新获取账户数据",
  refreshAccountHint:
    "账户数据没有更新成功，价格与分组可能过期。点这里重新获取。",
  installHint: "本机还没有这个应用，助手会先安装再接入。",
  installUnavailableHint:
    "本机没有检测到这个应用。安装后点这里重新检查，不会修改任何设置。",
  updateFirstHint: "检测到的版本暂不支持接入。更新应用后点这里重新检查。",
  selectInstallationHint: "这台电脑上有多个安装位置，请先在上面选择一个。",
  chooseModelFirst: "先选择模型",
  chooseGroupFirst: "先选择计费分组",
  checkModelsLabel: "检查常用模型与分组",
  restartAppLabel: "关闭并重新打开 {{app}}",
  restartAppHint:
    "设置还没有写入。这个应用正在运行，点这里确认已保存后由助手关闭并重新打开。",
  viewAccountRelogin: "查看账户并重新登录",
  refreshModelsGroups: "刷新模型与分组",
  viewOtherLines: "查看其他线路",
  recheckApp: "重新检查应用和接入状态",
  settingUpFor: "正在为这个应用设置",
  collapseApps: "收起应用",
  switchApp: "更换应用",
  showAllApps: "查看全部支持的应用",
  installationSummary: "安装与接入信息",
  autoSelectedSuffix: " · 已自动选择",
  versionNotRead: "版本未读取",
  accountStaleTitle: "账户数据暂未更新",
  accountStaleBody:
    "这里保留的是上次的模型与价格。刷新成功后再接入；已接入的应用仍可从“我的应用”打开。",
  refreshAccountData: "刷新账户数据",
  nativeInterface: "原生接口",
  missingModelPrefix: "之前选择的",
  missingModelSuffix: "当前不可用。请重新选择模型，不会自动替换。",
  billingCardHint: "选择价格与来源，不改变网络线路",
  missingGroupTitle: "之前的计费分组已不可用",
  missingGroupBody:
    "不在这个模型当前可用的分组中。请在上方重新选择，价格可能不同；不会自动切换。",
  favoriteModels: "常用模型",
  favoriteModelsIntro: "加入你会用的模型，每个模型单独选择计费分组。",
  updateModelGroup: "更新这个模型的分组",
  addFavoriteModel: "加入常用模型",
  defaultModelAria: "默认模型 {{model}}",
  removeModelAria: "移除 {{model}}",
  bindingUnavailable: "当前不可用，请重新选择或移除",
  defaultBadge: "默认",
  removeAction: "移除",
  favoriteModelsEmpty: "也可以只接入上面选中的一个模型，之后再添加。",
  pendingModelEditNote: "上方的选择尚未加入列表。点击“{{action}}”后，再确认接入。",
  favoriteModelsNote:
    "同一模型保留一个计费分组。已有列表里的模型可在应用内切换；新增模型、修改默认模型或分组后，需要更新接入。",
  networkLinePrefix: "网络线路",
  changeLine: "更换线路",
  lineIntro:
    "大陆优化优先适合中国大陆网络；全球加速使用 Cloudflare，海外可优先尝试。线路只影响连接，不改变计费分组与倍率。",
  currentConnection: "当前接入",
  configuredSummary:
    "这些设置已经保存。日常换模型可在应用内选择；修改常用列表后，再更新接入。",
  billingGroupLabel: "计费分组",
  selectGroupFirst: "请选择分组",
  favoriteModelsCount: "{{count}} 个常用模型",
  modelSetNote:
    "接入时安全配置全部模型；每个模型的真实连接结果在首次使用后显示。",
  lifecycleNotePrefix: "打开使用不会改设置。",
  backgroundNote:
    "使用时请保持野菜助手运行。关闭窗口可选择继续后台运行；完全退出会中断模型连接，不会锁定你的应用，随时可以恢复原设置。",
  progressStep: "第 {{done}} / {{total}} 步",
  progressNote:
    "进度会显示在这里；页面其他区域仍可使用，请不要重复点击接入按钮。",
  cancelInProgress: "正在安全取消…",
  cancelSetup: "取消本次接入",
  feedbackStaleTitle: "刚才的接入已完成",
  feedbackReopenTitle: "需要重新打开应用",
  feedbackSavedTitle: "设置已保存",
  staleContextPrefix: "刚才处理的是账户",
  staleContextSuffix: "。现在的选择尚未应用。",
  restartDialogTitle: "保存后自动重新打开 {{app}}",
  restartDialogMessage:
    "这个应用正在运行。请先保存正在编辑的内容。\n\n继续后，你不需要手动退出：野菜助手会先请求它正常退出，设置完成后再重新打开。Windows 若只剩后台进程，会仅结束这个已识别安装路径对应的进程；不会按名称结束其他程序。若系统仍阻止退出，本次不会修改设置。",
  restartDialogConfirm: "已保存，退出并继续",
  restartDialogCancel: "暂不接入",
};
type Copy = { [K in keyof typeof zh]: string };
const en: Copy = {
  defaultGroup: "Default group",
  defaultGroupBillingNote: "Billed at the default group rate ({{ratio}}×)",
  chooseGroupLegend: "Choose a pricing plan",
  planPriceUnavailable: "Price pending",
  planPriceUnit: "Estimated cost per 100M tokens",
  planSelected: "Selected",
  planLowest: "Lowest price",
  planDetails: "Details",
  planRatio: "Billing multiplier",
  planDescription: "Provider plan description",
  groupIntro:
    "Choose a pricing plan for this model. Estimates below use the same token mix.",
  ratioPending: "Ratio pending",
  groupsMissing:
    "No available groups for this model were read. Refresh your account data.",
  groupsEmptyHint: "Choose a model to see its billing groups.",
  pricesLabel: "Prices for the selected group",
  estimateSummary: "Cost reference for 100M tokens",
  estimateExampleNote: "Cost reference; actual costs vary with usage",
  estimateSaving: "Save about {{percent}}%",
  estimateFormula: "Approx. 0.69% new input + 99.14% cache reads + 0.17% output",
  estimateOfficialLabel: "Official estimate",
  estimateYeschoyLabel: "野菜API estimate",
  estimateNote:
    "Converted to CNY at {{referenceFx}} for the official price and {{siteFx}} for 野菜API, then applying {{group}}'s ratio{{tiered}}. Fixed reference rates, not live FX. {{cacheNote}}",
  estimateTieredNote: "; different request tiers form the range above",
  estimateCacheFallbackNote:
    "This model has no separate cache-read price, so the cache portion is conservatively estimated at the input price.",
  estimateCacheExcludedNote:
    "Cache writes billed separately are not included; actual charges follow the billing tier each request lands in.",
  estimateUnavailable:
    "The current rules cannot reliably convert this into a token cost, so no estimate is shown; actual charges follow the website's bills.",
  perMillionTokens: "per million tokens",
  officialPrice: "Official reference",
  actualPrice: "Your 野菜API price",
  inputPrice: "Input",
  outputPrice: "Output",
  priceUnavailable: "Unavailable",
  restoreAction: "Restore original settings",
  revokeAction: "Undo 野菜 settings",
  restoreFailed:
    "The restore did not finish; the existing record has been kept. Close the target app and try again — your changes will not be overwritten.",
  tokenCleanupPending:
    "Local settings are restored. The dedicated remote key has not been revoked yet — get online, sign in with the same account, and click “{{action}}” again to retry the revocation. Your restored settings will not be touched.",
  tokenKept:
    "Local settings are restored. The dedicated remote key was kept as you chose: it stays valid and may still incur charges; you can revoke it later on the website's token management page.",
  restoredWithChanges:
    "Restorable settings have been restored; the changes you made afterwards are kept. They take effect after you reopen the app.",
  restoredOriginal:
    "The pre-connection settings have been restored. They take effect after you reopen the app.",
  revokedLegacy:
    "The 野菜 connection has been undone. The old version did not save the original values, so choose the account or provider you want in the app.",
  restoreError:
    "Could not restore right now, please try again. The restore record is still kept on this computer.",
  closeLabel: "Close",
  introOriginal:
    "Restore this app's model, provider and related connection settings from before it was connected to 野菜. Changes you made afterwards are kept.",
  introLegacy:
    "This connection was made by an older version, which did not save the pre-connection settings. Only entries still belonging to 野菜 can be undone; the original values cannot be recovered.",
  noteNoUninstall:
    "The app will not be uninstalled, and chat history will not be deleted.",
  noteIsolated:
    "Other apps, your website account and your balance are not affected.",
  noteReopen: "Reopen {{app}} after the restore.",
  revokeOptionTitle: "Also revoke this app's dedicated key",
  revokeOnHint:
    "Once revoked, the key stops working immediately and no further charges can occur.",
  revokeOffHint:
    "If kept, the key stays valid and may keep incurring charges; you can revoke it later on the website's token management page.",
  cancelRestore: "Not now",
  restoring: "Restoring…",
  sectionLabel: "Latest connection result",
  recentTitle: "Recent 野菜 relay records",
  refreshResult: "Refresh result",
  outcomeOk: "Confirmed completed via the 野菜 relay",
  outcomeTimeout: "Timed out waiting for the model's reply",
  outcomeNetworkError: "Could not reach the model service",
  outcomeUpstreamError: "The model service did not complete the request",
  outcomeInvalidResponse: "The model's reply was malformed",
  outcomeStreamInterrupted: "The reply was interrupted before it finished",
  outcomeUnknownModel: "This model has not been added to the frequently used list yet",
  outcomePayloadTooLarge: "The request exceeds this computer's safety limit",
  outcomeLocalBusy: "This computer is processing another large request",
  noModelSpecified: "No model specified",
  advicePayloadTooLarge:
    "A single request exceeded 200 MiB and was not sent upstream. Reduce the files or images attached at once, then retry.",
  adviceLocalBusy:
    "To keep the desktop assistant from freezing, this computer buffers only one large request at a time. Wait for the current request to finish, then retry.",
  adviceUnauthorized: "Check your account and your access to this model.",
  adviceRateLimited:
    "Too many requests, or your quota is limited. Retry later and check your account.",
  adviceUnknownModel:
    "Choose a configured model, or add the new model to the list and update the connection.",
  adviceRetry:
    "Retry first; if it keeps failing, you can switch the connection line manually. Your model and billing group will not be changed for you.",
  emptyRequestState:
    "No requests from this app through the 野菜 relay yet. Send a message, then refresh here to confirm.",
  codexAccountNote:
    "The official account shown in Codex is your sign-in identity, not proof of which line or billing party handled this model request; only when a relay record appears here is the request confirmed to have gone through 野菜.",
  attributionNote:
    "Records come from server-side usage logs, attributed by each tool's own key, and show the full model ID actually forwarded; they are not based on app abbreviations or an AI's self-description. Send a message, wait a few seconds, then refresh to see it.",
  checking: "Checking this computer…",
  checkAgain: "Check applications again",
  scanningHint:
    "Reading the applications installed on this computer. Please wait.",
  readingConnection: "Reading connection status…",
  retryConnectionRead: "Read connection status again",
  refreshingAccount: "Refreshing account…",
  syncingSelection: "Syncing selection…",
  updateConnection: "Update connection settings",
  installAndConnect: "Install and connect",
  applyRunningHint:
    "Setup is in progress. Actions recover automatically after completion or safe cancellation.",
  targetScanRetryHint:
    "The application check did not finish. Check again without changing any settings.",
  connectionReadRetryHint:
    "Previous connection status could not be read. Read it again without overwriting settings.",
  connectionReadingHint:
    "Reading existing connection status on this computer. Please wait.",
  accountRefreshingHint:
    "Refreshing the account, models, and billing groups. Please wait.",
  selectionSyncHint:
    "Syncing the model selection for this account. Please wait.",
  installed: "Found · Ready to configure",
  installedShort: "Installed",
  chooseInstall: "Choose the installation to use",
  chooseInstallHint:
    "More than one installation was found. A supported one is selected when possible.",
  missing: "This application was not found. Install it, then check again.",
  unsupported:
    "The application was found, but a required runtime component is missing.",
  scanFailed: "Applications could not be checked. Try again.",
  unavailable: "Waiting for check",
  version: "Version",
  verifying:
    "Saving settings securely and starting the local connection without sending a test prompt…",
  verifyingCodex:
    "Checking Codex settings, secure credentials, and the local route, then opening the app…",
  verifyingDesktop:
    "Saving Claude Desktop settings and preparing its local connection without waiting for a model reply.",
  verifyingDsh:
    "Saving DSH settings and starting its local workspace without sending a test prompt…",
  readyTitle: "Connection complete",
  firstActivationTitle: "First connection complete",
  applySucceeded: "Connected",
  readyBody:
    "{{app}} settings are saved and the local connection is ready. The first real-use result will appear here.",
  selectInstallFirst: "Choose an installation first",
  installFirst: "Install the application first",
  updateFirst: "Runtime component missing",
  connectionFailed:
    "Setup did not complete. Local changes were restored. Check again and retry.",
  credentialHelperFailed:
    "Codex could not read the tool key from secure system storage. Settings were restored. Reopen Yeschoy API and retry.",
  authenticationFailed:
    "The selected line did not accept the tool key. Settings were restored. Refresh the account and retry.",
  endpointUnavailable:
    "This model endpoint is unavailable on the selected line. Settings were restored. Try the other line or retry later.",
  providerTimedOut:
    "The selected line timed out. Settings were restored. Try the other line or retry later.",
  providerBusy:
    "The selected model is busy. Settings were restored. Retry later or choose another model.",
  modelRequestRejected:
    "The selected model rejected the verification request. Settings were restored. Refresh models and choose again.",
  invalidProviderResponse:
    "The line returned an unrecognized model response. Settings were restored. Retry later.",
  desktopTimedOut:
    "No test message arrived from Claude Desktop. The connection was not confirmed and local changes were restored.",
  missingDuringSetup:
    "The selected application is no longer available. Check again.",
  selectionRequired:
    "More than one installation was found. Choose exactly one.",
  secureStoreFailed:
    "Secure system storage is unavailable. Unlock the keychain or credential manager, check this computer's connection status, then retry.",
  externalOverride:
    "System environment variables (such as ANTHROPIC_BASE_URL) are taking priority over the settings written by the assistant. Existing settings were not changed. Check and remove the relevant environment variables, then retry.",
  unsupportedProfile:
    "This app cannot be configured automatically on the current system. Existing settings were not changed.",
  launchFailed:
    "Settings were restored because the application could not start. Make sure it opens normally.",
  builderKicker: "Model and billing groups",
  builderTitle: "Choose it, then use it",
  builderIntro:
    "Choose a model, compare group prices, then finish setup in one click. The network line is chosen separately.",
  modelChoice: "Choose a model",
  modelQuestion: "Which AI do you want to use?",
  lineChoice: "Choose a connection line",
  lineQuestion: "Choose for your location. Pricing stays the same.",
  yeschoyPrice: "野菜 API price",
  saveInputOutput: "Save {{input}} on input · {{output}} on output",
  finishChoice: "Finish setup",
  finishHint:
    "Write and read back settings safely without sending a billable test message.",
  connectionDetails: "View connection details",
  endpointReferenceNote:
    "Documented reference value; the endpoint actually written into the app's settings prevails.",
  directConnection: "Direct connection",
  automaticCompatibility: "Automatic compatibility",
  surfaceClaudeCode: "Command line and editor workspace",
  surfaceClaudeDesktop: "Claude desktop app",
  surfaceCodexDesktop: "Codex in the ChatGPT desktop app",
  surfacePi: "Pi coding assistant",
  surfaceDsh: "DeepSeek Harness browser workbench",
  lifecycleDesktopRestart:
    "If {{name}} is running, you will be reminded to save before the connection update. After confirmation, the assistant asks the app to quit normally, writes the settings, and reopens it. On Windows, if only background processes remain, only the processes belonging to this installation path are terminated.",
  lifecycleBrowserLaunch:
    "Updating the connection saves the DSH configuration; “Open” only starts the local service and opens it in the browser without sending any model test messages.",
  lifecycleTerminalSession:
    "Updating the connection does not close command-line sessions in use; new settings take effect in newly opened terminal sessions.",
  retryRollbackFailed:
    "A problem occurred while automatically restoring the last unfinished settings; some settings have not been restored yet. Click “Repair automatically and retry” directly — no manual configuration edits are needed.",
  retryCredentialRestore:
    "The key settings were not restored during automatic recovery. Unlock the system keychain or credential manager, then click “Repair automatically and retry” — no manual configuration edits are needed.",
  retryRecoveryPending:
    "Another connection or restore operation is in progress. No application settings were changed this time; retry shortly. If this keeps happening, use “Restore original settings”.",
  readyTerminal:
    "{{app}} settings and the local connection are ready. Running command-line sessions are not interrupted; open a new session, or click “Open in terminal”. The first real request's result will appear under “Latest connection result”.",
  readyCodex:
    "{{app}} settings and the 野菜 local route are ready. {{favorites}}Codex can still show your official sign-in account; that is only your sign-in identity, not proof that model requests are billed officially. The first real request's result will appear under “Recent 野菜 relay records”; only when the full model ID appears is the request confirmed to have gone through the 野菜 relay.",
  favoritesConfigured: "Frequently used models were configured together.",
  readyDefault:
    "{{app}} settings and the local connection are ready. {{favorites}}Use the app as usual; the first real request's result will appear under “Latest connection result”.",
  restartBlocked:
    "The system could not close {{app}}; no settings were changed this time. Make sure no system dialog is blocking it, or quit it manually and retry.",
  appRunning:
    "{{app}} is running. Save your unfinished work first, then confirm that the assistant closes and reopens it. On Windows, if only background processes remain, only the processes belonging to the recognized installation path are terminated.",
  unsupportedGroup:
    "The selected group is no longer available. Refresh the account data and choose again.",
  launchStateUnavailable:
    "The system could not safely confirm whether the app is running; no settings were changed this time. Quit the app manually and retry.",
  launchAccessDenied:
    "Settings were saved, but the system blocked the automatic launch. Open the app manually from the system menu.",
  launchTargetChanged:
    "Settings were saved, but the app's installation location just changed. Check the app again and open it manually.",
  launchStoreApp:
    "Settings were saved, but this store app could not be opened automatically yet. Open it once from the Start menu first.",
  launchNotOpened:
    "Settings were saved, but the app was not opened automatically. Open it manually; no reconnection is needed.",
  verifyStartFailed:
    "The app did not start successfully; the connection settings were restored. Make sure the app is installed completely and opens manually.",
  verifyTimedOut:
    "The app's connection test timed out; the connection settings were restored. Retry later.",
  verifyReadFailed:
    "The app's test result could not be read; the connection settings were restored. Reopen the assistant and retry.",
  verifyInvalidReply:
    "The app did not produce a valid model reply; the connection settings were restored. Check the selected model and group, then retry.",
  bridgeUnavailable:
    "The 野菜 local gateway was not ready in time; the connection settings were restored. Reopen the 野菜 assistant and retry.",
  bridgeAuthFailed:
    "The 野菜 local gateway's security token did not match; the connection settings were restored. Reconnect and the assistant will generate a new token automatically.",
  bridgeCatalogInvalid:
    "The 野菜 local gateway did not load the full model list; the connection settings were restored. Check the models again and retry.",
  installConfirmRequired:
    "Installation finished, but the current selection needs to be confirmed again. Confirm the account and models, then connect; application settings were not changed this time.",
  assistantShuttingDown:
    "The assistant is quitting; this setup was stopped. Any settings already written will be restored first.",
  activationCancelled:
    "This setup was cancelled; if writing had already started, the assistant restored the original settings first.",
  activationTimedOut:
    "Setup waited longer than 90 seconds. The page is responsive again and the background has been asked to cancel safely. Check the connection status first; if it still shows in progress, wait a moment before retrying instead of submitting repeatedly.",
  activationAlreadyRunning:
    "Another setup is finishing safely. Please wait a moment and retry; nothing was submitted twice.",
  accountChanged:
    "The account has switched and this setup was stopped. Confirm the current account and reconnect.",
  invalidResponseResult:
    "The setup result cannot be confirmed right now. Check the local connection status first and avoid repeated submissions.",
  recoveryStorageUnavailable:
    "The original settings could not be saved securely; the app was not modified this time. Make sure the system keychain or credential manager is available, then retry.",
  recoveryReceiptFailed:
    "Saving the connection record did not finish. Check the local status, restore, then retry.",
  progressQueued: "Waiting for the secure configuration lock…",
  progressCheckingApp: "Checking application status and original settings…",
  progressAuthenticating: "Confirming the account and line…",
  progressCheckingModels: "Verifying models and billing groups…",
  progressSecuringAccess:
    "Creating an application key limited to the selected models…",
  progressPreparingSettings: "Preparing a restorable configuration…",
  progressApplyingSettings: "Writing and verifying settings securely…",
  progressRestoring:
    "The operation did not finish; restoring the original settings…",
  progressComplete: "Setup complete.",
  versionUnreadNote: "version not read; setup can still proceed",
  signInHint:
    "Sign in to scan this computer's apps and connect in one click.",
  refreshAccountLabel: "Fetch account data again",
  refreshAccountHint:
    "Account data did not update; prices and groups may be stale. Click to fetch it again.",
  installHint:
    "This app is not on this computer yet; the assistant installs it first, then connects.",
  installUnavailableHint:
    "This app was not detected. Install it, then check again here — no settings are changed.",
  updateFirstHint:
    "The detected version does not support connection yet. Update the app, then check again here.",
  selectInstallationHint:
    "This computer has multiple installation locations. Choose one above first.",
  chooseModelFirst: "Choose a model first",
  chooseGroupFirst: "Choose a billing group first",
  checkModelsLabel: "Check frequently used models and groups",
  restartAppLabel: "Close and reopen {{app}}",
  restartAppHint:
    "Settings have not been written yet. This app is running; click here to confirm you have saved, then let the assistant close and reopen it.",
  viewAccountRelogin: "View account and sign in again",
  refreshModelsGroups: "Refresh models and groups",
  viewOtherLines: "View other lines",
  recheckApp: "Recheck the app and connection status",
  settingUpFor: "Setting up",
  collapseApps: "Collapse apps",
  switchApp: "Change app",
  showAllApps: "View all supported apps",
  installationSummary: "Installation and connection info",
  autoSelectedSuffix: " · Auto-selected",
  versionNotRead: "version not read",
  accountStaleTitle: "Account data is not up to date",
  accountStaleBody:
    "Models and prices from last time are kept here. Connect after a successful refresh; connected apps can still be opened from “My apps”.",
  refreshAccountData: "Refresh account data",
  nativeInterface: "Native API",
  missingModelPrefix: "The previously selected",
  missingModelSuffix:
    "is currently unavailable. Choose a model again; it will not be replaced automatically.",
  billingCardHint: "Choose price and source; the network line stays unchanged",
  missingGroupTitle: "The previous billing group is no longer available",
  missingGroupBody:
    "is not among this model's currently available groups. Choose again above; the price may differ. It will not be switched automatically.",
  favoriteModels: "Frequently used models",
  favoriteModelsIntro:
    "Add the models you will use; each model has its own billing group.",
  updateModelGroup: "Update this model's group",
  addFavoriteModel: "Add to frequently used",
  defaultModelAria: "Default model {{model}}",
  removeModelAria: "Remove {{model}}",
  bindingUnavailable: "currently unavailable; choose again or remove",
  defaultBadge: "Default",
  removeAction: "Remove",
  favoriteModelsEmpty:
    "You can also connect just the one model selected above and add more later.",
  pendingModelEditNote:
    "The selection above has not been added to the list yet. Click “{{action}}”, then confirm the connection.",
  favoriteModelsNote:
    "Each model keeps one billing group. Models already in the list can be switched inside the app; after adding models, changing the default model, or changing a group, update the connection.",
  networkLinePrefix: "Network line",
  changeLine: "Change line",
  lineIntro:
    "Mainland Optimized is best for mainland China networks; Global Accelerated uses Cloudflare and is worth trying first overseas. The line only affects connectivity, not billing groups or ratios.",
  currentConnection: "Current connection",
  configuredSummary:
    "These settings are saved. For day-to-day model switches, choose inside the app; after changing the frequently used list, update the connection.",
  billingGroupLabel: "Billing group",
  selectGroupFirst: "Choose a group",
  favoriteModelsCount: "{{count}} frequently used models",
  modelSetNote:
    "All models are configured securely during connection; each model's real connection result appears after first use.",
  lifecycleNotePrefix: "Opening for use does not change settings.",
  backgroundNote:
    "Keep the 野菜 assistant running while using it. Closing the window can continue in the background; quitting entirely interrupts model connections but does not lock your app, and you can restore the original settings at any time.",
  progressStep: "Step {{done}} / {{total}}",
  progressNote:
    "Progress appears here; the rest of the page stays usable. Please don't click the connect button repeatedly.",
  cancelInProgress: "Cancelling safely…",
  cancelSetup: "Cancel this setup",
  feedbackStaleTitle: "The last setup is complete",
  feedbackReopenTitle: "The app needs to be reopened",
  feedbackSavedTitle: "Settings saved",
  staleContextPrefix: "The last action was for account",
  staleContextSuffix: ". The current selection has not been applied.",
  restartDialogTitle: "Save, then reopen {{app}} automatically",
  restartDialogMessage:
    "This app is running. Save what you are editing first.\n\nAfter continuing, you don't need to quit manually: the 野菜 assistant asks it to quit normally first, then reopens it once the settings are written. On Windows, if only background processes remain, only the processes belonging to this recognized installation path are terminated; no other programs are terminated by name. If the system still blocks the exit, no settings are changed this time.",
  restartDialogConfirm: "Saved — quit and continue",
  restartDialogCancel: "Not now",
};
const tw: Copy = {
  defaultGroup: "標準分組",
  defaultGroupBillingNote: "已按標準分組計費（{{ratio}}×）",
  chooseGroupLegend: "選擇價格方案",
  planPriceUnavailable: "價格待確認",
  planPriceUnit: "每 1 億 Token 預計費用",
  planSelected: "目前選擇",
  planLowest: "價格最低",
  planDetails: "詳情",
  planRatio: "計費倍率",
  planDescription: "後台方案說明",
  groupIntro:
    "同一個模型有不同價格方案，選好後按該方案計費。以下費用使用相同的 Token 占比估算。",
  ratioPending: "倍率待查詢",
  groupsMissing: "暫未讀到這個模型的可用分組，請重新整理帳戶資料。",
  groupsEmptyHint: "選擇模型後，可查看對應計費分組。",
  pricesLabel: "所選分組價格",
  estimateSummary: "1 億 Token 費用參考",
  estimateExampleNote: "費用參考，實際費用隨使用情況變化",
  estimateSaving: "約省 {{percent}}%",
  estimateFormula: "約 0.69% 新輸入 + 99.14% 快取讀取 + 0.17% 輸出",
  estimateOfficialLabel: "使用官網預計",
  estimateYeschoyLabel: "使用野菜預計",
  estimateNote:
    "官網按 {{referenceFx}}、野菜按 {{siteFx}} 換算為人民幣，再套用 {{group}} 的倍率{{tiered}}。固定參考匯率，非即時匯率。{{cacheNote}}",
  estimateTieredNote: "；不同請求檔位會形成以上區間",
  estimateCacheFallbackNote:
    "該模型沒有單獨的快取讀取價，快取部分按輸入價保守估算。",
  estimateCacheExcludedNote:
    "不含另行發生的快取寫入，實際費用以請求命中的計費檔位為準。",
  estimateUnavailable:
    "目前規則無法可靠換算為 Token 費用，暫不顯示估算；實際費用以網站帳單為準。",
  perMillionTokens: "每百萬 tokens",
  officialPrice: "官網參考價",
  actualPrice: "野菜API實際價",
  inputPrice: "輸入",
  outputPrice: "輸出",
  priceUnavailable: "暫不可用",
  restoreAction: "還原原設定",
  revokeAction: "撤銷野菜設定",
  restoreFailed:
    "還原沒有完成，現有記錄已保留。請關閉目標應用後重試；不會強行覆蓋你的修改。",
  tokenCleanupPending:
    "本機設定已還原。遠端專用 Key 暫未撤銷——請連上網路並登入同一帳戶後，再次點擊「{{action}}」即可重試撤銷，不會改動已還原的設定。",
  tokenKept:
    "本機設定已還原。遠端專用 Key 已按你的選擇保留：它仍然有效且可能產生計費，可稍後在網站的權杖管理頁手動撤銷。",
  restoredWithChanges:
    "已還原可還原的設定，你之後修改的內容已保留。重新開啟應用後生效。",
  restoredOriginal: "已還原接入前的設定。重新開啟應用後生效。",
  revokedLegacy:
    "已撤銷野菜接入。舊版本沒有儲存原值，請在應用中選擇你要用的帳戶或服務商。",
  restoreError: "暫時無法還原，請重試。還原記錄仍保留在這台電腦上。",
  closeLabel: "關閉",
  introOriginal:
    "還原這個應用接入野菜前的模型、服務商和相關連線設定。你後來修改過的內容會保留。",
  introLegacy:
    "這個接入來自舊版本，沒有儲存接入前的設定。只能撤銷仍屬於野菜的連線項，無法找回原來的值。",
  noteNoUninstall: "不會解除安裝應用，也不會刪除聊天記錄。",
  noteIsolated: "不影響其他應用、網站帳戶或餘額。",
  noteReopen: "還原後請重新開啟 {{app}}。",
  revokeOptionTitle: "同時撤銷此應用的專用 Key",
  revokeOnHint: "撤銷後該 Key 立即失效，不再產生任何計費。",
  revokeOffHint:
    "保留後該 Key 仍然有效且可能繼續計費；可稍後在網站的權杖管理頁手動撤銷。",
  cancelRestore: "先不還原",
  restoring: "正在還原…",
  sectionLabel: "最近連線結果",
  recentTitle: "最近野菜中轉記錄",
  refreshResult: "重新整理結果",
  outcomeOk: "已確認經野菜中轉完成",
  outcomeTimeout: "等待模型回覆逾時",
  outcomeNetworkError: "未能連上模型服務",
  outcomeUpstreamError: "模型服務未完成請求",
  outcomeInvalidResponse: "模型回覆格式異常",
  outcomeStreamInterrupted: "回覆在完成前中斷",
  outcomeUnknownModel: "這個模型尚未加入常用清單",
  outcomePayloadTooLarge: "請求內容超過本機安全上限",
  outcomeLocalBusy: "本機正在處理另一條大請求",
  noModelSpecified: "未指定模型",
  advicePayloadTooLarge:
    "單次請求超過 200 MiB，未傳送到上游。請減少一次附帶的檔案或圖片後重試。",
  adviceLocalBusy:
    "為避免桌面助手卡死，本機一次只緩衝一條大請求。請等待目前請求完成後重試。",
  adviceUnauthorized: "請檢查帳戶與這個模型的使用權限。",
  adviceRateLimited: "請求較多或額度受限，請稍後重試並檢查帳戶。",
  adviceUnknownModel: "請選用已設定的模型，或將新模型加入清單後更新接入。",
  adviceRetry:
    "請先重試；若持續失敗，可手動更換線路。不會替你更換模型或計費分組。",
  emptyRequestState:
    "尚未收到該應用經野菜中轉的請求。傳送一則訊息後，可在這裡重新整理確認。",
  codexAccountNote:
    "Codex 顯示的官方帳號是登入身分，不是本次模型請求線路或計費方的證明；這裡出現中轉記錄後，才說明請求確實經過了野菜中轉。",
  attributionNote:
    "記錄來自伺服器端用量日誌，按該工具自己的金鑰歸因，顯示實際轉發的完整模型 ID；不依據應用縮寫或 AI 的自我介紹判斷。傳送訊息後稍等幾秒再重新整理即可看到。",
  checking: "正在檢查這台電腦…",
  checkAgain: "重新檢查應用",
  scanningHint: "正在讀取這台電腦上已安裝的應用，請稍候。",
  readingConnection: "正在讀取接入狀態…",
  retryConnectionRead: "重新讀取接入狀態",
  refreshingAccount: "正在重新整理帳戶…",
  syncingSelection: "正在同步選擇…",
  updateConnection: "更新接入設定",
  installAndConnect: "安裝並接入",
  applyRunningHint: "接入正在進行，完成或安全取消後會自動恢復操作。",
  targetScanRetryHint:
    "這次沒有完成本機應用檢查。點擊按鈕重新檢查，不會修改任何設定。",
  connectionReadRetryHint:
    "沒有讀到上次的接入狀態。點擊按鈕重新讀取，不會覆蓋現有設定。",
  connectionReadingHint: "正在讀取這台電腦上的現有接入狀態，請稍候。",
  accountRefreshingHint: "正在重新整理帳戶、模型和計費分組，請稍候。",
  selectionSyncHint: "正在同步目前帳戶的模型選擇，請稍候。",
  installed: "已找到 · 可接入",
  installedShort: "已安裝",
  chooseInstall: "選擇要使用的安裝位置",
  chooseInstallHint: "發現了多個安裝。助手已優先選中可用版本，你也可以更換。",
  missing: "未在這台電腦找到該應用，請先安裝後重新檢查。",
  unsupported:
    "找到了應用，但缺少啟動所需的元件。請確認應用安裝完整後重新檢查。",
  scanFailed: "暫時無法檢查本機應用，請重新檢查。",
  unavailable: "等待檢查",
  version: "版本",
  verifying: "正在安全儲存設定並啟動本機連線，不會傳送測試訊息…",
  verifyingCodex:
    "正在安全儲存 Codex 設定與金鑰，並準備本機路由，完成後會自動開啟應用…",
  verifyingDesktop:
    "正在安全儲存 Claude Desktop 設定並準備本機連線，不會等待模型回覆。",
  verifyingDsh: "正在儲存 DSH 設定並啟動本機工作台，不會傳送測試訊息…",
  readyTitle: "接入完成",
  firstActivationTitle: "第一次接入成功",
  applySucceeded: "接入成功",
  readyBody:
    "{{app}} 的設定已儲存，本機連線已就緒。首次使用後的真實結果會顯示在這裡。",
  selectInstallFirst: "先選擇安裝位置",
  installFirst: "請先安裝應用",
  updateFirst: "缺少執行元件",
  connectionFailed: "沒有完成接入，所有本機改動已恢復。請重新檢查後再試。",
  credentialHelperFailed:
    "Codex 無法從系統安全儲存讀取工具金鑰，設定已恢復。請結束後重新開啟野菜 API 再試。",
  authenticationFailed:
    "所選線路沒有接受工具金鑰，設定已恢復。請重新整理帳戶後重試。",
  endpointUnavailable:
    "所選線路暫時無法使用這個模型介面，設定已恢復。可以換一條線路或稍後重試。",
  providerTimedOut:
    "所選線路回應逾時，設定已恢復。可以換一條線路或稍後重試。",
  providerBusy: "目前模型請求較多，設定已恢復。請稍後重試或選擇其他模型。",
  modelRequestRejected:
    "所選模型沒有接受測試請求，設定已恢復。請重新整理模型清單後重新選擇。",
  invalidProviderResponse:
    "線路回傳了無法識別的模型回覆，設定已恢復。請稍後重試。",
  desktopTimedOut:
    "沒有收到 Claude Desktop 的測試訊息，接入未確認，本機改動已恢復。",
  missingDuringSetup: "剛才選擇的應用已找不到，請重新檢查。",
  selectionRequired: "發現多個安裝，請明確選擇要使用的一個。",
  secureStoreFailed:
    "系統安全儲存暫時無法使用。請解鎖鑰匙圈或憑證管理員，並檢查本機接入狀態後重試。",
  externalOverride:
    "偵測到系統環境變數（如 ANTHROPIC_BASE_URL 等）的優先順序高於助手寫入的設定，本次未改動原設定。請檢查並移除相關環境變數後重試。",
  unsupportedProfile: "這個應用的執行方式暫不能自動設定，原設定沒有改動。",
  launchFailed:
    "設定已經恢復，因為應用未能正常啟動。請確認應用可以手動開啟。",
  builderKicker: "模型與計費分組",
  builderTitle: "選好，就能用",
  builderIntro: "選模型、比較分組價格，再一鍵完成接入。網路線路單獨選擇。",
  modelChoice: "選擇模型",
  modelQuestion: "想用哪個 AI？",
  lineChoice: "選擇連線線路",
  lineQuestion: "按你所在的位置選擇，價格不會因此改變",
  yeschoyPrice: "野菜 API 價",
  saveInputOutput: "輸入省 {{input}} · 輸出省 {{output}}",
  finishChoice: "完成接入",
  finishHint: "安全寫入並讀回應用設定；不會傳送收費的測試訊息。",
  connectionDetails: "查看連線詳情",
  endpointReferenceNote: "文件參考值，實際以寫入應用設定的端點為準。",
  directConnection: "直接連線",
  automaticCompatibility: "自動相容",
  surfaceClaudeCode: "命令列與編輯器工作區",
  surfaceClaudeDesktop: "Claude 桌面應用",
  surfaceCodexDesktop: "ChatGPT 桌面應用中的 Codex",
  surfacePi: "Pi 程式設計助手",
  surfaceDsh: "DeepSeek Harness 瀏覽器工作台",
  lifecycleDesktopRestart:
    "若 {{name}} 正在執行，更新接入時會先提醒你儲存；確認後由助手先請求應用正常結束，寫入設定並重新開啟。Windows 若只剩背景程序，會僅結束這個安裝路徑對應的程序。",
  lifecycleBrowserLaunch:
    "更新接入會儲存 DSH 設定；「開啟使用」只會啟動本機服務並在瀏覽器中開啟，不會傳送模型測試訊息。",
  lifecycleTerminalSession:
    "更新接入不會關閉正在使用的命令列工作階段；新設定從新開的終端機工作階段生效。",
  retryRollbackFailed:
    "自動恢復上次未完成的設定時遇到問題，部分設定尚未恢復。可直接點擊「自動修復並重試」，無需手動修改設定。",
  retryCredentialRestore:
    "自動恢復時金鑰設定尚未恢復。請解鎖系統鑰匙圈或憑證管理員，再點擊「自動修復並重試」，無需手動修改設定。",
  retryRecoveryPending:
    "另一項接入或恢復操作正在進行。本次沒有修改應用，請稍後直接重試；若一直出現，可使用「還原原設定」。",
  readyTerminal:
    "{{app}} 的設定和本機連線已經就緒。正在執行的命令列工作階段不會被中斷；請新開一個工作階段，或點擊「開啟終端機使用」。第一次真實請求的結果會顯示在「最近連線結果」裡。",
  readyCodex:
    "{{app}} 的設定和野菜本機路由已經就緒。{{favorites}}Codex 仍可顯示你的官方登入帳號，那只是登入身分，不代表模型請求走官方計費。第一次真實請求的結果會顯示在「最近野菜中轉記錄」裡；看到完整模型 ID，才表示這次請求確實經過野菜中轉。",
  favoritesConfigured: "常用模型已一起設定。",
  readyDefault:
    "{{app}} 的設定和本機連線已經就緒。{{favorites}}請在應用中正常使用；第一次真實請求的結果會顯示在「最近連線結果」裡。",
  restartBlocked:
    "系統未能關閉 {{app}}，本次沒有修改設定。請確認沒有系統彈窗攔截，或手動結束後重試。",
  appRunning:
    "{{app}} 正在執行。請先儲存未完成內容，再確認由助手關閉並重新開啟。Windows 若只剩背景程序，只會結束已識別安裝路徑對應的程序。",
  unsupportedGroup: "所選分組已不可用，請重新整理帳戶資料後重新選擇。",
  launchStateUnavailable:
    "暫時無法安全確認應用是否正在執行，本次沒有修改設定。請手動結束應用後再試。",
  launchAccessDenied:
    "設定已經儲存，但系統阻止了自動開啟。請從系統選單手動開啟應用。",
  launchTargetChanged:
    "設定已經儲存，但應用安裝位置剛剛發生變化。請重新檢查應用後手動開啟。",
  launchStoreApp:
    "設定已經儲存，但暫時無法自動開啟這個市集應用。請先從開始選單開啟一次。",
  launchNotOpened:
    "設定已經儲存，但沒有自動開啟應用。請手動開啟；不需要重新接入。",
  verifyStartFailed:
    "應用沒有啟動成功，接入設定已恢復。請確認應用安裝完整且可以手動開啟。",
  verifyTimedOut: "應用的連線測試逾時，接入設定已恢復。請稍後重試。",
  verifyReadFailed:
    "未能讀取應用的測試結果，接入設定已恢復。請重新開啟助手後再試。",
  verifyInvalidReply:
    "應用沒有完成有效的模型回覆，接入設定已恢復。請檢查所選模型與分組後再試。",
  bridgeUnavailable:
    "野菜本機閘道沒有在限定時間內就緒，接入設定已恢復。請重新開啟野菜助手後再試。",
  bridgeAuthFailed:
    "野菜本機閘道的安全權杖不一致，接入設定已恢復。請重新接入，助手會自動產生新權杖。",
  bridgeCatalogInvalid:
    "野菜本機閘道沒有載入完整模型清單，接入設定已恢復。請重新檢查模型後再試。",
  installConfirmRequired:
    "安裝已完成，但目前選擇需要重新確認。請確認帳戶和模型後再點接入；這次沒有改動應用設定。",
  assistantShuttingDown:
    "正在結束助手，本次接入已停止；已寫入的設定會先恢復。",
  activationCancelled:
    "已取消本次接入；如果設定寫入已經開始，助手已先恢復原設定。",
  activationTimedOut:
    "接入等待超過 90 秒，頁面已恢復操作並通知背景安全取消。請先查看接入狀態；若仍顯示處理中，請等待片刻後再試，不要連續重複提交。",
  activationAlreadyRunning:
    "已有一項接入正在安全收尾，請稍等片刻後再試；本次沒有重複提交。",
  accountChanged: "帳戶已切換，本次接入已停止。請確認目前帳戶後重新接入。",
  invalidResponseResult:
    "暫時無法確認接入結果。請先檢查本機接入狀態，避免連續重複提交。",
  recoveryStorageUnavailable:
    "暫時無法安全儲存原設定，這次沒有修改應用。請確認系統鑰匙圈或憑證管理員可用後重試。",
  recoveryReceiptFailed:
    "接入記錄儲存未完成，請檢查本機狀態並恢復後重試。",
  progressQueued: "正在等待安全設定鎖…",
  progressCheckingApp: "正在檢查應用狀態和原設定…",
  progressAuthenticating: "正在確認帳戶和線路…",
  progressCheckingModels: "正在核對模型與計費分組…",
  progressSecuringAccess: "正在建立僅限所選模型的應用金鑰…",
  progressPreparingSettings: "正在準備可還原的設定…",
  progressApplyingSettings: "正在安全寫入並複核設定…",
  progressRestoring: "操作未完成，正在恢復原設定…",
  progressComplete: "接入完成。",
  versionUnreadNote: "版本未讀取，不影響嘗試接入",
  signInHint: "登入後即可掃描本機應用並一鍵接入。",
  refreshAccountLabel: "重新取得帳戶資料",
  refreshAccountHint:
    "帳戶資料沒有更新成功，價格與分組可能過期。點這裡重新取得。",
  installHint: "本機還沒有這個應用，助手會先安裝再接入。",
  installUnavailableHint:
    "本機沒有偵測到這個應用。安裝後點這裡重新檢查，不會修改任何設定。",
  updateFirstHint: "偵測到的版本暫不支援接入。更新應用後點這裡重新檢查。",
  selectInstallationHint: "這台電腦上有多個安裝位置，請先在上面選擇一個。",
  chooseModelFirst: "先選擇模型",
  chooseGroupFirst: "先選擇計費分組",
  checkModelsLabel: "檢查常用模型與分組",
  restartAppLabel: "關閉並重新開啟 {{app}}",
  restartAppHint:
    "設定還沒有寫入。這個應用正在執行，點這裡確認已儲存後由助手關閉並重新開啟。",
  viewAccountRelogin: "查看帳戶並重新登入",
  refreshModelsGroups: "重新整理模型與分組",
  viewOtherLines: "查看其他線路",
  recheckApp: "重新檢查應用和接入狀態",
  settingUpFor: "正在為這個應用設定",
  collapseApps: "收起應用",
  switchApp: "更換應用",
  showAllApps: "查看全部支援的應用",
  installationSummary: "安裝與接入資訊",
  autoSelectedSuffix: " · 已自動選擇",
  versionNotRead: "版本未讀取",
  accountStaleTitle: "帳戶資料暫未更新",
  accountStaleBody:
    "這裡保留的是上次的模型與價格。重新整理成功後再接入；已接入的應用仍可從「我的應用」開啟。",
  refreshAccountData: "重新整理帳戶資料",
  nativeInterface: "原生介面",
  missingModelPrefix: "之前選擇的",
  missingModelSuffix: "目前不可用。請重新選擇模型，不會自動替換。",
  billingCardHint: "選擇價格與來源，不改變網路線路",
  missingGroupTitle: "之前的計費分組已不可用",
  missingGroupBody:
    "不在這個模型目前可用的分組中。請在上方重新選擇，價格可能不同；不會自動切換。",
  favoriteModels: "常用模型",
  favoriteModelsIntro: "加入你會用的模型，每個模型單獨選擇計費分組。",
  updateModelGroup: "更新這個模型的分組",
  addFavoriteModel: "加入常用模型",
  defaultModelAria: "預設模型 {{model}}",
  removeModelAria: "移除 {{model}}",
  bindingUnavailable: "目前不可用，請重新選擇或移除",
  defaultBadge: "預設",
  removeAction: "移除",
  favoriteModelsEmpty: "也可以只接入上面選中的一個模型，之後再新增。",
  pendingModelEditNote:
    "上方的選擇尚未加入清單。點擊「{{action}}」後，再確認接入。",
  favoriteModelsNote:
    "同一模型保留一個計費分組。已有清單裡的模型可在應用內切換；新增模型、修改預設模型或分組後，需要更新接入。",
  networkLinePrefix: "網路線路",
  changeLine: "更換線路",
  lineIntro:
    "大陸優先適合中國大陸網路；全球加速使用 Cloudflare，海外可優先嘗試。線路只影響連線，不改變計費分組與倍率。",
  currentConnection: "目前接入",
  configuredSummary:
    "這些設定已經儲存。日常換模型可在應用內選擇；修改常用清單後，再更新接入。",
  billingGroupLabel: "計費分組",
  selectGroupFirst: "請選擇分組",
  favoriteModelsCount: "{{count}} 個常用模型",
  modelSetNote:
    "接入時安全設定全部模型；每個模型的真實連線結果在首次使用後顯示。",
  lifecycleNotePrefix: "開啟使用不會改設定。",
  backgroundNote:
    "使用時請保持野菜助手執行。關閉視窗可選擇繼續背景執行；完全結束會中斷模型連線，不會鎖定你的應用，隨時可以還原原設定。",
  progressStep: "第 {{done}} / {{total}} 步",
  progressNote:
    "進度會顯示在這裡；頁面其他區域仍可使用，請不要重複點擊接入按鈕。",
  cancelInProgress: "正在安全取消…",
  cancelSetup: "取消本次接入",
  feedbackStaleTitle: "剛才的接入已完成",
  feedbackReopenTitle: "需要重新開啟應用",
  feedbackSavedTitle: "設定已儲存",
  staleContextPrefix: "剛才處理的是帳戶",
  staleContextSuffix: "。現在的選擇尚未套用。",
  restartDialogTitle: "儲存後自動重新開啟 {{app}}",
  restartDialogMessage:
    "這個應用正在執行。請先儲存正在編輯的內容。\n\n繼續後，你不需要手動結束：野菜助手會先請求它正常結束，設定完成後再重新開啟。Windows 若只剩背景程序，會僅結束這個已識別安裝路徑對應的程序；不會按名稱結束其他程式。若系統仍阻止結束，本次不會修改設定。",
  restartDialogConfirm: "已儲存，結束並繼續",
  restartDialogCancel: "暫不接入",
};
const ja: Copy = {
  defaultGroup: "標準グループ",
  defaultGroupBillingNote: "標準グループの料金で課金されます（{{ratio}}×）",
  chooseGroupLegend: "料金プランを選択",
  planPriceUnavailable: "料金を確認中",
  planPriceUnit: "1億トークンあたりの概算",
  planSelected: "選択中",
  planLowest: "最安値",
  planDetails: "詳細",
  planRatio: "課金倍率",
  planDescription: "提供元のプラン説明",
  groupIntro:
    "このモデルの料金プランを選択してください。以下は同じトークン構成での概算です。",
  ratioPending: "倍率は確認中",
  groupsMissing:
    "このモデルの利用可能なグループを取得できませんでした。アカウントデータを更新してください。",
  groupsEmptyHint: "モデルを選択すると、対応する課金グループを確認できます。",
  pricesLabel: "選択したグループの料金",
  estimateSummary: "1億トークンの費用目安",
  estimateExampleNote: "費用の目安です。実際の費用は利用状況により変わります",
  estimateSaving: "約{{percent}}%お得",
  estimateFormula:
    "約0.69%新規入力 + 99.14%キャッシュ読み取り + 0.17%出力",
  estimateOfficialLabel: "公式サイトでの概算",
  estimateYeschoyLabel: "野菜APIでの概算",
  estimateNote:
    "公式価格は {{referenceFx}}、野菜API は {{siteFx}} で人民元に換算し、{{group}}の倍率を適用します{{tiered}}。固定参考レートであり、リアルタイム為替ではありません。{{cacheNote}}",
  estimateTieredNote: "。リクエストの課金階層により、上記の範囲が生じます",
  estimateCacheFallbackNote:
    "このモデルにはキャッシュ読み取り専用の価格がないため、キャッシュ部分は入力単価で保守的に概算しています。",
  estimateCacheExcludedNote:
    "別途発生するキャッシュ書き込みは含みません。実際の費用は、リクエストが該当した課金階層に基づきます。",
  estimateUnavailable:
    "現在のルールではトークン費用に正確に換算できないため、概算は表示しません。実際の費用はサイトの請求明細に基づきます。",
  perMillionTokens: "100万 tokens あたり",
  officialPrice: "公式参考価格",
  actualPrice: "野菜API実際の料金",
  inputPrice: "入力",
  outputPrice: "出力",
  priceUnavailable: "現在利用できません",
  restoreAction: "元の設定に戻す",
  revokeAction: "野菜の設定を取り消す",
  restoreFailed:
    "復元は完了しませんでした。既存の記録は保持されています。対象のアプリを終了してもう一度お試しください。変更を強制的に上書きすることはありません。",
  tokenCleanupPending:
    "この端末の設定は復元されました。リモートの専用キーはまだ取り消されていません——ネットワークに接続して同じアカウントでログインし、「{{action}}」をもう一度クリックすると取り消しを再試行できます。復元済みの設定が変更されることはありません。",
  tokenKept:
    "この端末の設定は復元されました。リモートの専用キーは選択どおり保持されています。引き続き有効で課金が発生する可能性があるため、後でサイトのトークン管理ページから手動で取り消せます。",
  restoredWithChanges:
    "復元可能な設定を復元しました。後から変更した内容は保持されています。アプリを開き直すと有効になります。",
  restoredOriginal:
    "接続前の設定に戻しました。アプリを開き直すと有効になります。",
  revokedLegacy:
    "野菜への接続を取り消しました。旧バージョンは元の値を保存していないため、アプリで使用するアカウントまたはプロバイダーを選んでください。",
  restoreError:
    "一時的に復元できません。もう一度お試しください。復元の記録はこの端末に残っています。",
  closeLabel: "閉じる",
  introOriginal:
    "このアプリを野菜に接続する前のモデル、プロバイダー、関連する接続設定に戻します。後から変更した内容は保持されます。",
  introLegacy:
    "この接続は旧バージョンによるもので、接続前の設定が保存されていません。野菜に属する接続項目だけを取り消せます。元の値を復元することはできません。",
  noteNoUninstall:
    "アプリのアンインストールやチャット履歴の削除は行いません。",
  noteIsolated:
    "他のアプリ、サイトのアカウント、残高には影響しません。",
  noteReopen: "復元後は {{app}} を開き直してください。",
  revokeOptionTitle: "このアプリの専用キーも取り消す",
  revokeOnHint:
    "取り消すと、このキーは直ちに無効になり、以降の課金は発生しません。",
  revokeOffHint:
    "保持すると、このキーは有効なままで課金が続く可能性があります。後でサイトのトークン管理ページから手動で取り消せます。",
  cancelRestore: "今はしない",
  restoring: "復元中…",
  sectionLabel: "直近の接続結果",
  recentTitle: "野菜経由の最近の記録",
  refreshResult: "結果を更新",
  outcomeOk: "野菜経由で完了したことを確認",
  outcomeTimeout: "モデルの応答待ちでタイムアウト",
  outcomeNetworkError: "モデルサービスに接続できませんでした",
  outcomeUpstreamError: "モデルサービスがリクエストを完了しませんでした",
  outcomeInvalidResponse: "モデルの応答形式が異常です",
  outcomeStreamInterrupted: "応答が完了する前に中断されました",
  outcomeUnknownModel: "このモデルはまだよく使うリストに追加されていません",
  outcomePayloadTooLarge: "リクエストの内容がこの端末の安全上限を超えています",
  outcomeLocalBusy: "この端末は別の大きなリクエストを処理中です",
  noModelSpecified: "モデル未指定",
  advicePayloadTooLarge:
    "1回のリクエストが 200 MiB を超えたため、上流には送信されませんでした。一度に添付するファイルや画像を減らしてから、もう一度お試しください。",
  adviceLocalBusy:
    "デスクトップアシスタントのフリーズを防ぐため、この端末では大きなリクエストを一度に1件しかバッファしません。現在のリクエストが完了してから、もう一度お試しください。",
  adviceUnauthorized:
    "アカウントと、このモデルの利用権限を確認してください。",
  adviceRateLimited:
    "リクエストが多いか、利用枠が制限されています。しばらくしてから再試行し、アカウントをご確認ください。",
  adviceUnknownModel:
    "設定済みのモデルを選ぶか、新しいモデルをリストに追加してから接続を更新してください。",
  adviceRetry:
    "まずは再試行してください。失敗が続く場合は、回線を手動で切り替えられます。モデルや課金グループが勝手に変更されることはありません。",
  emptyRequestState:
    "このアプリからの野菜経由のリクエストはまだありません。メッセージを送信した後、ここで更新して確認できます。",
  codexAccountNote:
    "Codex に表示される公式アカウントはログイン身元であり、今回のモデルリクエストの回線や課金先の証明ではありません。ここに中継記録が表示されて初めて、リクエストが実際に野菜経由であると確認できます。",
  attributionNote:
    "記録はサーバー側の使用ログから取得し、ツールごとのキーで帰属され、実際に転送された完全なモデル ID を表示します。アプリの略称やAIの自己紹介には基づきません。メッセージ送信後、数秒待って更新すると表示されます。",
  checking: "このコンピュータを確認中…",
  checkAgain: "アプリを再確認",
  scanningHint:
    "このコンピュータにインストールされたアプリを読み込んでいます。しばらくお待ちください。",
  readingConnection: "接続状態を読み込み中…",
  retryConnectionRead: "接続状態を再読み込み",
  refreshingAccount: "アカウントを更新中…",
  syncingSelection: "選択を同期中…",
  updateConnection: "接続設定を更新",
  installAndConnect: "インストールして接続",
  applyRunningHint:
    "接続設定が進行中です。完了または安全なキャンセル後に、操作は自動的に復帰します。",
  targetScanRetryHint:
    "今回、このコンピュータのアプリ確認が完了しませんでした。ボタンを押して再確認してください。設定は一切変更されません。",
  connectionReadRetryHint:
    "前回の接続状態を読み込めませんでした。ボタンを押して再読み込みしてください。既存の設定は上書きされません。",
  connectionReadingHint:
    "このコンピュータ上の既存の接続状態を読み込んでいます。しばらくお待ちください。",
  accountRefreshingHint:
    "アカウント、モデル、課金グループを更新しています。しばらくお待ちください。",
  selectionSyncHint:
    "現在のアカウントのモデル選択を同期しています。しばらくお待ちください。",
  installed: "検出済み · 接続可能",
  installedShort: "インストール済み",
  chooseInstall: "使用するインストール先を選択",
  chooseInstallHint:
    "複数のインストールが見つかりました。利用可能なバージョンを優先的に選択済みです。変更もできます。",
  missing:
    "このコンピュータでこのアプリは見つかりませんでした。先にインストールしてから再確認してください。",
  unsupported:
    "アプリは見つかりましたが、起動に必要なコンポーネントがありません。アプリのインストールが完全であることを確認してから再確認してください。",
  scanFailed: "現在、このコンピュータのアプリを確認できません。再確認してください。",
  unavailable: "確認待ち",
  version: "バージョン",
  verifying:
    "設定を安全に保存し、ローカル接続を開始しています。テストメッセージは送信しません…",
  verifyingCodex:
    "Codex の設定とキーを安全に保存し、ローカルルートを準備しています。完了後、自動的にアプリを開きます…",
  verifyingDesktop:
    "Claude Desktop の設定を安全に保存し、ローカル接続を準備しています。モデルの応答は待ちません。",
  verifyingDsh:
    "DSH の設定を保存し、ローカルワークスペースを起動しています。テストメッセージは送信しません…",
  readyTitle: "接続完了",
  firstActivationTitle: "初回接続完了",
  applySucceeded: "接続しました",
  readyBody:
    "{{app}} の設定は保存され、ローカル接続の準備が整いました。初回利用後の実際の結果がここに表示されます。",
  selectInstallFirst: "先にインストール先を選択",
  installFirst: "先にアプリをインストール",
  updateFirst: "実行コンポーネント不足",
  connectionFailed:
    "接続は完了しませんでした。このコンピュータ上の変更はすべて復元済みです。再確認してから、もう一度お試しください。",
  credentialHelperFailed:
    "Codex がシステムの安全なストレージからツールキーを読み取れませんでした。設定は復元済みです。野菜 API を終了して開き直し、もう一度お試しください。",
  authenticationFailed:
    "選択した回線がツールキーを受け付けませんでした。設定は復元済みです。アカウントを更新してから、もう一度お試しください。",
  endpointUnavailable:
    "選択した回線では現在このモデルのエンドポイントを利用できません。設定は復元済みです。別の回線に切り替えるか、しばらくしてから再試行してください。",
  providerTimedOut:
    "選択した回線の応答がタイムアウトしました。設定は復元済みです。別の回線に切り替えるか、しばらくしてから再試行してください。",
  providerBusy:
    "このモデルへのリクエストが集中しています。設定は復元済みです。しばらくしてから再試行するか、別のモデルを選んでください。",
  modelRequestRejected:
    "選択したモデルがテストリクエストを受け付けませんでした。設定は復元済みです。モデル一覧を更新してから選び直してください。",
  invalidProviderResponse:
    "回線から認識できないモデル応答が返されました。設定は復元済みです。しばらくしてから再試行してください。",
  desktopTimedOut:
    "Claude Desktop からのテストメッセージが届かず、接続は確認されませんでした。このコンピュータ上の変更は復元済みです。",
  missingDuringSetup: "先ほど選択したアプリが見つかりません。再確認してください。",
  selectionRequired:
    "複数のインストールが見つかりました。使用するものを1つだけ選んでください。",
  secureStoreFailed:
    "システムの安全なストレージを現在利用できません。キーチェーンまたは資格情報マネージャーのロックを解除し、このコンピュータの接続状態を確認してから再試行してください。",
  externalOverride:
    "システムの環境変数（ANTHROPIC_BASE_URL など）が、アシスタントが書き込んだ設定より優先されていることを検出しました。今回は元の設定を変更していません。該当する環境変数を確認して削除し、もう一度お試しください。",
  unsupportedProfile:
    "このアプリの実行方法は現時点で自動設定できません。元の設定は変更されていません。",
  launchFailed:
    "アプリを正常に起動できなかったため、設定は復元されました。アプリを手動で開けることを確認してください。",
  builderKicker: "モデルと課金グループ",
  builderTitle: "選べば、すぐ使える",
  builderIntro:
    "モデルを選び、グループの価格を比較して、ワンクリックで接続を完了します。ネットワーク回線は別々に選択します。",
  modelChoice: "モデルを選択",
  modelQuestion: "どの AI を使いますか？",
  lineChoice: "接続回線を選択",
  lineQuestion: "所在地に合わせて選択してください。価格は変わりません。",
  yeschoyPrice: "野菜 API 価格",
  saveInputOutput: "入力 {{input}} お得 · 出力 {{output}} お得",
  finishChoice: "設定を完了",
  finishHint:
    "設定を安全に書き込み、読み込み直します。課金されるテストメッセージは送信しません。",
  connectionDetails: "接続の詳細を表示",
  endpointReferenceNote:
    "ドキュメント上の参考値です。実際にはアプリ設定に書き込まれたエンドポイントが優先されます。",
  directConnection: "直接接続",
  automaticCompatibility: "自動互換",
  surfaceClaudeCode: "コマンドラインとエディターのワークスペース",
  surfaceClaudeDesktop: "Claude デスクトップアプリ",
  surfaceCodexDesktop: "ChatGPT デスクトップアプリの Codex",
  surfacePi: "Pi プログラミングアシスタント",
  surfaceDsh: "DeepSeek Harness ブラウザワークスペース",
  lifecycleDesktopRestart:
    "{{name}} が実行中の場合、接続の更新前に保存を促します。確認後、アシスタントがアプリの正常終了を要求し、設定を書き込んで開き直します。Windows でバックグラウンドプロセスだけが残っている場合は、このインストール先に対応するプロセスのみ終了します。",
  lifecycleBrowserLaunch:
    "接続の更新で DSH 設定を保存します。「開いて使用」はローカルサービスを起動してブラウザで開くだけで、モデルのテストメッセージは送信しません。",
  lifecycleTerminalSession:
    "接続の更新で、使用中のコマンドラインセッションを閉じることはありません。新しい設定は、新しく開いたターミナルセッションから有効になります。",
  retryRollbackFailed:
    "前回未完了の設定を自動復元する際に問題が発生し、一部の設定がまだ復元されていません。「自動修復して再試行」を直接クリックしてください。設定を手動で編集する必要はありません。",
  retryCredentialRestore:
    "自動復元の際、キーの設定が復元されていません。システムのキーチェーンまたは資格情報マネージャーのロックを解除してから、「自動修復して再試行」をクリックしてください。設定を手動で編集する必要はありません。",
  retryRecoveryPending:
    "別の接続または復元操作が進行中です。今回はアプリの設定を変更していません。しばらくしてからそのまま再試行してください。続けて発生する場合は、「元の設定に戻す」をご利用ください。",
  readyTerminal:
    "{{app}} の設定とローカル接続の準備が整いました。実行中のコマンドラインセッションは中断されません。新しいセッションを開くか、「ターミナルで開いて使用」をクリックしてください。最初の実際のリクエストの結果は「直近の接続結果」に表示されます。",
  readyCodex:
    "{{app}} の設定と野菜のローカルルートの準備が整いました。{{favorites}}Codex には引き続き公式のログインアカウントが表示されますが、それはログイン身元であり、モデルリクエストが公式の課金で処理されている証明ではありません。最初の実際のリクエストの結果は「野菜経由の最近の記録」に表示されます。完全なモデル ID が表示されて初めて、そのリクエストが実際に野菜経由であると確認できます。",
  favoritesConfigured: "よく使うモデルも一緒に設定されました。",
  readyDefault:
    "{{app}} の設定とローカル接続の準備が整いました。{{favorites}}アプリを普段どおりご利用ください。最初の実際のリクエストの結果は「直近の接続結果」に表示されます。",
  restartBlocked:
    "システムが {{app}} を終了できず、今回は設定を変更していません。システムのダイアログに遮られていないか確認するか、手動で終了してから再試行してください。",
  appRunning:
    "{{app}} が実行中です。先に未完了の内容を保存してから、アシスタントによる終了と再起動を確認してください。Windows でバックグラウンドプロセスだけが残っている場合は、識別済みのインストール先に対応するプロセスのみ終了します。",
  unsupportedGroup:
    "選択したグループは現在利用できません。アカウントデータを更新してから選び直してください。",
  launchStateUnavailable:
    "アプリが実行中かどうかを安全に確認できず、今回は設定を変更していません。アプリを手動で終了してからお試しください。",
  launchAccessDenied:
    "設定は保存されましたが、システムが自動起動をブロックしました。システムのメニューから手動でアプリを開いてください。",
  launchTargetChanged:
    "設定は保存されましたが、アプリのインストール先がちょうど変わりました。アプリを再確認してから手動で開いてください。",
  launchStoreApp:
    "設定は保存されましたが、現在このストアアプリを自動で開けません。先にスタートメニューから一度開いてください。",
  launchNotOpened:
    "設定は保存されましたが、アプリを自動で開きませんでした。手動で開いてください。再接続は不要です。",
  verifyStartFailed:
    "アプリを起動できず、接続設定は復元されました。アプリのインストールが完全で、手動で開けることを確認してください。",
  verifyTimedOut:
    "アプリの接続テストがタイムアウトし、接続設定は復元されました。しばらくしてから再試行してください。",
  verifyReadFailed:
    "アプリのテスト結果を読み取れず、接続設定は復元されました。アシスタントを開き直してからお試しください。",
  verifyInvalidReply:
    "アプリが有効なモデル応答を完了できず、接続設定は復元されました。選択したモデルとグループを確認してから、もう一度お試しください。",
  bridgeUnavailable:
    "野菜のローカルゲートウェイが制限時間内に準備できず、接続設定は復元されました。野菜アシスタントを開き直してからお試しください。",
  bridgeAuthFailed:
    "野菜のローカルゲートウェイのセキュリティトークンが一致せず、接続設定は復元されました。再接続してください。アシスタントが新しいトークンを自動生成します。",
  bridgeCatalogInvalid:
    "野菜のローカルゲートウェイが完全なモデル一覧を読み込めず、接続設定は復元されました。モデルを再確認してからお試しください。",
  installConfirmRequired:
    "インストールは完了しましたが、現在の選択を再確認する必要があります。アカウントとモデルを確認してから接続してください。今回はアプリの設定を変更していません。",
  assistantShuttingDown:
    "アシスタントを終了しているため、今回の接続は中止されました。書き込み済みの設定は先に復元されます。",
  activationCancelled:
    "今回の接続はキャンセルされました。設定の書き込みが始まっていた場合は、アシスタントが先に元の設定を復元済みです。",
  activationTimedOut:
    "接続の待機が90秒を超えたため、ページの操作を復帰させ、バックグラウンドに安全なキャンセルを通知しました。先に接続状態を確認してください。処理中と表示される場合は、続けて再送信せず、しばらく待ってからお試しください。",
  activationAlreadyRunning:
    "別の接続が安全に完了しつつあります。しばらく待ってからお試しください。今回の再送信はありません。",
  accountChanged:
    "アカウントが切り替わったため、今回の接続は中止されました。現在のアカウントを確認してから再接続してください。",
  invalidResponseResult:
    "現在、接続結果を確認できません。連続した再送信を避け、先にこのコンピュータの接続状態を確認してください。",
  recoveryStorageUnavailable:
    "元の設定を安全に保存できず、今回はアプリを変更していません。システムのキーチェーンまたは資格情報マネージャーが利用可能か確認してから再試行してください。",
  recoveryReceiptFailed:
    "接続記録の保存が完了していません。ローカルの状態を確認して復元し、再試行してください。",
  progressQueued: "安全な設定ロックを待機中…",
  progressCheckingApp: "アプリの状態と元の設定を確認中…",
  progressAuthenticating: "アカウントと回線を確認中…",
  progressCheckingModels: "モデルと課金グループを照合中…",
  progressSecuringAccess:
    "選択したモデル限定のアプリキーを作成中…",
  progressPreparingSettings: "復元可能な設定を準備中…",
  progressApplyingSettings: "設定を安全に書き込み、検証中…",
  progressRestoring: "操作が完了せず、元の設定を復元中…",
  progressComplete: "接続が完了しました。",
  versionUnreadNote: "バージョン未取得。接続の試行には影響しません",
  signInHint:
    "ログインすると、このコンピュータのアプリをスキャンしてワンクリックで接続できます。",
  refreshAccountLabel: "アカウントデータを再取得",
  refreshAccountHint:
    "アカウントデータの更新に失敗したため、価格とグループが古い可能性があります。ここをクリックして再取得してください。",
  installHint:
    "このコンピュータにはまだこのアプリがありません。アシスタントが先にインストールしてから接続します。",
  installUnavailableHint:
    "このコンピュータでこのアプリは検出されませんでした。インストール後にここをクリックして再確認してください。設定は一切変更されません。",
  updateFirstHint:
    "検出されたバージョンは現時点で接続に対応していません。アプリを更新してからここをクリックして再確認してください。",
  selectInstallationHint:
    "このコンピュータには複数のインストール先があります。上で1つ選んでください。",
  chooseModelFirst: "先にモデルを選択",
  chooseGroupFirst: "先に課金グループを選択",
  checkModelsLabel: "よく使うモデルとグループを確認",
  restartAppLabel: "{{app}} を閉じて開き直す",
  restartAppHint:
    "設定はまだ書き込まれていません。このアプリが実行中です。ここをクリックして保存を確認すると、アシスタントが閉じて開き直します。",
  viewAccountRelogin: "アカウントを表示して再ログイン",
  refreshModelsGroups: "モデルとグループを更新",
  viewOtherLines: "他の回線を表示",
  recheckApp: "アプリと接続状態を再確認",
  settingUpFor: "設定中のアプリ",
  collapseApps: "アプリを折りたたむ",
  switchApp: "アプリを変更",
  showAllApps: "対応するすべてのアプリを表示",
  installationSummary: "インストールと接続の情報",
  autoSelectedSuffix: " · 自動選択済み",
  versionNotRead: "バージョン未取得",
  accountStaleTitle: "アカウントデータが未更新です",
  accountStaleBody:
    "ここには前回のモデルと価格が保持されています。更新が成功してから接続してください。接続済みのアプリは引き続き「マイアプリ」から開けます。",
  refreshAccountData: "アカウントデータを更新",
  nativeInterface: "ネイティブ API",
  missingModelPrefix: "先ほど選択した",
  missingModelSuffix:
    "は現在利用できません。モデルを選び直してください。自動的に置き換えられることはありません。",
  billingCardHint:
    "価格と提供元を選択。ネットワーク回線は変更されません",
  missingGroupTitle: "以前の課金グループは利用できなくなりました",
  missingGroupBody:
    "このモデルが現在利用できるグループに含まれていません。上で選び直してください。価格が異なる場合があります。自動的に切り替えることはありません。",
  favoriteModels: "よく使うモデル",
  favoriteModelsIntro:
    "使うモデルを追加してください。モデルごとに課金グループを個別に選択できます。",
  updateModelGroup: "このモデルのグループを更新",
  addFavoriteModel: "よく使うモデルに追加",
  defaultModelAria: "既定のモデル {{model}}",
  removeModelAria: "{{model}} を削除",
  bindingUnavailable: "現在利用できません。選び直すか削除してください",
  defaultBadge: "既定",
  removeAction: "削除",
  favoriteModelsEmpty:
    "上で選んだ1つのモデルだけを接続し、後から追加することもできます。",
  pendingModelEditNote:
    "上の選択はまだリストに追加されていません。「{{action}}」をクリックしてから、接続を確認してください。",
  favoriteModelsNote:
    "同じモデルには1つの課金グループを保持します。リスト内のモデルはアプリ内で切り替えられます。モデルの追加、既定モデルやグループの変更後は、接続を更新する必要があります。",
  networkLinePrefix: "ネットワーク回線",
  changeLine: "回線を変更",
  lineIntro:
    "大陸最適化は中国大陸のネットワークに適しています。グローバル高速化は Cloudflare を使用し、海外では先に試す価値があります。回線は接続にのみ影響し、課金グループや倍率は変わりません。",
  currentConnection: "現在の接続",
  configuredSummary:
    "これらの設定は保存済みです。日々のモデル切り替えはアプリ内で行えます。よく使うリストを変更した後は、接続を更新してください。",
  billingGroupLabel: "課金グループ",
  selectGroupFirst: "グループを選択してください",
  favoriteModelsCount: "よく使うモデル {{count}} 件",
  modelSetNote:
    "接続時にすべてのモデルを安全に設定します。各モデルの実際の接続結果は、初回利用後に表示されます。",
  lifecycleNotePrefix: "開いて使用しても設定は変更されません。",
  backgroundNote:
    "使用中は野菜アシスタントを実行したままにしてください。ウィンドウを閉じるとバックグラウンドでの実行を継続できます。完全に終了するとモデル接続は中断されますが、アプリをロックすることはなく、いつでも元の設定に戻せます。",
  progressStep: "ステップ {{done}} / {{total}}",
  progressNote:
    "進行状況はここに表示されます。ページの他の部分は引き続き使用できます。接続ボタンを繰り返しクリックしないでください。",
  cancelInProgress: "安全にキャンセル中…",
  cancelSetup: "今回の接続をキャンセル",
  feedbackStaleTitle: "先ほどの接続は完了しています",
  feedbackReopenTitle: "アプリを開き直す必要があります",
  feedbackSavedTitle: "設定を保存しました",
  staleContextPrefix: "先ほど処理したのはアカウント",
  staleContextSuffix: "です。現在の選択はまだ適用されていません。",
  restartDialogTitle: "保存後、{{app}} を自動的に開き直す",
  restartDialogMessage:
    "このアプリが実行中です。編集中の内容を先に保存してください。\n\n続行すると、手動で終了する必要はありません。野菜アシスタントが先に正常終了を要求し、設定の書き込み後に開き直します。Windows でバックグラウンドプロセスだけが残っている場合は、識別済みのこのインストール先に対応するプロセスのみ終了し、名前で他のプログラムを終了することはありません。システムが終了をブロックし続ける場合、今回の設定は変更されません。",
  restartDialogConfirm: "保存済み、終了して続行",
  restartDialogCancel: "今は接続しない",
};
export const configurationCopies = { zh, "zh-TW": tw, en, ja };
export type ConfigurationCopy = Copy;
export function useConfigurationCopy(): Copy {
  const { i18n } = useTranslation();
  const lang = i18n.resolvedLanguage ?? i18n.language;
  return (
    configurationCopies[lang as keyof typeof configurationCopies] ??
    (lang?.startsWith("zh") ? zh : en)
  );
}
