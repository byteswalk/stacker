![Stacker developer workstation banner](assets/brand/stacker-banner.png)

# Stacker

**A local-first Windows developer workstation manager for runtimes, AI coding agents, Git identities, package storage, network sources, and developer disk space.**

Stacker gives Windows developers one control surface for the infrastructure behind modern software work. Inspect the environment that is actually active, manage toolchains and work agents, keep Git accounts isolated, tune download sources, and find the build output and caches consuming local disks.

[Chinese documentation](README.zh-CN.md)

**Get Stacker:** [GitHub Releases](https://github.com/byteswalk/stacker/releases/latest) · [Gitee mirror](https://gitee.com/shaxiong/stacker/releases)

**Platform:** Windows 10/11 · **License:** [MIT](LICENSE) · **Desktop runtime:** [Tauri 2](https://tauri.app/)

## Why Stacker

AI coding tools can edit code and run commands, but they still depend on a healthy local workstation. Multiple projects and agents quickly produce conflicting runtimes, stale PATH entries, duplicated downloads, large browser bundles, package caches, and generated build directories.

Stacker manages that local layer without requiring a model connection or uploading project data:

- **Environment visibility** — verify the effective Git, Python, Node.js, Java, Maven, Gradle, Go, Rust, package-manager, proxy, and cache state.
- **Runtime lifecycle** — discover, install, switch, verify, and remove local toolchain versions.
- **Agent management** — inspect supported CLI and desktop products, distinguish regional editions, verify that installs actually run, and install, update, uninstall or repair them as parallel background tasks with one-click update.
- **Agent session data** — list the sessions of Codex, Claude, CodeBuddy, WorkBuddy, Qoder, Kimi, MiMo Code and the desktop apps as each client shows them, group them by project, summarize them or compose a handoff, move them to another computer, measure and relocate each agent's data folder, and delete them in bulk with a slim Markdown export.
- **API service** — offer signed-in agent CLIs (Codex, Claude, CodeBuddy, Qoder, Kimi Code, MiMo Code, Antigravity) to your own tools through OpenAI- and Anthropic-style endpoints, local by default with an optional LAN switch, a key on every request and a request log that never stores content.
- **Key vault** — keep API keys, tokens, SSH keys and website logins in a local encrypted vault with a recovery key; the browser extension can offer to save and fill website passwords.
- **Stacker AI** — set one AI source for the whole app; pages that can use a second opinion can diagnose, explain or search with it, and only what you ask about is sent.
- **Git account isolation** — use separate terminal contexts and repository-level commit identities for GitHub, Gitee, GitLab, Gitea, Forgejo, Codeup, enterprise, and generic HTTPS Git services.
- **Source and network control** — test latency, select download and repository sources, and preserve local custom sources. The proxy overview shows where terminal, Git, npm, Yarn, Maven and Gradle proxies come from; by default Stacker is hands-off and only ever syncs entries it wrote itself.
- **Package storage placement** — move or reset Maven, Gradle, npm, pnpm, pip, Composer, Go, Cargo, and rustup download or build stores without deleting the old location automatically.
- **Developer disk intelligence** — scan selected folders or disks, see them as a space map, recognize development projects and agent traces, find large and duplicate files, compress large files (transparent NTFS compression, H.265 video, JPEG/WebP/AVIF photos), review rebuildable artifacts by project, and remove only classified targets after confirmation, with a cleanup history.
- **Recoverable changes** — back up supported configuration before writing and restore it from local history.

## Product Tour

### Programming Ecosystem Check

Run an on-demand check of the commands, runtimes, package managers, build tools, proxy configuration, and developer caches that are actually effective on the workstation.

![Stacker programming ecosystem check](assets/screenshots/environment-check.png)

### Agent Install & Update

Review supported AI coding-agent CLI and desktop installations from one page. The catalog currently covers Claude Code, Codex, Antigravity, Grok Build, OpenCode, ZCode, Kimi, CodeBuddy, WorkBuddy, Qoder, TRAE, DeepSeek Harness, OpenClaw, Hermes Agent, pi, GitHub Copilot, Cursor, Factory, Kiro, MiniMax Code, Xiaomi MiMo, and Agnes Code, with China and international editions listed separately where vendors ship both. Installs, updates, uninstalls and repairs run as background tasks with a task center; a CLI is only reported healthy when it actually runs, and broken installs can be repaired with the command their own error names. Availability and automated lifecycle actions vary by vendor surface: a few vendor installers need a click in their own window or a UAC approval.

![Stacker AI work agents](assets/screenshots/work-agents.png)

### Agent Session Data

Reads each agent's own records directly — Codex's state database, Claude's desktop session index, and the stores of CodeBuddy, WorkBuddy, Qoder, Kimi Code, MiMo Code and the desktop apps — so titles, archive state and projects match the clients. Sub-agent runs fold into their parent session and automated runs stay hidden by default. Sessions can be summarized or composed into a project handoff with a local agent, moved to another computer, and exported; the space tab measures every agent's data folder and can move it to another drive. Bulk deletion is previewed and re-verified; by default each session is saved as slim Markdown first, Codex sessions are deleted through the Codex App Server, and Claude sessions still shown in the desktop sidebar are never deleted.

### API Service

Offers the agent CLIs you are already signed in to as OpenAI- and Anthropic-style endpoints for your own tools. Every request runs statelessly in an empty temporary folder with no tools, needs the key shown on the page, and is refused if it comes from a web page. The service listens on localhost unless you turn on LAN access. See the [API service guide](docs/gateway.md).

### Git Account Environments

Keep multiple Git service accounts available without changing a machine-wide default identity. Access tokens remain in Windows Credential Manager and are never included in summaries copied for AI.

### Developer Disk Analysis

Quick Scan checks known developer caches. Deep analysis accepts multiple folders or fixed disks, continues in the background, reports live progress, and shows the result as a space map with drill-down and Explorer access. Project analysis recognizes common stacks, agent traces, dependencies, build output, and dedicated agent build directories. Large and duplicate files can be reviewed and removed, and large files can be compressed: transparent NTFS compression, or re-encoding videos to H.265 and photos to JPEG/WebP/AVIF at a resolution you choose. Cleanup is available only for classified **Development Artifacts** and **Caches & Downloads**; every run requires confirmation, revalidates its targets, and is kept in the cleanup history.

![Stacker developer disk analysis](assets/screenshots/space-analysis.png)

## Supported Ecosystems

| Ecosystem | Capabilities |
| --- | --- |
| Git | Git for Windows detection and updates, isolated account terminals, project initialization, repository migration |
| Python | pyenv-win, runtime discovery and installation, default version, pip sources, terminal integration |
| PHP | PHP for Windows discovery and installation, default runtime, Composer lifecycle and sources |
| Node.js | fnm, runtime discovery and installation, npm/pnpm/yarn sources, large-download mirrors |
| Java | JDK discovery and installation, user or system `JAVA_HOME` and `PATH` |
| Maven | Version discovery, installation, repository mirrors, proxy configuration, `settings.xml` |
| Gradle | Version discovery, Wrapper download sources, repository mirrors, initialization scripts |
| Go | SDK discovery and installation, user or system `GOROOT` and `GOPROXY` |
| Rust | rustup toolchains, channels and pinned versions, components, targets, Cargo sources |

Storage-location controls are available where the ecosystem provides a stable user-level setting. Existing data can be copied to an empty destination, but Stacker deliberately preserves the old directory until the user verifies the new location.

## Security and Privacy

- Project files, machine summaries, and Git access tokens are not uploaded by Stacker. AI features send only what you ask about, and only to the AI source or local agent you chose.
- Session data is read locally; it reaches a model only when you ask for a summary, handoff or distillation.
- Git tokens are stored through Windows Credential Manager. The key vault encrypts its entries with Argon2id and XChaCha20-Poly1305; agent sign-in credentials are never read.
- The API service listens on localhost by default, requires its key on every request, and keeps no request or reply content.
- System-level environment changes and protected-directory scans require explicit Windows UAC approval.
- Uncertain disk items remain view-only; cleanup is limited to classified targets and requires confirmation.
- Supported configuration changes create local backups before writing.
- Release assets include SHA-256 checksums.

## Code Signing Policy

See the project [code signing policy](CODE_SIGNING.md) for the signing scope, build and approval process, project roles, privacy behavior, and release verification guidance.

Planned signing service (application pending): Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/). Stacker `v0.3.2` and earlier releases are currently unsigned.

## Download

Download the latest build from [GitHub Releases](https://github.com/byteswalk/stacker/releases/latest). If GitHub is slow or unavailable on your network, use the [Gitee release mirror](https://gitee.com/shaxiong/stacker/releases).

- **Installer** for regular desktop use.
- **Portable package** for temporary or removable-tool use.
- **`SHA256SUMS.txt`** for artifact verification.

## Requirements

- Windows 10 or Windows 11, 64-bit.
- Microsoft Edge WebView2 Runtime, included with most current Windows installations.
- Administrator approval only for operations that explicitly modify system-level state or scan protected paths.

## Build from Source

Install Node.js, Rust stable, MSVC Build Tools, and the WebView2 development runtime.

```powershell
npm ci
npm run tauri dev
```

Run project checks:

```powershell
npm run lint
npm run typecheck
npm run test
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Build the Windows installer, portable package, and checksum file:

```powershell
npm run release:windows
```

## Browser Chat Extension

A Chromium browser extension (Chrome, Edge, and other Chromium-based browsers) manages ChatGPT, Claude, Gemini, Grok and DeepSeek web conversations independently of the desktop app: organize, search, export, and delete them, with data kept on the local machine only. See the [extension guide](extension/README.md) for installation and usage.

## Project Documentation

- [Session data behavior and safety boundaries](docs/sessions.md)
- [API service (local gateway)](docs/gateway.md)
- [Development, verification, cleanup, and maintainer handoff](docs/development.md)

## License

Stacker is available under the [MIT License](LICENSE).
