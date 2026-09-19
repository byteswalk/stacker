# 智能体磁盘占用账本（子项目 C2）

日期：2026-09-18
状态：设计已确认，待实现

## 背景

用户最大的痛点是智能体数据占满磁盘，却分不清哪些有用。C1 解决了会话本身；本子项目列出智能体在磁盘上的全部占用，说明每一项是什么，并清理能证明无用的残留。

维护者机器实测（约 15 GB）：

| 位置 | 大小 | 构成 |
| --- | --- | --- |
| `~/.codex/sessions` | 9.1 GB | 主会话 5.4 GB、子智能体 3.6 GB；最大单个会话 992 MB，其中 773 MB 是 `compacted` 压缩快照 |
| `~/.codex` 其他 | 约 3.5 GB | `logs_2.sqlite` 667 MB、`thread_history_1.sqlite` 574 MB、`plugins` 472 MB、`.tmp` 316 MB、`generated_images` 310 MB、`target` 301 MB（在此目录内的编译产物）、`.sandbox-bin` 300 MB（10 个 `codex-command-runner-<版本>.exe`） |
| `%LOCALAPPDATA%\OpenAI\Codex` | 742 MB | `bin\<hash>` 是按内容寻址的不同组件（codex、node、rg、node_repl），`runtimes` 244 MB |
| `~/.claude` | 822 MB | `projects` 769 MB；`session-env` 471 条，多数无对应会话 |
| `%APPDATA%\Claude` | 603 MB | `claude-code\2.1.274`、`2.1.275` 各约 225 MB，浏览器缓存约 140 MB |
| `%LOCALAPPDATA%\Claude-3p` | 274 MB | 第三方模型版桌面端数据，其中 `claude-code` 254 MB |
| `%LOCALAPPDATA%\Claude\Logs` | 27 MB | 桌面端日志 |

`%APPDATA%\Claude` 与 `%LOCALAPPDATA%\Packages\Claude_<id>\LocalCache\Roaming\Claude` 是 MSIX 重定向的同一目录（文件 ID 相同），统计必须去重。

## 1. 模型

```rust
pub enum FootprintKind {
    Sessions,   // 会话记录：不在此删除，跳转到「会话」标签
    Reclaimable,// 可安全清理：能证明无用，默认勾选
    Review,     // 需你判断：说明后果，默认不勾选，逐项确认后可删
    Keep,       // 保留：只显示大小
}

pub struct FootprintItem {
    pub id: String,            // 规则 ID + 路径散列，稳定
    pub agent: Agent,          // Codex | Claude
    pub owner: Owner,          // Cli | DesktopApp | Shared —— 决定删除前需要退出哪个程序
    pub kind: FootprintKind,
    pub label: String,         // 「旧版 Claude Code 2.1.274」
    pub explain: String,       // 它是什么；删除后会怎样
    pub paths: Vec<String>,    // 一项可包含多个路径（如 400 个孤儿 session-env）
    pub bytes: u64,            // 去重后大小
    pub files: u64,
    pub blocked: Option<String>, // 当前不可清理的原因，如 E_APP_RUNNING
}

pub struct FootprintReport {
    pub agents: Vec<AgentFootprint>, // 每个智能体：total、各类合计、items
    pub total: u64,
    pub reclaimable: u64,
    pub scanned_at: u64,
    pub warnings: Vec<String>,       // 无法访问的目录等
}
```

## 2. 根目录

| 智能体 | 根目录 | 所属 |
| --- | --- | --- |
| Codex | C1 解析出的 Codex 根目录（`CODEX_HOME` 或 `~/.codex`） | Shared |
| Codex | `%LOCALAPPDATA%\OpenAI\Codex` | DesktopApp |
| Codex | `%LOCALAPPDATA%\Packages\OpenAI.Codex_*`（存在时） | DesktopApp |
| Claude | C1 解析出的 Claude 根目录（`CLAUDE_CONFIG_DIR` 或 `~/.claude`） | Shared |
| Claude | `%APPDATA%\Claude`、`%LOCALAPPDATA%\Claude`、`%LOCALAPPDATA%\Claude-3p`、`%LOCALAPPDATA%\Packages\Claude_*` | DesktopApp |

根目录下未被任何规则匹配的顶层条目归入一个「其他」项（Keep），保证合计等于根目录实际大小。

## 3. 规则

规则是静态表，每条：智能体、根目录、相对路径模式、类别、所属、标签与说明、可选判定函数。判定函数可以把条目改为 Keep 或设置 `blocked`。

Codex（`~/.codex`）：

