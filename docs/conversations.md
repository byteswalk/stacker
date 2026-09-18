# AI 智能体空间：会话管理

## 使用入口

进入「AI智能体空间」默认打开会话管理。首次进入自动建立本机索引，以后先显示已有索引；点击「刷新索引」重新核对新增、修改和移除的记录。扫描不会向任何模型发送内容，也不会运行历史命令。

- 「会话管理」：按来源、项目、时间、归档状态筛选；可选原文搜索；每页 40 条。
- 「项目资料」：查看单条摘要，将选中会话的摘要导出为项目交接材料。
- 「关联空间」：查看关联项目，进入已有磁盘扫描；高级入口保留环境约束与空间跟踪报告。
- 「数据来源」：配置 Codex、Claude Code CLI、Claude Desktop 本地 JSONL 根目录，或导入 Stacker 导出目录。删除来源只是停用索引入口，不删除原文件。

## 来源与能力

| 来源 | 本地阅读、搜索、导出、总结 | 修改源会话 |
| --- | --- | --- |
| Codex sessions / archived_sessions | 支持；优先读取本地原生标题 | 本机 CLI 接口通过能力检查后可归档、恢复、删除 |
| Claude Code CLI projects | 支持已识别的 JSONL；子智能体分开标识 | 只读，未验证完整原生删除语义 |
| Claude Desktop 本地 JSONL | 独立来源；仅识别的本地格式可读 | 只读；不代表已经接入桌面端全部历史 |
| Stacker JSON 导出目录 | 支持，不恢复官方客户端内部状态 | 不修改源记录 |

Codex 桌面端、CLI 或 IDE 可能共享 CODEX_HOME。来源中标出的客户端以持久化元数据为准，不能将 appServer 一概称为桌面端。云端、SSH、WSL 内部和未落盘记录不在本机扫描覆盖范围。

## 批量管理

勾选当前页或全部筛选结果，再执行操作。改变筛选条件会清空选择，避免操作旧筛选中的记录。

- 收藏、隐藏、项目分组仅存于 Stacker，不修改官方客户端。
- 归档、恢复、删除通过 Codex App Server，不直接修改 Codex 的 SQLite 数据库。
- 原生写操作要求完全退出 Codex 桌面端和 CLI。还在写入、过于新近、不完整、重复或范围未知的记录会被阻止。
- 预览同时核对本地父子关系与官方接口列出的后代。归档与删除都可能影响派生会话；恢复仅针对明确选中的归档会话。
- 确认前列出实际影响范围、路径、总大小和被阻止项目。预览有效期十分钟且只能执行一次。
- 执行时重算影响范围、验证内容摘要，先备份所有涉及的原始 JSONL 和可阅读导出，再从子会话到父会话调用原生接口并核对结果。
- 取消只停止后续项目，不撤销已完成的操作。超时或崩溃后不要假设未发生写入，先刷新并核对任务结果。不会自动恢复删除队列。

备份会继续占用磁盘，删除会话不一定减少总占用。备份位于「数据来源」展示的本地存储目录下 `backups/<操作ID>`。添加该目录为「Stacker 导出资料」可恢复阅读；不保证恢复官方会话状态。不删除源码、工作树、模型或构建产物。

## 总结与交接

在「数据来源」设置兼容 Chat Completions 的完整接口地址、模型名称和可选 API Key。远端必须 HTTPS；loopback 本机模型可以 HTTP。关闭跨站重定向，避免密钥被转发到其他服务。Windows 使用现有用户级 DPAPI 加密保存密钥，不读取其他智能体的登录凭据。

每次总结先展示模型、目标地址和实际发送内容。基础脱敏会处理常见密钥、授权头、密码及私钥，但不能保证识别所有秘密，用户仍须核对。允许发送后才调用模型；发送内容变化或模型地址变化需要重新确认。

摘要保留目标、结论、命令配置、待办、冲突及原文行号。历史文本只作为不可信资料，没有执行工具。原始记录变化后摘要显示过期。生成内容是模型的提炼，不是代码验收结论。

当前每次预览最多 20 条会话，每条最多 100,000 字符；超限不静默截断。网络请求有 120 秒上限；取消在当前请求结束后停止后续项目。尚未提供自动拆分超长会话、跨会话模型融合和云端同步。

导出原文会生成 JSON 阅读包与 Markdown，并为原生来源另存一份原始 JSONL；导出交接资料会将已有摘要、来源 ID 和原始项目路径整理为 `HANDOFF.md`，并标记缺失或过期摘要。这是可阅读资料迁移，不是将另一个智能体变成具有原生完整记忆的同一会话。

## 数据与诊断

- 新增数据隔离于 `%LOCALAPPDATA%/Stacker/dev/conversations` 或 `stable/conversations`。
- SQLite WAL 索引只保存元数据及 Stacker 注释，不建立第二份全文存储。原文在读取或全文搜索时按需解析。
- 大文件按读取与内存上限解析，受限记录显示「部分内容」。原文搜索仅覆盖能够解析的内容，不保证覆盖超限、损坏或无法读取的记录；这类记录不能执行原生删除或发送总结。
- 扫描增量依据文件大小和精细修改时间；破坏性操作另做 SHA-256 核对。
- 不能读取的目录、链接、未知格式和安全上限会列入扫描问题，不将读取失败当成空目录。
- 新版本索引拒绝被旧版本覆盖；索引损坏显式报错，不静默重置用户摘要。
- 共用应用日志，只记录操作 ID、阶段、计数和错误类别，不记录请求正文、聊天内容或模型密钥。

## 技术决策

继续使用现有 React + Tauri + Rust，不增加独立服务或每智能体插件。SQLite 通过 rusqlite（MIT）管理本地元数据；serde_json 解析原文；ureq 提供受限 HTTP 客户端；沿用应用现有 DPAPI、任务日志、文件选择器、主题和弹窗。

参考对象包括 [Agent Sessions](https://github.com/jazzyalex/agent-sessions) 和 [agent-session-search](https://github.com/benvenker/agent-session-search)。前者是另一套 macOS 应用，后者是 MCP/CLI 检索工具，不适合直接嵌入当前 Windows 桌面架构；本次不复制其代码。Stacker 的差异是把本地会话整理与已有安装、环境管理和磁盘核对串联，而不是再做一个通用智能体聊天客户端。

接口依据：[Codex App Server](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md)、[Claude SDK](https://code.claude.com/docs/en/agent-sdk/typescript)、[Claude Desktop 历史边界](https://code.claude.com/docs/en/desktop)。能力由本机协议生成结果确认，不以线上最新文档替代本机兼容检查。

## 回归命令

```powershell
npm test
npm run lint
npm run check:i18n
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml conversations -- --ignored --nocapture
```

最后一条是手动集成测试：一个只读抽样本机格式并输出计数；另一个只在临时 CODEX_HOME 中创建合成父子会话，测试本机 Codex 的归档、恢复和删除。禁止将破坏性回归测试指向用户实际 CODEX_HOME。
