#!/usr/bin/env bash
# YesChoy / 野菜 — New API「桌面一键连」最小补丁安装器（草案）
# 对照：yeschoy-ai-desktop/docs/patch/new-api-desktop-connect/
# 状态：可在本地对源码目录跑；不部署生产、不推上游、不自动 docker up。
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PAYLOAD_DIR="${SCRIPT_DIR}/payload"
HELPERS="${SCRIPT_DIR}/patch_helpers.py"
FILE_MARKER="YESCHOY_DESKTOP_CONNECT"

SRC=""
DRY_RUN=0
SKIP_FRONTEND=0
ASSUME_YES=0
UNDO=0

usage() {
  cat <<'USAGE'
用法：
  ./install-newapi-desktop-connect.sh --src /path/to/new-api [选项]

选项：
  --src DIR          New API 源码根目录（必填）
  --dry-run          只探测、列将改文件，不写入
  --skip-frontend    不注册 /desktop/connect 页面（你已自己做了）
  --yes              跳过确认提示
  --undo             按备份包回滚（目录内 desktop-connect-backup-*.tgz）
  -h, --help         显示帮助

示例：
  ./install-newapi-desktop-connect.sh --src ~/new-api --dry-run
  ./install-newapi-desktop-connect.sh --src ~/new-api --yes
  ./install-newapi-desktop-connect.sh --src ~/new-api --undo --yes
USAGE
}

log()  { printf '→ %s\n' "$*"; }
warn() { printf '⚠ %s\n' "$*" >&2; }
die()  { printf '✗ %s\n' "$*" >&2; exit 1; }
ok()   { printf '✓ %s\n' "$*"; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --src) SRC="${2:-}"; shift 2 ;;
    --dry-run) DRY_RUN=1; shift ;;
    --skip-frontend) SKIP_FRONTEND=1; shift ;;
    --yes|-y) ASSUME_YES=1; shift ;;
    --undo) UNDO=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "未知参数: $1（用 --help 看用法）" ;;
  esac
done

[[ -n "$SRC" ]] || die "请用 --src 指定 New API 源码目录"
SRC="$(cd "$SRC" && pwd)" || die "源码目录不存在: $SRC"
[[ -f "$HELPERS" ]] || die "缺少 $HELPERS"
[[ -d "$PAYLOAD_DIR" ]] || die "缺少补丁包目录：$PAYLOAD_DIR"

detect_module() {
  local mod
  [[ -f "$SRC/go.mod" ]] || die "不是 Go 项目：找不到 $SRC/go.mod"
  mod="$(awk '/^module /{print $2; exit}' "$SRC/go.mod")"
  [[ -n "$mod" ]] || die "go.mod 里没有 module 行"
  if [[ "$mod" != *new-api* && "$mod" != *newapi* ]]; then
    warn "module 名是「$mod」，不像常见 New API；将继续，但请确认这是对的仓。"
  fi
  printf '%s' "$mod"
}

detect_repo() {
  [[ -d "$SRC/controller" && -d "$SRC/router" && -d "$SRC/model" && -d "$SRC/middleware" ]] \
    || die "目录结构不像 New API（需要 controller/ router/ model/ middleware/）"
  [[ -f "$SRC/router/api-router.go" ]] || die "缺少 router/api-router.go"
  [[ -f "$SRC/model/main.go" ]] || die "缺少 model/main.go"
  [[ -f "$SRC/middleware/auth.go" ]] || die "缺少 middleware/auth.go"
}

already_has_full_stack() {
  if [[ -f "$SRC/controller/desktop.go" ]] && grep -qE 'CreateDesktopDeviceAuthorization|GetDesktopBootstrapV2' "$SRC/controller/desktop.go" 2>/dev/null; then
    return 0
  fi
  if grep -Rq --include='*.go' 'func IssueDesktopConnectCode' "$SRC/controller" 2>/dev/null \
     && ! grep -Rq --include='*.go' "$FILE_MARKER" "$SRC/controller" 2>/dev/null; then
    return 0
  fi
  return 1
}

patch_already_applied() {
  grep -Rq --include='*.go' "$FILE_MARKER" "$SRC" 2>/dev/null
}

