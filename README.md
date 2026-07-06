# Stacker

Stacker 是一个面向 Windows 开发者的开发生态管理器。它用 Tauri 2 + React/TypeScript 做桌面界面，Rust 后端负责读写环境变量、包管理器配置、下载解压运行时、终端代理和缓存清理。

## 功能范围

- 运行时版本管理：Java、Python、Node.js、Go、Maven、Gradle、Rust。
- 包源/镜像切换：pip、conda、npm/pnpm、yarn、Go、Maven、Gradle、Cargo。
- Node / Python / Rust 分别接入 fnm、pyenv-win、rustup。
- 终端代理：写用户级 `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` / `NO_PROXY`，可选写 Maven/Gradle JVM 代理。
- 磁盘清理：扫描并清理常见开发缓存，删除范围限制在内置候选路径。
- 自定义私有源：密码用 Windows DPAPI 本地加密保存，应用时写入对应工具的原生配置。
- 方案与历史：保存/套用换源方案；换源、环境切换和终端集成写入前会生成可还原备份。

## 开发环境

- Windows 10 / 11。
- Rust toolchain，项目声明的最低版本为 `1.77.2`。
- Node.js + npm。
- Visual Studio Build Tools / MSVC 工具链。
- WebView2 Runtime。

安装依赖：

```powershell
npm install
```

仅启动前端 Vite：

```powershell
npm run dev
```

启动 Tauri 桌面应用：

```powershell
npm run tauri dev
```

构建前端：

```powershell
npm run build
```

静态检查：

```powershell
npm run lint
cd src-tauri
cargo check
```

打包桌面应用：

```powershell
npm run tauri build
```

## 本地数据

Stacker 会写入当前 Windows 用户的数据目录，主要包括：

- `%APPDATA%\stacker\settings.json`
- `%APPDATA%\stacker\profiles.json`
- `%APPDATA%\stacker\custom_sources.json`
- `%APPDATA%\stacker\backups\...`
- `%APPDATA%\stacker\mirrors.json`

下载的 fnm / pyenv-win / JDK / Maven / Gradle / Go 默认放在 Stacker 程序所在目录下的子目录中，便于集中管理。

## 安全边界

这个应用的产品目标就是管理本机开发环境，所以会执行真实系统改动：

- 写 HKCU 用户级环境变量和 PATH。
- 用户选择系统级切换时，通过 UAC 提权写 HKLM 环境变量和系统 PATH。
- 写包管理器配置文件，例如 `.npmrc`、`pip.ini`、`settings.xml`、`init.gradle`、Cargo 配置。
- 写 shell 集成：PowerShell profile、Git Bash `.bashrc`、cmd AutoRun。
- 必要时调整 PowerShell CurrentUser 执行策略，让 profile 能正常运行。

改动前会尽量写入历史备份；系统级环境还原同样需要管理员权限。

## 维护注意

- 当前工作副本没有 Git 元数据。正式开源前需要在根目录重新初始化仓库。
- `src-tauri/target`、`dist`、`node_modules` 都是生成物，不应提交。
- `src-tauri/src/update.rs` 中 `APP_REPO` 已指向 `byteswalk/stacker`；发布 GitHub Releases 后，应用内“检查更新”会使用最新 release。
- README、许可证、应用标识、仓库地址和发布流程稳定后再打第一个公开 tag。

开源发布步骤见 [docs/OPEN_SOURCE.md](docs/OPEN_SOURCE.md)。
