# 下载与更新源：部署范围

`https://ergou.qzz.io` 是独立的只读文件源。第三方应用镜像与野菜自身自动更新使用不同目录、清单、发布器和信任边界；任何一边失败都不能改变另一边的状态。

## 固定部署位置

- 主机：用户指定的 `43.134.77.210`，Ubuntu 24.04.4。
- 配置：`/opt/yeschoy-download`；备份子目录只允许 root 读取。
- 公开内容：`/srv/yeschoy-download/public`，容器以只读方式挂载，发布前禁止符号链接。
- 容器：`yeschoy-download-origin`；使用宿主机已有 Caddy 2.8.4 镜像的固定 ID，不拉取或升级旧服务。
- 入口：保留 `/opt/ganfu/web/Caddyfile` 的原内容，只追加 `Caddyfile.ingress`。该文件以单文件 bind mount 挂载，替换时必须保留 inode，不能通过 rename 让容器读到旧文件。
- 无宿主机新端口；只加入现有 `web_default` 私网，由既有 Caddy 转发新域名。源容器无账号、签名密钥、Docker socket、写入挂载或访问日志配置。
- 保留非 root、只读和 no-new-privileges；能力集仅保留 `NET_BIND_SERVICE`。实测原镜像 `/usr/bin/caddy` 带此文件能力，全部移除会在 exec 阶段 EPERM；并不因此使用宿主机网络或开放新端口。[上游说明](https://github.com/caddyserver/caddy-docker/issues/396)

## 发布顺序

1. 只读检查共享入口、容器网络、镜像 ID、专用路径和旧网站 HTTP 状态。
2. 将既有入口备份到专用配置目录的 root-only 子目录，校验 SHA256。
3. 上传本目录中明确列出的配置和公开探针；不递归上传项目、日志或工作目录。
4. 验证源配置，启动独立容器，检查容器内健康、方法限制、当前更新频道状态、404 和 Range 响应。
5. 用 `ingress.py prepare` 生成候选配置；在既有入口容器环境内 validate，避免展开或记录其环境密钥。
6. `ingress.py promote` 在锁内再次检查原字节并保留 inode；运行 `caddy reload`，不得重建或停止共享入口。
7. 验证源站证书、公开域名 HTTPS、文件哈希和 Range；确认原有域名仍返回原来的状态。记录实际失败，不关闭 TLS 校验。

可运行 `python3 deploy/download-origin/probe.py` 检查公开域名，追加 `--origin-ip 43.134.77.210` 检查源站。两种模式均验证 TLS，显式忽略环境代理，不代表已经覆盖中国大陆各运营商网络。源站对穿越路径必须返回 404；Cloudflare 若先返回 400 拒绝此类 URL，公开测试会单独保留该实际状态，不要求源站接收到已被边缘拒绝的请求。

## 回退

只有当前入口字节仍等于本次候选时，才可使用 `ingress.py restore` 恢复备份，然后执行共享入口的 `caddy reload`。若存在别人的后续改动，停止并人工合并。先恢复入口，再停止本次新增容器；保留备份和文件，不执行广泛删除。

## 野菜自动更新频道

`/updates/stable.json` 在没有清单时返回空 HTTP 204；存在由 [self-update](../self-update/README.md) 原子发布的清单时才提供 HTTP 200。`/updates/releases/<version>/` 只保存不可变的签名构件。`/updates/beta.json` 保持同样的空频道兼容行为，尚未启用测试版推送。

健康字段 `updatesPublished` 只表达当前稳定清单是否具有公开权威，不代表某一客户端已经下载安装成功。检查可运行 `python3 deploy/download-origin/probe.py --expect-updates published`；回退后改用 `unpublished`。客户端公钥、真实递增版本、macOS/Windows 原生安装与重启回归仍是每次发布的门禁，不能只放一个 JSON 文件就宣布成功。

第三方安装包只从编译进同步器的厂商官方地址取得，并以原字节、SHA256 和固定包身份发布。技术发布器不再解析许可或批准文件；运营政策独立管理。无代理大陆网络、首次安装、登录和实际接入仍需分别测试。

服务器口令与签名私钥不得写入仓库、下载目录或操作说明。域名的 DNS/Cloudflare 控制台不在本次写入范围；如果当前解析或代理阻碍证书签发，应由域名管理员确认，不擅自降低加密模式。

## RU-053：第三方下载清单与远程停用

实现位于 [vendor-sync](../vendor-sync/README.md)：官方包私有同步、可选操作员原生签名验证和不可变原包发布是独立步骤。通过结构、来源、身份、发布者、大小和摘要检查后，发布器写入 `/apps/<sourceId>/<sha256>.<format>`，最后原子更新 schema-2 `/apps/catalog.json`。现有只读 `/apps/*` 路由即可提供这些文件，不需要给 Web 容器写权限或修改共享入口。

目录用 `native_verified` 表示已有匹配的操作员原生收据，用 `client_native_required` 表示必须由客户端完成原生信任判断。客户端无论看到哪一种都要核对 SHA256、大小、固定包身份和原厂签名；目录不可用、为空或文件失败时退回厂商官网。界面只显示实际来源和安装前验签，不展示运营或工程说明。

源站必须把 `.msix` 响应为 `application/vnd.ms-appx`、把 `.dmg` 响应为 `application/x-apple-diskimage`，并支持字节范围下载。只验证 HTTP 200/206 不够；错误的媒体类型会被安全下载器拒绝并触发官网回退。

操作员可原子发布空目录来停用全部第三方镜像；旧对象保留但不再具有目录权威，客户端无需升级便回退官网。逐来源重新发布即可恢复。该开关不影响 `/updates/*`。Windows 原生签名、首次安装和国内真实网络证据不能由单元测试或跨编译代替。
