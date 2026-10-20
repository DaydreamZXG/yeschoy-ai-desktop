# New API 一键桌面连接补丁（草案）

站长入口：

```bash
./install-newapi-desktop-connect.sh --src /path/to/new-api --dry-run
./install-newapi-desktop-connect.sh --src /path/to/new-api --yes
```

中文说明见上一级 [`A-一键补丁脚本.md`](../A-一键补丁脚本.md)。

- 不自动部署、不 `docker up`、不推上游。
- 野菜自有完整桌面栈会被拒绝，避免误覆盖。
- 回滚：`./install-newapi-desktop-connect.sh --src … --undo --yes`
