# Code signing policy

This policy describes which Stacker artifacts may be code-signed, how they are built and approved, and what users can verify before installing them.

## Signing service

Planned signing service (application pending): Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by [SignPath Foundation](https://signpath.org/).

Stacker `v0.3.2` and earlier releases are currently unsigned. The project will identify signed releases explicitly after the SignPath application has been approved and signing has been integrated into the release workflow.

## Scope

Only official Windows release artifacts built from this repository and its versioned build scripts may be submitted for signing. The signing identity must not be used for development builds, local test builds, forks, third-party packages, or upstream tools downloaded and managed by Stacker. Upstream open-source files may be bundled only when their licenses permit redistribution; they are not submitted as standalone Stacker signing artifacts.

Official release artifacts currently include:

- the Windows installer;
- the portable Windows package;
- files directly required to distribute those packages.

## Build and approval process

- Source repository: [github.com/byteswalk/stacker](https://github.com/byteswalk/stacker)
- Read-only release mirror: [gitee.com/shaxiong/stacker](https://gitee.com/shaxiong/stacker)
- Build system: GitHub Actions on hosted Windows runners
- Release trigger: a version tag matching the application version
- Dependency installation: lockfile-based Node.js and Rust dependency resolution
- Verification: linting, metadata validation, frontend tests and build, Rust formatting, Clippy, Rust tests, and the Windows release build
- Integrity: each release publishes SHA-256 checksums for downloadable artifacts

Every signing request requires manual approval after the automated build and verification steps complete. Pull requests and untrusted forks do not receive signing credentials or signing approval rights.

## Project roles

Stacker is currently maintained as a single-maintainer open-source project:

- Committer and reviewer: [byteswalk](https://github.com/byteswalk)
- Signing approver: [byteswalk](https://github.com/byteswalk)

Changes from external contributors require maintainer review before merging. Repository and SignPath accounts with signing authority must use multi-factor authentication before production signing is enabled.

## Privacy and network behavior

Stacker is a local-first workstation management application. It does not provide telemetry and does not upload project files, environment summaries, local logs, or Git access tokens.

Stacker makes network requests only for features that require remote data or downloads, including:

- application, source-manifest, runtime, tool, and work-agent update checks;
- source latency tests and user-requested package downloads;
- Git service validation and repository operations explicitly initiated by the user.

Users can disable background update checks and can inspect or change configured download sources. Git access tokens are stored in Windows Credential Manager and are sent only to the selected Git service when validating credentials or performing an authenticated operation. Configuration backups, operation history, and diagnostic logs remain on the local machine.

Protected-directory scans and system-level environment changes require explicit Windows UAC approval. Disk cleanup is limited to classified targets selected by the user, requires confirmation, and revalidates targets before deletion.

## Release verification

Users should download Stacker only from the official [GitHub Releases](https://github.com/byteswalk/stacker/releases) page or the [Gitee release mirror](https://gitee.com/shaxiong/stacker/releases). Compare downloaded files with the published `SHA256SUMS.txt`. After signed releases become available, Windows signature details will identify the SignPath Foundation certificate chain described above.
