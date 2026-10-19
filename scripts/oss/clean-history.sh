#!/usr/bin/env bash
# 生成一个「全新历史」的开源仓库副本。只在本地生成，不推送、不改动当前仓库。
#
# 用法：scripts/oss/clean-history.sh <输出目录> [源分支，默认 HEAD]
# 之后人工检查输出目录，确认无误再自己决定推到新的公开仓库。
set -euo pipefail
out="${1:?usage: clean-history.sh <output-dir> [ref]}"
ref="${2:-HEAD}"
repo="$(git rev-parse --show-toplevel)"
if [ -e "$out" ]; then
  echo "输出目录已存在：$out（为安全起见不覆盖）" >&2
  exit 1
fi
mkdir -p "$out"
# 1. 只导出指定版本的文件快照（不带任何历史、不带 .git）
git -C "$repo" archive --format=tar "$ref" | tar -x -C "$out"

# 2. 再保险：删除公开仓库不该出现的内部目录/文件（即使快照里已经没有）
rm -rf "$out/outputs" "$out/.product-governance" "$out/deploy/download-origin" \
       "$out/deploy/vendor-sync" "$out/.cursor" "$out/.claude" \
       "$out/session-manager.md" "$out/.cursor-agent-tmp-notes.md" "$out/.tmp-agent-notes.md"
find "$out" -name 'deployment-receipt*.json' -delete
find "$out" -name '*.env' ! -name '*.example.env' -delete

# 3. 扫描：内部地址与常见密钥形态（只报文件名和行号，不打印匹配内容）
echo "== 内部地址/密钥形态扫描（只列位置） =="
grep -rnIE --exclude-dir=node_modules \
  '43\.134\.[0-9]+\.[0-9]+|sk-[A-Za-z0-9]{32,}|ctx7sk-|ghp_[A-Za-z0-9]{30,}|AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY' \
  "$out" | cut -d: -f1,2 || echo "（无匹配）"
if command -v gitleaks >/dev/null 2>&1; then
  gitleaks detect --no-git --source "$out" --redact || true
else
  echo "提示：安装 gitleaks 后会自动做更完整的密钥扫描。"
fi

# 4. 初始化全新历史：一个提交
git -C "$out" init -q -b main
git -C "$out" add -A
git -C "$out" -c user.name="${OSS_AUTHOR_NAME:-YesChoy}" -c user.email="${OSS_AUTHOR_EMAIL:-oss@example.invalid}" \
  commit -q -m "Initial open-source release"
echo "完成：$out （单个初始提交，未推送）"
