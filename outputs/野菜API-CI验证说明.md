# 野菜API候选版 CI 验证说明（RU-006）

## 当前范围

本次只适配 GitHub Actions 与 CI 校验脚本，不修改 NewAPI、服务器、客户端运行逻辑、密钥、本地工具配置或任何线上配置，也不自动发布安装包。

`YesChoy CI` 对 `main` 和 `product/**` 的 push / pull_request 运行，也支持 `workflow_dispatch`。所有变更都触发，不用路径过滤跳过检查。`YesChoy Nightly` 保留原夜间时间设置，调用同一提交中的 CI 定义；文件仍叫 `wsl2-nightly.yml` 以保留入口，但它不再声称覆盖 WSL。

| 检查 | 实际验证范围 |
| --- | --- |
| 前端（Ubuntu） | 冻结 pnpm 依赖、TypeScript、工作流格式、当前候选版 11 项单测、真实 renderer 构建、RU-005 当前验收与 RU-006 CI 回归 |
| Windows x64 | Rust 格式、Clippy、当前库全部测试、应用二进制编译 |
| macOS Intel | 同上，在原生 Intel runner 运行 |
| macOS Apple Silicon | 同上，在原生 arm64 runner 运行 |

Node 使用 22，pnpm 由 package.json 的 packageManager 固定，Rust 与 rust-toolchain.toml 的 1.95 对齐。Actions 固定到已核对的 commit SHA。Rust 缓存按操作系统、架构、工具链文件和 Cargo.lock 隔离，不复用宽泛的上游缓存键。

## 原失败与修复

旧运行 [33335919230](https://github.com/DaydreamZXG/yeschoy-ai-desktop/actions/runs/33335919230) 首轮 Windows 编译成功，随后把 TEMP/TMP 改成 WSL UNC 路径，再调用 cargo test 导致重新链接。Windows 的 link.exe / mt.exe 因临时 manifest 路径报 c1010070 / LNK1327，业务测试尚未开始。

旧流程还指定 `cc_switch_lib` 和 `config::tests::atomic_write_replaces_existing_wsl_unc_file`；当前编译入口是 `yeschoy_desktop_lib`，未注册旧 config 模块。恢复这个写入模块并不能代表当前只读客户端测试通过。因此本次用当前候选测试替换该无效门禁，不保留一个永远找不到的测试，也不通过忽略失败制造绿色结果。

新流程保留 runner 自带的本机临时目录。`scripts/ci/run-rust-tests.py` 根据 Cargo JSON 编译产物选择唯一的当前库测试程序，直接执行它，要求：

- 发现至少一项测试且包含当前六项核心用例；不得重复或选择旧库。
- 执行成功且通过数量等于发现数量；失败、ignored、filtered、空结果、缺少汇总、编译错误或超时都退出失败。
- 不使用 shell、不覆盖 home 或 TEMP/TMP，不调用真实模型、不发送账号或配置。

RU-001/002/003 的历史测试和上游测试仍保留在仓库，部分断言对应已被后续发布单元替换的界面，不将它们冒充当前验收。当前用户状态与安全边界由 RU-005 验收、候选单测和原生六项用例覆盖，CI 自身由 RU-006 回归覆盖。未运行通用 `pnpm test:unit`，避免扫入未启用的上游组件与本机 work 参考副本。

## 验收与恢复

本地可运行：

```sh
pnpm typecheck
pnpm test:candidate
pnpm build:renderer
python3 -m pytest -q --import-mode=importlib tests/work_packages/ru005 tests/work_packages/ru006
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
python3 scripts/ci/run-rust-tests.py
```

本地证据不能替代 GitHub 原生平台结果。推送后按同一 commit SHA 检查前端、Windows、两种 macOS 任务。旧失败记录保留，不改写历史为成功。夜间入口可手动 workflow_dispatch 验证，不必等待次日定时触发。

同一 workflow / 分支的新运行会取消旧运行，取消不等于成功；只有最新预期 SHA 的所有任务结论为 success 才能报告该轮 CI 通过。网络或 runner 临时故障可以重跑同一 SHA；持续的编译或测试失败必须定位修复，不允许 continue-on-error、删测试或放宽断言。需要修改运行时代码时，单独审查其发布单元，不在 CI 修复中偷偷扩大范围。

## 明确未覆盖

- **WSL 配置写入未验证**：候选版没有配置写入能力，本次没有 WSL 文件系统兼容性结论。未来加入该能力时需新增真正启用的契约测试。
- **不是安装包验收**：二进制编译不代表 DMG/EXE/MSI 安装、启动界面、卸载、覆盖升级、签名、公证或自动更新已经完成。本次不生成发布安装包，也不调用原 release/R2 工作流。
- 不证明真实登录、余额、充值、模型调用、价格计费或线上连通性；这些仍遵循 RU-005 的未接入边界。
- macOS hosted runner 不等于最低支持系统和所有真机兼容。私有仓库的每日检查会消耗 GitHub Actions 额度。

参考：[GitHub runner 平台](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)、[同提交复用工作流](https://docs.github.com/en/actions/how-tos/reuse-automations/reuse-workflows)。
