# Stacker 开发与维护交接

## 当前状态

- 应用版本元数据为 `0.3.4`（首个签名发行版）；`v0.3.3` 标签是最近一次已发布基线。
- 技术栈为 React 19 + TypeScript 6 + Vite 8 + Tauri 2 + Rust 2021，最低 Rust 版本为 1.77.2。
- 仅支持 Windows 10/11 x64。开发、测试和正式环境的数据目录由后端隔离。
- GitHub `origin` 与 Gitee `gitee` 是等价发布目标。源码提交、标签和发行资产必须保持一致；中文链接优先 Gitee，英文链接优先 GitHub。

## 模块边界

| 范围 | 前端入口 | Rust 模块 | 说明 |
| --- | --- | --- | --- |
| 编程生态与安装 | `src/pages/*`、`src/VersionManager.tsx` | `installer.rs`、`versions.rs`、各生态模块 | 发现、安装、切换和验证运行时及构建工具 |
| 依赖存储位置 | `src/StorageLocations.tsx` | `storage.rs`、`storage/maven.rs` | 管理 Maven、Gradle、npm、pnpm、pip、Composer、Go、Cargo、rustup 存储位置 |
| 智能体管理 · 安装更新 | `src/pages/Agents.tsx`、`src/features/agents`、`src/features/agent-tasks` | `agents/` | 注册表（`agents/registry.rs` 是智能体的唯一数据源）、健康检查、安装更新卸载修复、后台任务、一键更新 |
| 智能体管理 · 会话数据 | `src/pages/AgentData.tsx`、`src/features/sessions` | `sessions/`、`webchat/`、`distill/` | 读取 Codex、Claude、CodeBuddy、WorkBuddy、Qoder、Kimi、MiMo 及桌面端自带的会话；项目归类、收藏、摘要与交接、精简导出、批量删除、跨电脑迁移、占用统计与数据目录迁移；网页对话与知识提炼 |
| 智能体管理 · 接口服务 | `src/pages/Gateway.tsx`、`src/pages/GatewayLogPage.tsx`、`src/features/gateway` | `gateway/`、`runner/` | 把已登录的智能体 CLI 以 OpenAI / Anthropic 风格接口提供给本机（可选局域网），见 [gateway.md](gateway.md) |
| 密钥保管 | `src/pages/Vault.tsx`、`src/features/vault` | `vault/` | 本机加密保管库（Argon2id + XChaCha20-Poly1305），恢复密钥、凭据管理器同步、SSH 密钥放置，配合浏览器插件保存网站密码 |
| Stacker AI | `src/features/ai` | `ai_config.rs`、`ai_features.rs` | 全局 AI 来源与推理强度；各页面的 AI 诊断、搜索与解释 |
| 磁盘清理 | `src/pages/Cleanup.tsx`、`src/features/space-analysis` | `space_analysis/`、`cleanup.rs`、`project_junk.rs` | 后台扫描、磁盘占用图、项目识别、大文件与重复文件、文件压缩、空间变化、清理计划、清理历史和提权执行 |
| 代理、源与设置 | `src/pages/Proxy.tsx`、`src/pages/Settings.tsx` | `proxy.rs`、`sources.rs`、`settings.rs` | 只管理明确选择的终端和开发工具配置，不检测或配置 TUN/VPN |
| 桌面生命周期 | `src/App.tsx`、`src/main.tsx` | `lib.rs`、`logging.rs` | 单实例、窗口恢复、托盘、首次关闭选择、统一日志和错误边界 |

第三方依赖补丁：`src-tauri/vendor/tiny_http` 是 tiny_http 0.12.0 的副本，经 `Cargo.toml` 的 `[patch.crates-io]` 生效，改动处标有 “Stacker patch”（未读完的请求体按固定小块丢弃并设上限；请求行与请求头数量设上限；请求头须在 15 秒内读完；连接空闲读超时 20 秒、写超时 30 秒；同时最多 64 个连接、同一地址最多 16 个），用来防止伪造 Content-Length 的请求让进程崩溃，以及慢速连接（slowloris）占满线程。升级 tiny_http 时需确认上游已修复或把补丁移植过去。

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

### 更新包校验与签名（必须）

