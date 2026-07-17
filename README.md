# Stacker

**Windows 开发生态与 AI 工作智能体管理器**

Stacker 将 Git、AI 工作智能体、编程语言运行时、构建工具、下载源、仓库镜像、终端代理、日志和开发缓存集中到一个 Windows 桌面应用中。它帮助开发者看清本机环境、完成版本维护，并为本地 AI 工作智能体提供可直接使用的环境摘要。

[简体中文](#简体中文) | [English](#english)

[![Release](https://img.shields.io/github/v/release/byteswalk/stacker?label=release)](https://github.com/byteswalk/stacker/releases/latest)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-2563eb)](#系统要求)
[![License](https://img.shields.io/github/license/byteswalk/stacker)](LICENSE)
[![Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24c8db)](https://tauri.app/)

## 简体中文

### 下载

从 [GitHub Releases](https://github.com/byteswalk/stacker/releases/latest) 获取最新版：

- **安装版**：带开始菜单、卸载入口和后续版本检测，适合日常使用。
- **免安装版**：解压后运行 `Stacker.exe`，适合临时测试和便携工具盘。
- **SHA-256 校验**：使用 Release 中的 `SHA256SUMS.txt` 校验下载文件。

当前版本：[Stacker v0.2.0](https://github.com/byteswalk/stacker/releases/tag/v0.2.0)

### 产品定位

AI 工作智能体可以修改代码、安装依赖、运行测试和构建项目，但它们仍依赖本机真实存在的 Git、Node.js、Python、Java、Go、Rust 和构建工具。Stacker 不替代这些工具，也不替代 Codex 或 Claude Code；它负责整理智能体运行所需的 Windows 开发生态，并将实际状态清晰地交给用户和智能体。

适用场景：

- 新 Windows 电脑的开发环境初始化。
- 多语言、多版本运行时的日常维护。
- AI 工作智能体运行前的环境检查与上下文准备。
- 企业代理、私有 Git 服务和受限网络下的开发配置。
- 下载源、包仓库、终端集成和开发缓存的集中管理。

### 核心能力

#### 生态环境体检

按需检查 Git、Node.js、Python、Java、Maven、Gradle、Go、Rust、包管理器、终端代理和开发缓存。结果以评分和可处理项呈现，不会在进入页面时强制执行耗时扫描。

#### AI 工作智能体

集中检测 Claude Code、Codex、Antigravity、OpenCode、ZCode、Kimi Code、WorkBuddy、Qoder、TRAE Work、OpenClaw 和 Hermes Agent 的 CLI 与桌面端状态。

- 检测已安装版本和可用更新。
- 在官方支持的情况下执行安装、更新、卸载和启动。
- 单独刷新某个智能体，不影响其他卡片。
- 生成仅包含已安装智能体的本机生态摘要，便于交给 AI 继续工作。

#### Git 与多账号执行环境

- 检测和安装 Git for Windows，识别 Git Bash 与 Git Credential Manager。
- 管理 GitHub、Gitee、GitLab、Gitea、Forgejo、GitHub Enterprise、阿里云云效 Codeup 及通用 HTTPS Git 服务账号。
- 访问令牌只保存到 Windows 凭据管理器，不写入项目文件或终端输出。
- 为每个账号打开隔离的 PowerShell、Git Bash 或 cmd 执行环境。
- 支持设置默认提交身份、初始化工程、仓库迁移以及复制账号操作摘要给 AI。

#### 运行时与构建工具

| 生态 | 主要能力 |
| --- | --- |
| Python | pyenv-win、运行时安装与扫描、默认版本、pip 镜像、终端集成 |
| Node.js | fnm、运行时安装与扫描、npm/pnpm/yarn 镜像、大文件下载镜像 |
| Java | JDK 扫描、安装、删除、默认版本、用户级或系统级环境变量 |
| Maven | 版本安装与扫描、默认版本、仓库镜像、代理、settings.xml |
| Gradle | 版本安装与扫描、Wrapper 下载源、仓库镜像、init.gradle |
| Go | SDK 安装与扫描、默认版本、用户级或系统级 GOPROXY |
| Rust | rustup 工具链、stable/beta/nightly、组件与 target、Cargo 源 |

所有生态页统一提供状态刷新、磁盘扫描、版本安装、默认版本切换、终端验证和“复制摘要给 AI”。删除运行时前会二次确认；删除当前默认版本时会同步处理相关环境配置。

#### 源管理

- 运行时下载源、包仓库镜像、构建工具仓库和大文件下载镜像分类维护。
- 支持测速、超时控制、手动应用、清除和自定义源。
- 支持导入、导出以及从服务器拉取新版公共源清单。
- 公共源清单更新不会覆盖本机自定义源。
- 后台可定期检查程序、源清单和生态版本更新。

#### 终端代理、磁盘与日志

- 统一配置终端代理和 `NO_PROXY`，并为已打开终端生成临时生效命令。
- 扫描开发工具缓存、Windows 临时目录及 JetBrains/Android Studio 历史版本。
- 关键配置修改前自动备份，可在“历史”页面查看详情并恢复。
- 日志级别可实时切换，支持打开日志目录、实时日志窗口、保留天数和手动清理。

#### 中英文界面

设置中可在简体中文和 English 之间即时切换。界面针对 Windows 缩放和较小可用工作区进行了响应式处理。

### 快速开始

1. 下载并启动 Stacker。
2. 在“生态环境体检”页面点击“开始体检”。
3. 进入有问题的生态页面，安装或扫描运行时并设置默认版本。
4. 对下载源或包仓库执行测速，确认后点击“应用”。
5. 在“AI 工作智能体”页面刷新状态，并按需复制本机生态摘要。
6. 需要恢复配置时，前往“历史”页面查看最近备份。

### 安全与隐私

- Stacker 不上传本机环境、项目内容或访问令牌。
- Git 访问令牌保存在 Windows 凭据管理器中。
- 系统级环境变量修改会触发 Windows UAC，由用户确认后执行。
- 关键配置写入前会创建本地备份。
- 安装包来自 GitHub Release，可使用随附 SHA-256 清单校验。

### 系统要求

- Windows 10 或 Windows 11，64 位。
- WebView2 Runtime。现代 Windows 10/11 通常已预装。
- 部分系统级操作需要管理员授权。
- 具体运行时和智能体可能有各自的系统要求与许可条款。

### 从源码构建

准备 Node.js、Rust stable、MSVC Build Tools 和 WebView2 开发环境。

```powershell
npm install
npm run tauri dev
```

执行完整检查：

```powershell
npm run lint
npm run test
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

生成 Windows 安装版、免安装版和校验清单：

```powershell
npm run release:windows
```

产物位于 `release/v<版本号>/`。发布前需保持 `package.json`、`Cargo.toml`、`tauri.conf.json` 和 `resources/latest.json` 的版本一致。

### 技术栈

- Tauri 2
- React 19
- TypeScript
- Rust
- Vite

---

## English

### Overview

Stacker is a Windows desktop application for managing local development ecosystems and AI work agents. It brings runtime versions, build tools, Git accounts, registries, mirrors, terminal proxy settings, logs, backups, and development cache cleanup into one interface.

AI agents can edit code and run commands, but they still depend on a working local toolchain. Stacker prepares that toolchain, reports its actual state, and produces a concise environment summary that can be handed to an agent before it starts working.

### Highlights

- On-demand health checks for Git, Node.js, Python, Java, Maven, Gradle, Go, Rust, proxies, and caches.
- Detection and lifecycle actions for 11 AI work-agent ecosystems, including Claude Code, Codex, Antigravity, OpenCode, OpenClaw, and Hermes Agent.
- Isolated Git account terminals for GitHub, Gitee, GitLab, Gitea, Forgejo, GitHub Enterprise, Alibaba Cloud Codeup, and generic HTTPS Git services.
- Version installation, discovery, default selection, deletion, and terminal verification across supported runtimes.
- Source catalog management with latency testing, custom sources, import/export, and remote catalog updates.
- Terminal proxy management, local backup and restore history, development cache cleanup, and configurable application logging.
- Simplified Chinese and English user interfaces.

### Download

Download the latest release from [GitHub Releases](https://github.com/byteswalk/stacker/releases/latest).

- **Installer**: recommended for daily use.
- **Portable package**: extract and run `Stacker.exe`.
- **Checksums**: verify artifacts with `SHA256SUMS.txt`.

Current release: [Stacker v0.2.0](https://github.com/byteswalk/stacker/releases/tag/v0.2.0)

### Quick Start

1. Launch Stacker and open **Environment Check**.
2. Start a health check and review actionable findings.
3. Open the relevant ecosystem page to install, discover, or select a runtime.
4. Test download sources and apply the preferred source explicitly.
5. Open **AI Work Agents** to refresh agent status and copy the installed environment summary.
6. Use **History** to inspect or restore backed-up configuration changes.

### Security

- Machine state, project content, and access tokens are not uploaded by Stacker.
- Git tokens are stored in Windows Credential Manager.
- System-level changes require explicit Windows UAC approval.
- Important configuration files are backed up before modification.
- Release artifacts include SHA-256 checksums.

### Build from Source

```powershell
npm install
npm run tauri dev
```

Run the release pipeline locally:

```powershell
npm run release:windows
```

### License

Stacker is released under the [MIT License](LICENSE).
