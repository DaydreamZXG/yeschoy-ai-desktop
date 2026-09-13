# 仓库内验收工具

本目录固定了 2026-09-12 使用的 `govern-product-build` 验收脚本及策略，避免验收依赖开发者全局技能目录的可变内容。仅用于开发验收，不进入客户端运行时。原全局技能未修改。

本仓库修正：已完成的旧验收之后发生源码变化时，默认仍失败。只有产品负责人**明确批准建立当前基线**、新单元显式依赖并替代旧单元，并绑定内容寻址的衔接记录，才允许进入新单元的架构审查。衔接记录不是通过测试的证据。

## 日常使用

```sh
python3 scripts/governance/runtime/scripts/validate_governance.py --project-root . --mode review --release-unit RU-076
python3 scripts/governance/runtime/scripts/seal_governance.py --project-root . --release-unit RU-076 --decision-id DEC-124
python3 scripts/governance/runtime/scripts/dispatch_work_package.py --project-root . --release-unit RU-076 --package-id WP-RU076-CURRENT-BASELINE --assigned-to root
python3 scripts/governance/runtime/scripts/verify_work_package.py --project-root . --run-id <dispatch-returned-id>
```

有经授权的历史漂移时，先用 `baseline_reconciliation.py --project-root . --release-unit <review-unit>` 生成只读、不可覆盖的衔接记录，再把返回的两字段对象放到该未冻结单元的 `baselineReconciliation`。对应决策必须为 accepted 并显式包含 `approvesBaselineReconciliation: true`。这是额外的授权，不应成为日常默认选项。之后仍需正常审查、冻结、调度和验证。

## 不变的保护

- 旧报告、命令、机器输出、源码快照和哈希全部重新验证；不能通过衔接掩盖篡改。
- 新记录绑定单元、决策、直接前任、前任最后完成的报告和完整当前文件哈希；记录之后再改相关源码会失效。
- 无关单元的漂移、任何进行中的任务、未接受的决策仍阻塞。
- 原报告和冻结快照不改写；冻结后的新单元包含衔接记录哈希。
- 调度后的新验收必须产生自己的只读隔离测试证据。未测试的平台、原生启动、发布和线上行为不得由前端回归推断为通过。

工具自身回归：`python3 scripts/governance/runtime/scripts/self_test.py`。覆盖默认拒绝漂移、显式授权、过期记录、记录/历史报告篡改、无关单元漂移、运行中任务，以及原工具的冻结、回放、依赖、只读边界和所有权测试。