应用下载到的安装包要过两关才会执行：先比 SHA-256，再用内置公钥验 minisign 签名，任一不符就删文件并拒装；**两样缺一个都不自动安装**，只让用户去发布页手动下载。

- 校验值挡的是下载损坏、上传错文件、发布资产被单独替换。
- 签名挡的是发布清单被篡改——下载地址和校验值都在清单里，改清单的人能一起改掉，但签名需要私钥。

**私钥只在发版这台机器上**，不进仓库、不进 CI（`.gitignore` 里已挡 `release-signing.key*`）。首次准备：

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --example release-key -- keygen "$env:USERPROFILE\.stacker\release-signing.key"
```

会提示设置密码（直接回车表示不加密），并打印一行公钥；把它填进 `src-tauri/src/update.rs` 的 `RELEASE_PUBLIC_KEY`。换密钥对就是重复这一步再换掉那一行——但换了之后，**老版本的 Stacker 就验不过新版本的签名**，那些用户只能手动下载一次。

发版前可以自检本机私钥和程序里的公钥是否配套：

```powershell
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored the_local_release_key_matches_the_built_in_one
```

`npm run release:windows` 会自动签名并把校验值和签名写回 `resources/latest.json`（私钥路径取 `STACKER_SIGNING_KEY`，默认 `%USERPROFILE%\.stacker\release-signing.key`；私钥有密码时会交互式询问）。因此发版时：

1. **先把版本号 bump 上去再跑脚本**——脚本会用刚构建出来的产物覆盖 `latest.json` 里的校验值和签名，在一个已发布的版本号上跑会把清单写成对不上的值。
2. `resources/latest.json` 的改动要**跟发布一起提交**（`npm run check:release-metadata` 会卡住缺失或格式不对的值）。
3. 上传前可以自检产物：`cargo run --manifest-path src-tauri/Cargo.toml --example release-key -- verify release/v<版本>/<安装包> release/v<版本>/<安装包>.minisig`（用的就是程序里内置的公钥）。
4. `SHA256SUMS.txt` 和两个 `.minisig` 要随安装包一起上传到 GitHub 与 Gitee 的 Release。发布后可用 `cargo test -- --ignored live_release_publishes_checksums` 核对。

因为私钥不在 CI 上，**发布由本机完成**，仓库里没有自动发布的工作流。构建产物在 `release/v<版本>/`，上传用：

```powershell
gh release create v0.3.4 (Get-ChildItem release\v0.3.4 -File | ForEach-Object FullName) --title "Stacker v0.3.4" --generate-notes
```

还没做的是 Windows 代码签名证书（Authenticode）。没有它，首次安装仍会触发 SmartScreen 提示；上面这套签名只保护自动更新，不解决 SmartScreen。

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
- WorkBuddy 中国版 / 国际版共用 CodeBuddy CLI。
- 不能完全无人值守的安装：Qoder 桌面端安装器拒绝静默安装，会自动改为弹出安装窗口；Hermes 桌面端没有静默模式，需要点 Install，克隆与本地构建约 40 分钟（窗口安装的时限为 60 分钟）；Kiro CLI 与 OpenClaw 桌面端（VC++ 运行库）需要 UAC 授权。
- MiniMax Code 国际版桌面端只在 Microsoft Store 发布，Windows 区域为中国时商店不提供，会提示改区域或从官网安装。
- 很多安装包来自 GitHub Release（`release-assets.githubusercontent.com`），下载速度取决于代理软件对该域名的分流；Stacker 不做国内地址绕行，这属于代理软件的职责。
- 会话摘要可以使用用户配置的兼容接口，也可以无状态地调用本机已登录的智能体 CLI；Stacker 不读取、不复制任何智能体的登录凭据。
- Claude 来源目前按已识别的本地记录只读处理；云端账号间迁移、厂商私有项目结构和完整原生删除语义不在当前能力范围。
- Stacker 可以导出会话、摘要和项目路径作为可阅读交接资料，但不能把一个厂商账号的云端会话无损写入另一个账号或另一个厂商。
- 正式发布前仍需在真实 Windows 桌面完成多 DPI、多显示器、断网、代理切换、休眠恢复、安装失败和升级回滚验收。
