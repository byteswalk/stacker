# Stacker

**A local-first Windows developer workstation manager for runtimes, AI work agents, Git identities, network sources, and developer disk space.**

Stacker gives Windows developers one control surface for the infrastructure behind modern software work. Inspect the environment that is actually active, manage toolchains and work agents, keep Git accounts isolated, tune download sources, and find the build output and caches consuming local disks.

[Chinese documentation](README.zh-CN.md)

**Get Stacker:** [GitHub Releases](https://github.com/byteswalk/stacker/releases/latest) · [Gitee mirror](https://gitee.com/shaxiong/stacker/releases)

**Platform:** Windows 10/11 · **License:** [MIT](LICENSE) · **Desktop runtime:** [Tauri 2](https://tauri.app/)

## Why Stacker

AI coding tools can edit code and run commands, but they still depend on a healthy local workstation. Multiple projects and agents quickly produce conflicting runtimes, stale PATH entries, duplicated downloads, large browser bundles, package caches, and generated build directories.

Stacker manages that local layer without requiring a model connection or uploading project data:

- **Environment visibility** — verify the effective Git, Python, Node.js, Java, Maven, Gradle, Go, Rust, package-manager, proxy, and cache state.
- **Runtime lifecycle** — discover, install, switch, verify, and remove local toolchain versions.
- **AI work-agent lifecycle** — inspect supported CLI and desktop agents, refresh one agent independently, and copy an installed-agent summary for AI use.
- **Git account isolation** — use separate terminal contexts and repository-level commit identities for GitHub, Gitee, GitLab, Gitea, Forgejo, Codeup, enterprise, and generic HTTPS Git services.
- **Source and network control** — test latency, select download and repository sources, manage terminal proxy settings, and preserve local custom sources.
- **Developer disk intelligence** — scan selected folders or disks, drill into directory usage, locate large files, filter cleanup candidates by path, and remove classified rebuildable data with confirmation.
- **Recoverable changes** — back up supported configuration before writing and restore it from local history.

## Product Tour

### Environment Check

Run an on-demand check of the commands, runtimes, package managers, build tools, proxy configuration, and developer caches that are actually effective on the workstation.

![Stacker environment check](assets/screenshots/environment-check.png)

### AI Work Agents

Review supported AI coding and work-agent CLI or desktop installations from one page. Agent checks and lifecycle actions refresh only the affected card, so unrelated work stays responsive.

![Stacker AI work agents](assets/screenshots/work-agents.png)

### Git Account Environments

Keep multiple Git service accounts available without changing a machine-wide default identity. Access tokens remain in Windows Credential Manager and are never included in summaries copied for AI.

### Developer Disk Analysis

Quick Scan checks known developer caches. Deep analysis accepts multiple folders or fixed disks, continues in the background, reports live progress, and supports directory drill-down and Explorer access. Cleanup is available only for classified **Development Artifacts** and **Caches & Downloads**; every run requires confirmation and revalidates its targets.

![Stacker developer disk analysis](assets/screenshots/space-analysis.png)

## Supported Ecosystems

| Ecosystem | Capabilities |
| --- | --- |
| Git | Git for Windows detection and updates, isolated account terminals, project initialization, repository migration |
| Python | pyenv-win, runtime discovery and installation, default version, pip sources, terminal integration |
| Node.js | fnm, runtime discovery and installation, npm/pnpm/yarn sources, large-download mirrors |
| Java | JDK discovery and installation, user or system `JAVA_HOME` and `PATH` |
| Maven | Version discovery, installation, repository mirrors, proxy configuration, `settings.xml` |
| Gradle | Version discovery, Wrapper download sources, repository mirrors, initialization scripts |
| Go | SDK discovery and installation, user or system `GOROOT` and `GOPROXY` |
| Rust | rustup toolchains, channels and pinned versions, components, targets, Cargo sources |

## Security and Privacy

- Project files, machine summaries, and Git access tokens are not uploaded by Stacker.
- Git tokens are stored through Windows Credential Manager.
- System-level environment changes and protected-directory scans require explicit Windows UAC approval.
- Uncertain disk items remain view-only; cleanup is limited to classified targets and requires confirmation.
- Supported configuration changes create local backups before writing.
- Release assets include SHA-256 checksums.

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
npm install
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

## License

Stacker is available under the [MIT License](LICENSE).