print_version_hint() {
  local ver="未知"
  [[ -f "$SRC/VERSION" ]] && ver="$(tr -d '[:space:]' < "$SRC/VERSION" | head -c 64)"
  log "检测到源码：$SRC"
  log "go module ：$(detect_module)"
  log "VERSION   ：$ver"
  log "补丁包    ：$(tr -d '\r' < "$PAYLOAD_DIR/VERSION" | head -1)"
}

apply_helper() {
  local cmd="$1" file="$2"
  local out rc=0
  out="$(python3 "$HELPERS" "$cmd" "$file" 2>&1)" || rc=$?
  while IFS= read -r line; do
    [[ -z "$line" ]] && continue
    case "$line" in
      ok:*) ok "$file ← ${line#ok:}" ;;
      skip:*) ok "$file 已有 ${line#skip:}，跳过" ;;
      WARN:*) warn "$file：${line#WARN:}" ;;
      *) log "$line" ;;
    esac
  done <<< "$out"
  return 0
}

do_undo() {
  local latest
  latest="$(ls -1 "$SRC"/desktop-connect-backup-*.tgz 2>/dev/null | head -1 || true)"  # 最早一份=打补丁前
  [[ -n "$latest" ]] || die "找不到备份包 desktop-connect-backup-*.tgz，无法自动回滚"
  log "将用备份回滚：$latest"
  if [[ $ASSUME_YES -ne 1 ]]; then
    read -r -p "确认回滚？[y/N] " ans
    [[ "${ans:-}" == "y" || "${ans:-}" == "Y" ]] || die "已取消"
  fi
  if [[ $DRY_RUN -eq 1 ]]; then
    log "[dry-run] 会解压覆盖并删除补丁新增文件"
    exit 0
  fi
  tar -xzf "$latest" -C "$SRC"
  rm -f \
    "$SRC/model/desktop_session.go" \
    "$SRC/controller/desktop_connect.go" \
    "$SRC/controller/desktop_connect_page.go" \
    "$SRC/controller/desktop_connect.html" \
    "$SRC/middleware/desktop_bearer.go" \
    "$SRC/router/desktop_connect.go"
  ok "已回滚。请重新编译 / docker compose build && up"
  exit 0
}

[[ $UNDO -eq 1 ]] && do_undo

detect_repo
MODULE="$(detect_module)"
print_version_hint

if already_has_full_stack; then
  die "这个仓已经有完整桌面连接实现（不是本最小补丁）。不要用本脚本覆盖。若是野菜自有站点，无需再打补丁。"
fi

if patch_already_applied; then
  warn "检测到本补丁标记已存在（可能装过）。脚本仍可幂等重跑；想干净重来请先 --undo。"
fi

PLAN_COPY=(
  "model/desktop_session.go"
  "controller/desktop_connect.go"
  "controller/desktop_connect_page.go"
  "controller/desktop_connect.html"
  "middleware/desktop_bearer.go"
  "router/desktop_connect.go"
)
PLAN_EDIT=(
  "router/api-router.go"
  "model/main.go"
  "middleware/auth.go"
)
if [[ $SKIP_FRONTEND -eq 0 ]]; then
  if [[ -f "$SRC/router/web-router.go" ]]; then
    PLAN_EDIT+=("router/web-router.go")
  else
    warn "没有 router/web-router.go，连接页请自行把 payload/web/desktop/connect.html 挂到 /desktop/connect"
  fi
fi

log "将复制文件："
for f in "${PLAN_COPY[@]}"; do echo "    + $f"; done
log "将改动文件："
for f in "${PLAN_EDIT[@]}"; do echo "    ~ $f"; done

if [[ $DRY_RUN -eq 1 ]]; then
  ok "dry-run 结束，未写入任何文件。"
  cat <<'HINT'

正式打补丁后你还要做：
  1. 重新编译或：docker compose build && docker compose up -d
  2. HTTPS 打开 https://你的域名/desktop/connect
  3. 登录后点「打开桌面助手」，客户端能看到余额

回滚：desktop-connect-backup-时间戳.tgz 或 git checkout。
HINT
  exit 0
fi

