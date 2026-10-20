# 验收记录（勿当生产部署凭证）

- 时间：2026-10-20（Asia/Shanghai）
- 上游：`QuantumNous/new-api` @ `603a5ceb55908cc24b509aca2caa6fc956c9958c`
- 动作：`--dry-run` → `--yes` → `go build`（stub `web/dist`）→ 进程冒烟
- 冒烟结果：
  - `GET /api/desktop/v2/bootstrap` → 200
  - `GET /desktop/connect` → 200（含 connect.js / data-yeschoy-connect）
  - 未登录 `POST …/connect-codes` → 401
  - 登录后 refresh → Bearer 发码 → exchange → `GET /api/user/self` 读到 root + quota
  - 二次兑码 → `connect_code_expired`
  - 错误 Host 兑码 → `connect_origin_mismatch`
  - `--undo` 后与上游文件一致
- 野菜完整栈仓：脚本正确拒绝
- **未**对生产站部署；**未**推 QuantumNous 上游
