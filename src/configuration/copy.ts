const zh = {
  defaultGroup: "标准方案",
  defaultGroupBillingNote: "已按标准分组计费（{{ratio}}×）",
  chooseGroupLegend: "选择价格方案",
  planPriceUnavailable: "价格待确认",
  planPriceUnit: "每 1 亿 Token 预计费用",
  planSelected: "当前选择",
  planLowest: "价格最低",
  planDetails: "计费详情",
  planRatio: "计费倍率",
  planDescription: "方案说明",
  groupIntro:
    "同一个模型有不同价格方案，选好后按该方案计费。以下费用使用相同的 Token 占比估算。",
  ratioPending: "倍率待查询",
  // 读不到分组只说明这个模型不在价目表里，不是出了错。以前这句话让人以为
  // 卡住了、得去刷新，其实照常接入就行 —— 按账户默认分组计费。
  groupsMissing: "这个模型暂无分组价格，按账户默认分组计费，可以正常接入。",
  groupsEmptyHint: "选择模型后，可查看对应计费分组。",
  pricesLabel: "所选分组价格",
  estimateSummary: "1 亿 Token 费用参考",
  estimateExampleNote: "费用参考，实际费用随使用情况变化",
  estimateSaving: "预计节省约 {{percent}}%",
  estimateAmount: "约 {{amount}}",
  estimateFormula: "约 0.69% 新输入 + 99.14% 缓存读取 + 0.17% 输出",
  estimateOfficialLabel: "使用官网预计",
  estimateYeschoyLabel: "使用野菜预计",
  estimateNote:
    "两边采用相同模型、计费档位和使用量比较。官网按 {{referenceFx}}、野菜按 {{siteFx}} 换算为人民币，野菜费用已包含所选方案优惠。{{tiered}}固定参考汇率，实际费用以账单为准。{{cacheNote}}",
  estimateTieredNote:
    "优先采用标准档；没有明确标准档时采用服务端首档。其他档位的费用范围见上方。",
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
  cacheReadPrice: "缓存读取",
  cacheWritePrice: "缓存写入",
  cachePriceUnavailable: "暂无报价",
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
  chooseInstall: "「打开使用」启动哪一个",
  // 这个选择不影响写什么设置 —— 所有适配器的配置都写在用户目录下，
  // 跟选中哪个安装无关。它只决定助手替你启动哪一个副本。
  // 原文案「选择要使用的安装位置」会让人以为选错了就配错了。
  chooseInstallHint:
    "发现了多个副本。设置写在你的用户目录里，对哪个副本都生效；这里只决定「打开使用」启动哪一个。助手默认选中你终端里会跑的那个。",
  // 不兼容的模型以前是直接从列表里消失的。对小白来说「没有这个模型」和
  // 「这个模型在别的应用里能用」是天差地别的两件事，而他自己分辨不出来。
  // 所以灰着显示，并且直接告诉他去哪个应用用 —— 不解释协议。
  groupNotForThisApp: "这个价格方案在{{app}}里用不了",
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
  verifyingWorkBuddy:
    "正在把模型安全写入 WorkBuddy，并读回确认；不会发送收费测试消息，也不会关闭 WorkBuddy…",
  // 这两条以前叫「接入完成」和「第一次接入成功」。写完配置文件并不等于模型能用，
  // 而「成功」是用户唯一会读的那个词 —— 他据此关掉助手去干活，然后在目标应用里
  // 撞上第一个报错。现在标题只说已经发生的事：设置写完了。能不能用，等真实请求
  // 的证据（见 verifiedTitle）。
  readyTitle: "设置已完成",
  firstActivationTitle: "第一次跑通了",
  // 第二级：助手在服务端用量日志里看到了这条接入的一次真实成功请求。
  awaitingFirstUseBody:
    "打开 {{app}} 随便发一句话。助手看到这次真实请求之后，才会说接入可用 —— 在那之前它只知道设置写对了。",
  verifiedTitle: "首次使用已验证",
  verifiedBody: "{{app}} 已经用 {{model}} 通过野菜完成了一次真实请求。",
  applySucceeded: "接入成功",
  readyBody:
    "{{app}} 的设置已保存，本地连接已就绪。首次使用后的真实结果会显示在这里。",
  selectInstallFirst: "先选择安装位置",
  installFirst: "请先安装应用",
  updateFirst: "缺少运行组件",
  connectionFailed: "没有完成接入，所有本机改动已恢复。请重新检查后再试。",
  // 这四条对应写配置文件的四种失败。它们在 Windows 上最常见（杀毒软件锁文件、
  // 权限不足、磁盘已满、用户手工改坏配置），过去都落到通用的"设置未能完整更新"，
  // 没有告诉用户原因，也没有可执行的下一步。措辞按各自的回滚语义区分：
  // 写入/回读失败会回滚，解析失败发生在写之前，读取失败无法保证已回滚。
  configurationParseFailed:
    "这个应用现有的配置文件无法读懂，可能被手动改过或已损坏。本次没有改动任何设置。请先备份这个文件，修复其中的格式错误（或删除后让应用重新生成），再重试接入。",
  configurationReadFailed:
    "读不到这个应用的配置文件，可能被其他程序占用，或者没有读取权限。请关闭这个应用后重试；如果首页显示有待恢复的设置，请先恢复原设置。",
  configurationWriteFailed:
    "无法写入这个应用的配置文件，原设置已恢复。常见原因是杀毒软件或这个应用本身正占用文件，也可能是磁盘空间不足。请关闭它们后重试。",
  configurationReadbackFailed:
    "设置刚写入就被其他程序改了回去，原设置已恢复。常见原因是杀毒软件或云同步盘正在监控这个文件。请把它加入白名单或先暂停同步，再重试。",
  credentialHelperFailed:
    "Codex 无法从系统安全存储读取工具密钥，设置已恢复。请退出后重新打开野菜 API 再试。",
  authenticationFailed:
    "所选线路没有接受工具密钥，设置已恢复。请刷新账户后重试。",
  endpointUnavailable:
    "所选线路暂时无法使用这个模型接口，设置已恢复。可以换一条线路或稍后重试。",
  providerTimedOut: "所选线路响应超时，设置已恢复。可以换一条线路或稍后重试。",
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
    "这个应用的配置目录被其他设置占用，助手没有改动原文件。请改用助手里的「打开」启动，或换一个未被占用的安装后再试。",
  unsupportedProfile: "这个应用的运行方式暂不能自动配置，原设置没有改动。",
  launchFailed: "设置已经恢复，因为应用未能正常启动。请确认应用可以手动打开。",
  builderKicker: "模型与计费分组",
  builderTitle: "选好，就能用",
  builderIntro: "选模型、比较分组价格，再一键完成接入。网络线路单独选择。",
  // 三个圆圈数字是纯装饰（aria-hidden），读屏用户过去完全拿不到步骤结构。
  stepOf: "第 {{n}} 步，共 3 步",
  modelAndPriceStep: "选择模型与价格",
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
  surfaceWorkBuddy: "腾讯 AI 办公与开发助手",
  lifecycleDesktopRestart:
    "若 {{name}} 正在运行，更新接入时会先提醒你保存；确认后由助手先请求应用正常退出，写入设置并重新打开。Windows 若只剩后台进程，会仅结束这个安装路径对应的进程。",
  lifecycleBrowserLaunch:
    "更新接入会保存 DSH 配置；“打开使用”只会启动本地服务并在浏览器中打开，不会发送模型测试消息。",
  lifecycleTerminalSession:
    "更新接入不会关闭正在使用的命令行会话；新设置从新开的终端会话生效。",
  lifecycleHotReload:
    "WorkBuddy 会自动读取更新后的模型列表，不需要关闭或重启。已打开的对话可能继续使用原模型；请新建对话并在模型选择器中选择野菜模型。",
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
  // WorkBuddy 的配置格式不支持「取密钥的命令」这种写法，所以它是六个应用里
  // 唯一把密钥直接写进配置文件的。文件权限已设为仅本人可读，但同步到网盘、
  // 贴进工单、共享屏幕都会漏出去，所以这里必须说一句。
  readyWorkBuddy:
    "模型已加入 WorkBuddy。{{favorites}}无需重启，请新建对话并在模型选择器中选择野菜模型；已有对话不会被强行切换。WorkBuddy 的配置格式要求密钥直接写在 ~/.workbuddy/models.json 里（已设为仅你本人可读）——别把这个文件放进网盘同步目录，也别直接发给别人。",
  restartBlocked:
    "系统未能关闭 {{app}}，本次没有修改设置。请确认没有系统弹窗拦截，或手动退出后重试。",
  appRunning:
    "{{app}} 正在运行。请先保存未完成内容，再确认由助手关闭并重新打开。Windows 若只剩后台进程，只会结束已识别安装路径对应的进程。",
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
  // 通用的 launchNotOpened 说"没有自动打开"，对这两种情况都不够准确：
  // 一种是进程根本没能启动，另一种是启动了但没能确认成功——后者应用很可能已经开着。
  launchStartFailed:
    "设置已经保存，但应用没能启动。请确认它还在原来的位置、并且可以手动打开；不需要重新接入。",
  launchUnconfirmed:
    "设置已经保存，但没能确认应用是否启动成功。请手动看一下它有没有打开；不需要重新接入。",
  // 下面这些 reason code 此前全部落到通用的 launchNotOpened / setupWriteFailed。
  // 其中 terminal_* 是 Claude Code 和 Pi 的启动路径：对终端工具说「请手动打开
  // 应用」是错的建议 —— 助手的价值恰恰是替用户把带参数的终端拉起来。
  launchTerminalFailed:
    "设置已经保存，但没能打开终端窗口。你可以自己打开终端再启动它；不需要重新接入。",
  launchTerminalUnavailable:
    "设置已经保存，但没有找到可用的终端程序。macOS 请确认「终端」还在，Windows 请确认系统目录可正常访问；不需要重新接入。",
  launchWorkspaceUnavailable:
    "设置已经保存，但工作目录不可用，可能已被移动、删除或没有写入权限。请重新选择一个工作目录后再打开。",
  launchTargetInvalid:
    "设置已经保存，但这个应用的安装路径无法使用（不是绝对路径，或含有特殊字符）。请把它安装到常规位置后重新检查应用。",
  launchTargetMissing:
    "设置已经保存，但没有找到这个应用的可执行文件，它可能已被移动或卸载。请重新检查应用。",
  launchExitFailed:
    "设置已经保存，但这个应用没能退出，新设置要重开之后才生效。请手动退出它，再用「打开使用」启动。",
  launchWaitFailed:
    "设置已经保存，但没能确认这个应用是否已经退出。请手动确认它已关闭，再用「打开使用」启动。",
  homeUnavailable:
    "读不到当前用户的主目录，本次没有改动任何设置。请确认账户的用户目录可以正常访问后重试。",
  // Codex 的历史接管只改「这条对话属于哪个供应商」这一项元数据，从不读写消息
  // 正文。所以这三条都必须先把「记录没丢」讲清楚 —— 用户最怕的就是这个。
  codexHistoryConflict:
    "上一次的 Codex 历史整理还没有收尾，本次没有继续，设置也没有改动。你的对话记录没有丢失。请关闭 Codex 后重试。",
  codexHistoryFailed:
    "没能整理 Codex 历史对话的归属，设置已恢复。你的对话记录没有丢失，仍在原来的位置。请关闭 Codex 后重试。",
  codexHistoryRecoveryFailed:
    "整理 Codex 历史归属失败后没能完成回退。你的对话记录没有被删除，但可能有一部分暂时不出现在 Codex 的恢复列表里。请先不要新建对话，回到接入设置执行「恢复原设置」。",
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
  assistantShuttingDown: "正在退出助手，本次接入已停止；已写入的设置会先恢复。",
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
  recoveryReceiptFailed: "接入记录保存未完成，请检查本地状态并恢复后重试。",
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
  pendingModelEditNote:
    "上方的选择尚未加入列表。点击“{{action}}”后，再确认接入。",
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

export const configurationCopies = { zh };
export type ConfigurationCopy = Copy;
export function useConfigurationCopy(): Copy {
  return zh;
}
