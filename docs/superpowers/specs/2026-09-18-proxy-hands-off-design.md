# 代理「不干预」与代理总览（子项目 E）

日期：2026-09-18
状态：已实现

## 背景

现状问题（代码核实）：

- 每次启动执行 `settings_set_proxy_mode(已保存模式)`，默认模式为「跟随系统」。Windows 未开启系统代理时（TUN/VPN 模式、代理软件尚未启动），`sync_existing_explicit_proxies(None)` 会清除 Git 全局 `http.proxy`/`https.proxy`、`.npmrc`/`.yarnrc` 代理行、Maven/Gradle 代理、`gradle.properties` 与 `MAVEN_OPTS` 代理；终端代理开启时还会删除用户环境变量 `HTTP_PROXY`/`HTTPS_PROXY`/`ALL_PROXY`/`NO_PROXY`。
- 判断「已存在」只看有没有值，不区分是否由 Stacker 写入，用户自己配置的代理也会被清除。

## 1. 模式

| 模式 | 外部配置 | Stacker 自身联网 |
| --- | --- | --- |
| `hands_off` 不干预（默认） | 永不自动写入或清除，启动不同步；只在用户点击时写 | 读取 Windows 系统代理（只读），有则使用 |
| `system` 跟随系统 | 启动时与点击「同步」时，把 Stacker 管理的条目同步到 Windows 系统代理地址 | 同上 |
| `manual` 手动 | 同上，使用用户填写的地址 | 使用用户填写的地址 |

升级：已保存的 `system`（旧默认）与 `off` → `hands_off`；`manual` 保持。模式与地址在「终端代理」页设置，偏好设置页移除代理区块。

## 2. 归属账本

`settings.json` 新增 `proxy_managed: { <位置>: <Stacker 写入的值> }`：

- Stacker 写入某位置后记录写入值；用户点击「清除」后从账本移除。
- 同步（跟随系统 / 手动）只处理账本中的位置，且仅当当前值等于记录值；不相等说明被用户或其他工具修改 → 从账本移除，今后不再触碰。
- 跟随系统而 Windows 暂无代理地址：清除账本中未被修改的条目，记录值置空（仍归 Stacker 管理）；之后有地址时重新写入。
- 升级前已有的配置全部视为外部设置。

## 3. 位置

| 位置 ID | 内容 | 读 / 写 / 清 |
| --- | --- | --- |
| `windows` | Windows 系统代理 | 只读 |
| `env` | 用户环境变量 `HTTP_PROXY`、`HTTPS_PROXY`、`ALL_PROXY`、`NO_PROXY` | 值取 `HTTP_PROXY`；写入沿用现有 `proxy::enable`，清除沿用 `proxy::disable` |
| `git` | Git 全局 `http.proxy`、`https.proxy` | `git config --global` |
| `npm` | `~/.npmrc` 的 `proxy`、`https-proxy` | 行级替换，保留其他行 |
| `yarn` | `~/.yarnrc` 的 `proxy`、`https-proxy` | 同上 |
| `maven` | `~/.m2/settings.xml` 中 Stacker 代理（`stacker-http`/`stacker-https`），保留镜像配置 | 沿用 `maven_apply` |
| `maven_opts` | 用户环境变量 `MAVEN_OPTS` 中的 `-Dhttp(s).proxy*` | 沿用现有函数，保留其他参数 |
| `gradle` | Stacker 的 Gradle 初始化脚本中的代理，保留镜像配置 | 沿用 `gradle_apply` |
| `gradle_props` | `~/.gradle/gradle.properties` 中的 `systemProp.http(s).proxy*` | 沿用现有函数 |

值统一规范化为 `host:port` 比较。

## 4. 页面「终端代理」

- 顶部：模式（不干预 / 跟随系统 / 手动）、当前生效地址（手动模式可编辑）、「同步」按钮（跟随系统 / 手动时）。
- 代理总览：每个位置一行，显示当前值、来源（Stacker 管理 / 外部设置 / 未设置）、「写入当前地址」「清除」。清除外部设置需要二次确认。
- 保留「直连白名单 NO_PROXY」与「让已打开的终端立即生效」区块。

Git 页与镜像源页中已有的写入 / 清除代理操作改为经过账本（写入记录、清除移除）。

## 5. 删除

`proxy::sync_existing_explicit_proxies`、`sources::sync_existing_tool_proxies`、`git::sync_existing_proxy` 与启动时的 `settings_set_proxy_mode` 调用删除，由 `proxy_ledger::reconcile()` 取代。

## 测试

- 不干预模式下启动对账不写任何位置。
- 账本：值被改过的位置跳过并移出账本；系统代理消失时清除但保留归属，恢复后重新写入。
- 模式迁移。
- 每个位置在临时 HOME 下的读写清（Git 使用 `GIT_CONFIG_GLOBAL` 指向临时文件）。