| 路径 | 类别 | 判定 |
| --- | --- | --- |
| `sessions`、`archived_sessions` | Sessions | 同时报告其中压缩快照估算占比（取最大的 20 个会话抽样） |
| `state_*.sqlite*`、`goals_*`、`memories_*`、`queue_*`、`thread_history_*`、`config.toml`、`auth.json`、`plugins`、`skills`、`rules`、`memories`、`AGENTS.md` | Keep | — |
| `.sandbox-bin\codex-command-runner-<版本>.exe` | Reclaimable | 按版本号保留最新一个；其余可清理 |
| `cache` | Reclaimable | Codex 运行时阻止 |
| `.tmp` | Reclaimable | Codex 运行时阻止 |
| 根目录下的 `*.log`（如 `sandbox.<日期>.log`） | Reclaimable | 保留最近 7 天 |
| `logs_*.sqlite*` | Review | 说明：Codex 的运行日志数据库，删除后 Codex 会重建；需先退出 Codex |
| `tmp\<子目录>`、`target`、`generated_images`、`attachments`、`visualizations` | Review | 按子目录拆项；说明为智能体工作产物 |

Codex（`%LOCALAPPDATA%\OpenAI\Codex`）：`bin`、`runtimes` 为 Keep（按内容寻址的组件，不能按版本判断）。

Claude（`~/.claude`）：

| 路径 | 类别 | 判定 |
| --- | --- | --- |
| `projects` | Sessions | — |
| `file-history\<id>`、`session-env\<id>` | Reclaimable | 仅当 C1 目录中不存在会话 `<id>` 时；合并为「无对应会话的编辑检查点 / 环境快照」两项 |
| `shell-snapshots\*` | Reclaimable | 修改时间超过 7 天 |
| `cache`、`telemetry` | Reclaimable | — |
| `plugins`、`settings*.json`、`CLAUDE.md`、`plans`、`backups`、其他 | Keep | — |

Claude 桌面端（每个桌面端根目录）：

| 路径 | 类别 | 判定 |
| --- | --- | --- |
| `claude-code\<版本>` | Reclaimable | 保留版本号最大的和任何正在运行的 `claude.exe` 所在版本；桌面端运行时阻止 |
| `Cache`、`Code Cache`、`GPUCache`、`DawnGraphiteCache`、`DawnWebGPUCache` | Reclaimable | 桌面端运行时阻止 |
| `Logs\*.log` | Reclaimable | 保留最近 7 天 |
| `claude-code-sessions`、`IndexedDB`、`Local Storage`、`Session Storage`、配置文件、其他 | Keep | — |

## 4. 统计

- 每个根目录用现有 `space_analysis::walker` 的方式遍历：不跟随符号链接和目录联接，记录无法访问的目录为警告。
- 按 Windows 文件 ID（卷序列号 + 文件索引）去重；MSIX 重定向目录只计一次，显示在 `%APPDATA%` 路径下。
- 统计在后台线程执行，结果缓存到下次点击「重新统计」或执行清理后。
- 运行状态：用进程列表判断 Codex（`codex.exe`、`codex-code-mode-host.exe`、ChatGPT/Codex 桌面进程）和 Claude（桌面端 `Claude.exe`、`claude.exe`）是否在运行，取进程路径用于判断正在使用的版本。

## 5. 清理

- 用户勾选条目后预览：列出路径数、大小、被阻止项。默认勾选全部未被阻止的 Reclaimable；Review 项默认不勾选，勾选时显示其说明。
- 执行前重新统计所选条目，路径集合或大小变化超过 1% 时停止并要求重新预览。
- 删除规则与 C1 相同：不跟随链接；路径必须位于其根目录内；只删除预览中列出的路径。
- 应用在运行时对应条目被阻止，提示先退出，绝不强制关闭进程。
- 后台任务逐项报告：已释放、失败原因。完成后刷新账本。
- 不删除任何数据库的一部分；`logs_*.sqlite` 作为整组文件（含 `-wal`、`-shm`）删除。

## 6. 界面

「会话数据」页标签改为：会话 / 项目 / 占用 / 数据来源。

「占用」标签：

- 顶部：智能体数据共 X GB，其中可安全清理 Y GB；按钮「清理 Y GB」和「重新统计」。
- 每个智能体一组，组内按类别分段：会话记录、可安全清理、需你判断、保留。每行：勾选框（Keep 与 Sessions 无）、标签、说明、大小、被阻止原因；可展开看路径。
- 「会话记录」行显示「其中压缩快照约 Z GB」，点击跳转到「会话」标签并按占用排序（C1 查询增加 `sort: "bytes"`）。

## 7. 不在范围内

- 就地剔除会话内的旧压缩快照。
- 存储位置迁移（D）。
- 非 Codex / Claude 智能体的占用（后续按注册表扩展规则）。

## 测试

Rust（临时目录合成数据）：

- 规则匹配与分类；command-runner 与 claude-code 只保留最新及正在使用的版本。
- 孤儿 `session-env` / `file-history` 识别（依赖传入的会话 ID 集合）。
- 文件 ID 去重：硬链接与重复根目录只计一次。
- 未匹配条目进入「其他」，合计等于根目录大小。
- 清理：链接拒绝、越界拒绝、运行时阻止、预览后变化中止。

前端：分组与默认勾选、Review 项需要勾选确认、会话记录跳转。

手动验收：本机只读统计结果与背景表一致（Codex 约 13.7 GB、Claude 约 1.7 GB，MSIX 目录不重复）；清理旧 command-runner、旧 claude-code 版本和孤儿 session-env 后释放空间与预览一致。