if [[ $ASSUME_YES -ne 1 ]]; then
  read -r -p "确认对上述目录打补丁？[y/N] " ans
  [[ "${ans:-}" == "y" || "${ans:-}" == "Y" ]] || die "已取消"
fi

BAK=""
if patch_already_applied; then
  BAK="$(ls -1 "$SRC"/desktop-connect-backup-*.tgz 2>/dev/null | head -1 || true)"
  warn "已打过补丁：不新建备份（保留最早一份以便 --undo）。"
else
  TS="$(date +%Y%m%d-%H%M%S)"
  BAK="$SRC/desktop-connect-backup-${TS}.tgz"
  BACKUP_LIST=()
  for f in "${PLAN_EDIT[@]}"; do
    [[ -f "$SRC/$f" ]] && BACKUP_LIST+=("$f")
  done
  for f in "${PLAN_COPY[@]}"; do
    [[ -f "$SRC/$f" ]] && BACKUP_LIST+=("$f")
  done
  if [[ ${#BACKUP_LIST[@]} -gt 0 ]]; then
    tar -czf "$BAK" -C "$SRC" "${BACKUP_LIST[@]}"
    ok "已备份 → $BAK"
  else
    warn "没有可备份的已有文件（首次安装）；回滚时用 git 或删新增文件即可。"
    BAK=""
  fi
fi

install_payload_file() {
  local rel="$1"
  local srcf="$PAYLOAD_DIR/$rel"
  local dstf="$SRC/$rel"
  [[ -f "$srcf" ]] || die "补丁包缺文件：$rel"
  mkdir -p "$(dirname "$dstf")"
  if [[ "$rel" == *.go ]]; then
    sed "s|__NEW_API_MODULE__|${MODULE}|g" "$srcf" > "$dstf"
  else
    cp "$srcf" "$dstf"
  fi
  ok "写入 $rel"
}

for f in "${PLAN_COPY[@]}"; do
  install_payload_file "$f"
done

apply_helper api_router "$SRC/router/api-router.go"
apply_helper automigrate "$SRC/model/main.go"
apply_helper user_auth "$SRC/middleware/auth.go"
if [[ $SKIP_FRONTEND -eq 0 && -f "$SRC/router/web-router.go" ]]; then
  apply_helper web_router "$SRC/router/web-router.go"
  if ! grep -q 'func DisableCache' "$SRC"/middleware/*.go 2>/dev/null; then
    warn "未找到 middleware.DisableCache。若编译失败，编辑 web-router.go，去掉 DisableCache()，只留 ServeDesktopConnectPage。"
  fi
fi

if command -v go >/dev/null 2>&1; then
  log "尝试 go vet 补丁相关包（失败不阻断）…"
  if (cd "$SRC" && go vet ./model/ ./middleware/ ./controller/ ./router/ >/tmp/yeschoy-desktop-vet.log 2>&1); then
    ok "go vet 通过"
  else
    warn "go vet 有报错，见 /tmp/yeschoy-desktop-vet.log（私改仓常见）"
  fi
fi

cat <<EOF

════════════════════════════════════════════════════════
补丁已写入（草案安装器）。接下来请你本地完成：

【1. 重建】任选你平时的方式，例如：
  cd $SRC
  docker compose build && docker compose up -d
  # 或：go build -o new-api && 重启进程

【2. 反代】确认没有把下面路径吞进旧 SPA 规则：
  /desktop/connect
  /api/desktop/v2/*

【3. 验收】
  · HTTPS 打开 https://你的域名/desktop/connect
  · 登录后点按钮，能唤起野菜桌面端
  · 客户端能看到余额 / 用户信息（说明 Bearer 没漏）

【回滚】
  $0 --src $SRC --undo --yes
  # 备份包：${BAK:-（本次无旧文件备份，用 git 回退）}
  # 或：git checkout -- router/api-router.go model/main.go middleware/auth.go router/web-router.go
  # 并删除新增的 desktop_session.go / desktop_connect*.go / desktop_bearer.go / desktop_connect.html

对照文档：yeschoy-ai-desktop/docs/patch/new-api-desktop-connect/README.md
本安装器不自动部署生产站。
════════════════════════════════════════════════════════
EOF
