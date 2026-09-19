# Stacker 开发与维护交接

## 当前状态

- 应用版本元数据仍为 `0.3.3`；`v0.3.3` 标签是已发布基线，当前 `main` 包含标签之后尚未单独发版的功能。
- 技术栈为 React 19 + TypeScript 6 + Vite 8 + Tauri 2 + Rust 2021，最低 Rust 版本为 1.77.2。
- 仅支持 Windows 10/11 x64。开发、测试和正式环境的数据目录由后端隔离。
- GitHub `origin` 与 Gitee `gitee` 是等价发布目标。源码提交、标签和发行资产必须保持一致；中文链接优先 Gitee，英文链接优先 GitHub。

## 模块边界

| 范围 | 前端入口 | Rust 模块 | 说明 |
| --- | --- | --- | --- |
| 编程生态与安装 | `src/pages/*`、`src/VersionManager.tsx` | `installer.rs`、`versions.rs`、各生态模块 | 发现、安装、切换和验证运行时及构建工具 |
| 依赖存储位置 | `src/StorageLocations.tsx` | `storage.rs`、`storage/maven.rs` | 管理 Maven、Gradle、npm、pnpm、pip、Composer、Go、Cargo、rustup 存储位置 |
| 智能体管理 · 安装更新 | `src/pages/Agents.tsx`、`src/features/agents`、`src/features/agent-tasks` | `agents/` | 注册表（`agents/registry.rs` 是智能体的唯一数据源）、健康检查、安装更新卸载修复、后台任务、一键更新 |
| 智能体管理 · 会话数据 | `src/pages/AgentData.tsx`、`src/features/sessions` | `sessions/` | 读取 Codex、Claude 会话元数据，项目归类，收藏，精简导出与批量删除 |
| 磁盘分析 | `src/features/space-analysis` | `space_analysis/` | 后台扫描、项目识别、空间变化、清理计划和提权执行 |
| 代理、源与设置 | `src/pages/Proxy.tsx`、`src/pages/Settings.tsx` | `proxy.rs`、`sources.rs`、`settings.rs` | 只管理明确选择的终端和开发工具配置，不检测或配置 TUN/VPN |
| 桌面生命周期 | `src/App.tsx`、`src/main.tsx` | `lib.rs`、`logging.rs` | 单实例、窗口恢复、托盘、首次关闭选择、统一日志和错误边界 |

IPC 命令集中注册在 `src-tauri/src/lib.rs`。新命令需要同时补充 Rust 错误语义、`src/invoke.ts` 调用、前端失败恢复和国际化文案。耗时扫描、清理和会话任务必须继续使用后台任务状态，不得阻塞 WebView。

## 本地数据与安全边界

- 日志、设置、备份、会话索引和任务报告使用 Tauri 解析出的应用数据目录；便携版使用程序旁的数据目录。不要把用户数据写进源码目录。
- 会话列表直接读取智能体自己的元数据，Stacker 只保存收藏、摘要和数据来源设置。分类规则与删除边界见 [sessions.md](sessions.md)。
- Git 凭据使用 Windows 凭据管理器；兼容摘要接口的可选密钥使用 Windows DPAPI。日志和诊断不得记录聊天正文、访问令牌、密钥或授权头。
- 存储位置迁移先备份配置，再复制到空目录，最后写入配置。旧目录始终保留，不能自动删除。
- 磁盘清理只能执行后端生成并再次校验的计划。未知目录、源码、配置、工作树和无法证明可重建的内容保持只读。

## 开发与验证

首次检出或清理依赖后执行：

```powershell
npm ci
npm run tauri dev
```

提交前完整检查：

```powershell
npm run lint
npm run typecheck
npm run test
npm run check:i18n
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

`sessions::catalog::tests::live_catalog` 和 `sessions::codex_rpc::tests::isolated_codex_delete` 默认忽略。前者只读列出本机会话数量；后者必须使用临时 `CODEX_HOME`，禁止指向真实用户数据。

## 构建、发布与清理

`npm run release:windows` 会校验版本元数据和国际化、执行前后端测试与 Clippy，并生成安装版、便携 ZIP 和 `SHA256SUMS.txt`。正式发布前必须同步更新 `package.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`resources/latest.json`，再将同一提交、标签、说明、资产和校验文件发布到 GitHub 与 Gitee。

以下目录全部可重建，不得提交：

- `node_modules/`：`npm ci` 恢复。
- `dist/`：`npm run build` 恢复。
- `src-tauri/target/`：Cargo 编译缓存，删除后首次 Rust 构建会重新编译。
- 根目录 `target/`：Codex App Server 协议生成结果，只是开发检查产物。
- `release/`：本地发行产物；只有明确需要交付时才保留安装包、便携 ZIP 和校验文件。
- `.idea/`、`src-tauri/gen/`、日志和各类 `*.tmp`：本机或生成内容。

不要清理用户配置目录、会话备份目录、项目工作树或未确认的扫描结果。对 Windows 路径执行批量删除前应先解析并核对绝对路径。

## 人工验收重点

1. 1280x720 默认窗口、960x540 最小窗口、窗口位置与尺寸恢复、恢复默认尺寸。
2. 首次关闭时选择退出或驻留托盘，之后可在设置中修改；单实例再次启动会唤醒现有窗口。
3. 深浅主题、中文和英文长文本、125%/150% DPI 下无截断或重叠。
4. 代理关闭、跟随 Windows 和手动地址三种模式，不应误判 TUN/VPN，也不应修改模式范围外的系统设置。
5. 各生态安装、切换、存储位置迁移、恢复默认和新终端生效；特别核对 PATH 中旧版本残留。
6. 智能体国内/国际产品检测、版本比较、安装更新失败恢复，以及 Kimi、TRAE、Qoder、DeepSeek Harness 的厂商变更。
7. 会话首次索引、增量刷新、重复与子会话处理、导出、摘要批准、批量预览、Codex 原生操作和备份。
8. 磁盘深度扫描、跳过路径、项目识别、`target-codex-*` 等独立构建目录、取消、提权和清理后复核。

## 已知限制与后续优先级

- 智能体厂商的安装入口、注册表名称和版本源会变化，产品检测需要持续用真实安装样本回归。
- 智能体安装、更新、卸载、修复已作为后台任务并行执行（`agents/tasks`），有一键更新与完成提示；其他生态页面的安装仍使用单任务弹窗。
- 新增或调整智能体只改 `agents/registry.rs`：产品、共享 CLI、版本、图标、进程特征和数据目录都在这里登记。
- WorkBuddy 中国版 / 国际版官网没有可静默安装的稳定直链，桌面端只提供官网下载；两版共用 CodeBuddy CLI。
- 会话摘要当前使用用户明确配置并批准的兼容接口。直接选择本机已安装智能体执行摘要尚未实现，也不能读取其他智能体的登录凭据。
- Claude 来源目前按已识别的本地记录只读处理；云端账号间迁移、厂商私有项目结构和完整原生删除语义不在当前能力范围。
- Stacker 可以导出会话、摘要和项目路径作为可阅读交接资料，但不能把一个厂商账号的云端会话无损写入另一个账号或另一个厂商。
- 正式发布前仍需在真实 Windows 桌面完成多 DPI、多显示器、断网、代理切换、休眠恢复、安装失败和升级回滚验收。
