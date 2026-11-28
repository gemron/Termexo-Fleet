# 短视频与社交媒体全平台发布包（Termexo Fleet v0.1.1）

> 成片素材路径：
> - **9:16 手机竖屏大字版视频**：`D:\devlop\Termexo-Fleet\docs\promo\termexo-fleet-promo-vertical.mp4`（`1080×1920 @ 60fps · 45s`）
> - **9:16 竖屏封面**：`D:\devlop\Termexo-Fleet\docs\promo\termexo-fleet-promo-vertical-cover.png`
> - **16:9 横屏高清视频**：`D:\devlop\Termexo-Fleet\docs\promo\termexo-fleet-promo.mp4`（`1920×1080 @ 60fps · 45s`）
> - **16:9 横屏封面**：`D:\devlop\Termexo-Fleet\docs\promo\termexo-fleet-promo-cover.png`

---

## 1. 抖音 / 微信视频号 / 小红书（上传 9:16 竖屏大字版视频）

### 推荐标题（任选 1 条）
- 开源了一个本地 AI 编程舰队指挥台：多账号自动切号、L1–L4 省费路由、Claude 写完自动叫 Codex 异构审
- 跑 Claude Code 和 Codex 总被限流烧额度？我用 Rust 写了个零黑盒的多机调度台（MIT 开源）
- 多台电脑一起跑 AI 编程 Agent！45 秒看懂本地优先、零黑盒的 Termexo Fleet

### 正文简介
同时用 Claude Code、Codex CLI、OpenCode 跑多账号和多台机器，最怕烧额度、429 限流中断和黑盒看不到终端。
这是我维护的 MIT 开源桌面端调度中枢 **Termexo Fleet v0.1.1**（Rust + Tauri 2）：
- 零黑盒真实 PTY 终端：随时点开实时日志，一键 `同意 (y)` 放行审批
- L1–L4 难度天梯：轻量改文档走低成本模型，安全审查强制跨模型家族（Claude 写 → Codex 审）
- 反向 MCP 自主派生子 Agent 蜂群：独立 Git Worktree 隔离 + 深度预算护栏
- 支持 Windows / macOS / Linux，一行命令即可体验：`npx termexo-fleet@latest`
（注：需搭配本地/局域网 Termexo 与自有 Agent CLI 账号使用）

### 置顶评论（发完立刻自评置顶）
开源仓库搜 GitHub：`gemron/Termexo-Fleet`
官网演示与下载：`termexo.com/#fleet`
Node.js 18.18+ 一行体验：`npx termexo-fleet@latest`
欢迎提 Issue 反馈多机实战问题，觉得有用点个 Star 鼓励一下！

### 话题标签
`#AI编程` `#ClaudeCode` `#Codex` `#开源项目` `#程序员工具` `#多Agent` `#MCP` `#Rust` `#效率工具`

---

## 2. B 站 / YouTube（上传 16:9 横屏 1080p60 视频）

### B 站标题
【MIT 开源】本地多机 AI 编程舰队指挥中枢 Termexo Fleet v0.1.1：零黑盒终端、L1–L4 路由与异构交叉审

### 分区与标签
- 分区：`科技 → 软件应用`（或 `计算机技术`）
- 标签：`AI编程`、`Claude Code`、`Codex`、`开源软件`、`MCP`、`多Agent`、`Rust`、`Tauri`、`程序员`、`效率工具`

### 简介与章节时间轴
我是 Termexo Fleet 维护者，这是一个基于 Rust + Tauri 2 + Angular 22 的本地优先 AI 编程多机调度中枢（MIT 开源），现已发布 v0.1.1，支持 Windows x64 / macOS（Apple Silicon & Intel）/ Linux x64。

- 一行启动：`npx termexo-fleet@latest`（Node.js 18.18+）
- GitHub 仓库：https://github.com/gemron/Termexo-Fleet
- v0.1.1 下载：https://github.com/gemron/Termexo-Fleet/releases/tag/v0.1.1
- 官网展示：https://www.termexo.com/#fleet

时间轴：
00:00 多账号多机器跑 AI 编程的 4 个痛点
00:05 Termexo Fleet 登场
00:09 任务指挥甲板、实时路由预判与三阶段 DAG 流水线
00:17 零黑盒滑出式 PTY 终端与人工一键放行
00:22 L1–L4 难度天梯与跨模型家族异构审查
00:28 反向 MCP 自主子 Agent 蜂群（Git Worktree 隔离）
00:31 Windows + macOS 多机星型算力拓扑
00:34 本地 SQLite 效能账本与 Ctrl+K 全局命令面板
00:39 跨平台获取方式（npx termexo-fleet@latest）

前提说明：需搭配运行中的 Termexo 桌面端及用户自备的 Agent CLI/账号使用；Windows 安装包未商业签名，macOS 为 ad-hoc 临时签名。

---

## 3. X (Twitter) 英文发布帖（配 16:9 或 9:16 45s 视频）

### Tweet 1（主帖）
Just shipped **Termexo Fleet v0.1.1** — an MIT open-source, local-first desktop commander for orchestrating Claude Code, Codex CLI & OpenCode across machines and accounts.

- Zero black-box: inspect & approve every live PTY terminal
- L1–L4 difficulty routing + cross-family L4 audits (Claude writes → Codex reviews)
- Reverse MCP sub-agent swarms in isolated Git Worktrees
- Self-healing 429 failover + local SQLite cost ledger

Windows, macOS (Apple Silicon & Intel) & Linux in one command:
`npx termexo-fleet@latest`

GitHub: https://github.com/gemron/Termexo-Fleet

### Tweet 2（自评补充架构与边界）
Built with Rust Workspace + Tauri 2 + Angular 22. All tokens stay in your OS Keyring, all state stays in local SQLite.

Note: requires a running Termexo desktop instance + your own coding-agent CLIs/accounts. Windows installers are currently unsigned and macOS builds use ad-hoc signing.

Release & downloads: https://github.com/gemron/Termexo-Fleet/releases/tag/v0.1.1
45s tour: https://www.termexo.com/#fleet

---

## 4. 微博中文短帖（配 9:16 竖屏视频或 16:9 封面图）

刚发布了 MIT 开源项目 Termexo Fleet v0.1.1，一个本地优先的多机 AI 编程 Agent 调度中枢（Rust + Tauri 2）：

1）零黑盒实时 PTY 终端，随时旁观 Claude Code / Codex 输出并一键放行审批
2）L1–L4 难度天梯路由：轻量改文档走低成本模型，L4 安全审查自动排除上游作者模型家族（Claude 写 → Codex 审）
3）反向 MCP 自主派生子 Agent 蜂群，独立 Git Worktree 隔离 + 深度/预算护栏
4）429 限流自动冷却切号 + 本地 SQLite 效能省费账本

支持 Windows / macOS（M 芯片 & Intel）/ Linux，Node.js 18.18+ 一行体验：npx termexo-fleet@latest
（需搭配本机或局域网 Termexo 与自有 Agent CLI 使用）

GitHub：https://github.com/gemron/Termexo-Fleet
演示：https://www.termexo.com/#fleet
#开源项目# #AI编程# #ClaudeCode# #程序员#
