# GitHub Actions 自动编译与 npm 发布指南（对齐 Termexo 架构）

参考 [gemron/Termexo](https://github.com/gemron/Termexo) 的跨平台发布与自包含 npm 分发架构，Termexo Fleet 使用以下模块化工作流实现 **Windows / macOS (Apple Silicon & Intel) / Linux (x64)** 与 **npm (`termexo-fleet`)** 联合自动发布：

## 工作流架构

| 工作流 | 文件 | 作用 |
| --- | --- | --- |
| **Release desktop and npm** | `.github/workflows/release.yml` | 主发布流水线：校验版本与前端 → 并行调用三平台构建 → 组装四平台内嵌载荷打包 `termexo-fleet-<ver>.tgz` → 校验所有二进制 SHA-256 → 上传草稿 Release → 发布 npm（OIDC 溯源或 Token）并验证 SHA-512 完整性 → 公开 GitHub Release |
| **Build Windows installers** | `.github/workflows/windows-build.yml` | `windows-2022` + MSVC 编译 NSIS EXE、MSI，校验 PE x64 与 MSI 架构，收集 Windows npm 载荷 |
| **Build macOS installers** | `.github/workflows/macos-build.yml` | `macos-15`（Apple Silicon）与 `macos-15-intel`（Intel）双机编译，ad-hoc 签名并校验 `lipo` / `codesign` / `hdiutil`，实测 macOS npm 解压启动链路 |
| **Build and test Linux desktop** | `.github/workflows/linux-build.yml` | `ubuntu-24.04` 启动 `dbus-run-session` + `gnome-keyring-daemon` 跑通全部 Rust 钥匙串测试，构建 `.deb`，校验 ELF64 x86_64 并打包保留 `usr/` 结构的 Linux npm 归档 |
| **CI** | `.github/workflows/ci.yml` | `main` 推送与 PR 自动校验前端、npm 启动器及三平台 Rust 工作区测试 |

## npm 内嵌多平台载荷（`packages/termexo-fleet/`）

与 `termexo` 一致，`termexo-fleet` 将 **Windows x64 (`termexo-fleet.exe`)、macOS Apple Silicon (`Termexo Fleet.app.tar.gz`)、macOS Intel (`Termexo Fleet.app.tar.gz`) 与 Linux x64 (`Termexo-Fleet.tar.gz`)** 直接打包在 `vendor/` 中：

- 用户执行 `npx termexo-fleet@latest` 或 `npm install --global termexo-fleet && termexo-fleet` 时，**无需运行时二次访问 GitHub 下载**，国内直接使用 `npmmirror` 即可高速启动。
- **Windows**：直接启动包内 `vendor/win32-x64/termexo-fleet.exe`。
- **macOS**：首次启动按当前架构解压到 `~/Library/Caches/Termexo-Fleet/npm/`（保留权限、符号链接与签名），通过 `/usr/bin/open -a` 启动。
- **Linux**：首次启动解压保留 `usr/bin/termexo-fleet` 目录结构到 `$XDG_CACHE_HOME/Termexo-Fleet/npm/`（默认 `~/.cache/Termexo-Fleet/npm/`），自动检测图形会话与 WebKitGTK 共享库依赖后启动。

## npm 首次配置（对齐 Termexo OIDC Trusted Publishing）

在 npm 包 `termexo-fleet` 的 **Settings → Trusted Publisher** 添加：

| 字段 | 值 |
| --- | --- |
| Publisher | `GitHub Actions` |
| Organization or user | `gemron` |
| Repository | `Termexo-Fleet` |
| Workflow filename | `release.yml` |
| Environment name | `npm` |
| Allowed actions | `Allow npm publish` |

> **说明**：如果 `termexo-fleet` 尚未在 npmjs.org 上存在，首次发版可先在 GitHub 仓库 **Settings → Environments → `npm`**（或 Repository Secrets）临时配置一次 `NPM_TOKEN` 完成首个版本发布，包创建后再在 npmjs.org 绑定上方 Trusted Publisher 并删掉 `NPM_TOKEN`，后续即可全自动零密钥 OIDC 发布。

## 发布新版本

```bash
node scripts/release-sync-version.mjs v0.1.1
git add -A
git commit -m "chore(release): v0.1.1"
git tag -a v0.1.1 -m "Termexo Fleet v0.1.1"
git push origin main --follow-tags
```

也可在 GitHub Actions → **Release desktop and npm → Run workflow** 手动触发，并可选择 `dry_run = true` 演练全流程而不发布，或选择 `publish_npm = false` 仅发布 GitHub Release。
