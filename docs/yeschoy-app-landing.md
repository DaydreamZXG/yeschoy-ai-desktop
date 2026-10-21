# yeschoy.app 落地页

面向开源与客户端用户的一页式站点，视觉对齐桌面端 `src/theme/new-api.css`
（青柠 `#c9fa10` + 近黑 `#111111`）。

详见 [`download-page.md`](./download-page.md) 的路径表与部署清单。

本地预览：

```bash
pnpm build:download && pnpm build:connect
pnpm exec vite preview --outDir public --port 4173
```

打开 `http://127.0.0.1:4173/`。部署到中转站静态目录（如 `/var/www/yeschoy-app`）需另开变更，不在本 PR 范围。
