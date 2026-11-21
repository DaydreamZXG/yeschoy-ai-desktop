# Third-party code / 第三方代码

This project is released under the MIT License (see `LICENSE`). It contains
code from the following project, used under its own license.

本项目以 MIT 许可发布（见 `LICENSE`），其中包含下列项目的代码。

## CC Switch

- Source / 来源: <https://github.com/farion1231/cc-switch>
- License / 许可: MIT, Copyright (c) 2025 Jason Young
- Commit / 提交: `06082e189d65e6d6dbadc35dacdac1ce6c79d89a`

**What comes from it / 来自它的部分**

The protocol translation module in `src-tauri/src/proxy/`: 12 source files,
byte-for-byte identical to upstream. It converts between the OpenAI Responses,
OpenAI Chat Completions and Anthropic Messages formats. The exact file list and
checksums are in `src-tauri/src/proxy/VENDOR.md`, and a test fails if any of
them is edited.

`src-tauri/src/proxy/` 下的协议转换模块：12 个源文件，与上游逐字节相同，负责在
OpenAI Responses、OpenAI Chat Completions、Anthropic Messages 三种格式之间转换。
文件清单和校验值见 `src-tauri/src/proxy/VENDOR.md`，改动其中任何一个文件测试都会失败。

**How it is used / 怎么用的**

It is compiled into the app; users do not install or run CC Switch. Only two
files of ours call into it: `src-tauri/src/codex_responses_bridge.rs` (Codex)
and `src-tauri/src/claude_chat_bridge.rs` (Claude).

它被编译进客户端，用户不需要安装或运行 CC Switch。项目里只有两个文件调用它：
`src-tauri/src/codex_responses_bridge.rs`（Codex）和
`src-tauri/src/claude_chat_bridge.rs`（Claude）。

**History / 历史**

This repository started as a fork of CC Switch, so its git history contains
upstream commits. The copyright notice in `LICENSE` is kept for that reason.

本仓库最初从 CC Switch 分叉而来，git 历史里包含上游的提交，`LICENSE` 里因此保留了它的版权声明。
