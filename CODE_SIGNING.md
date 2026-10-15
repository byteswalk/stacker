# Release signing and verification

Stacker releases are not Authenticode-signed, and the project does not plan to add Authenticode signing. Windows SmartScreen may warn the first time the installer runs; verify the download as described below, then choose **More info → Run anyway**.

## What a release contains

- `Stacker-<version>-setup-windows-x64.exe`, the Windows installer. It is the only package; portable packages ended with v0.3.4, and the built-in updater installs only the installer.
- `SHA256SUMS.txt` with the installer's SHA-256 checksum.
- `Stacker-<version>-setup-windows-x64.exe.minisig`, a [minisign](https://jedisct1.github.io/minisign/) signature over the installer.

The same version, notes, files and checksums are published on [GitHub Releases](https://github.com/byteswalk/stacker/releases) and the [Gitee release mirror](https://gitee.com/shaxiong/stacker/releases).

## How releases are built

- Source repository: [github.com/byteswalk/stacker](https://github.com/byteswalk/stacker), mirrored to [gitee.com/shaxiong/stacker](https://gitee.com/shaxiong/stacker).
- The maintainer builds each release locally from the tagged commit with `npm run release:windows`, which first runs release-metadata, internationalization, lint, frontend tests, Rust formatting, Rust tests and Clippy.
- The signing key stays on the maintainer's machine and never goes to the repository or CI. Its public key is compiled into Stacker.
- Maintainer, reviewer and release approver: [byteswalk](https://github.com/byteswalk). Changes from external contributors require maintainer review before merging.

## How updates are checked

Before running a downloaded update, Stacker compares its SHA-256 with the published checksum and verifies the minisign signature with the built-in public key. If either is missing or does not match, the file is deleted and the update is not installed; the user is pointed to the release page instead.

## Verifying a download yourself

Download Stacker only from the two release pages above. Compare the installer with `SHA256SUMS.txt`:

```powershell
Get-FileHash .\Stacker-<version>-setup-windows-x64.exe -Algorithm SHA256
```

With minisign installed, also check the signature:

```powershell
minisign -Vm Stacker-<version>-setup-windows-x64.exe -P RWRFeBheM9RE7hJRWngACNiP4WwTCpVuaU+wHV0QyARJPayvTHGs1Qmv
```

## Privacy and network behavior

Stacker is a local-first workstation management application. It does not provide telemetry and does not upload project files, environment summaries, local logs, or Git access tokens.

Stacker makes network requests only for features that require remote data or downloads, including:

- application, source-manifest, runtime, tool, and work-agent update checks;
- source latency tests and user-requested package downloads;
- Git service validation and repository operations explicitly initiated by the user.

Users can disable background update checks and can inspect or change configured download sources. Git access tokens are stored in Windows Credential Manager and are sent only to the selected Git service when validating credentials or performing an authenticated operation. Configuration backups, operation history, and diagnostic logs remain on the local machine.

Protected-directory scans and system-level environment changes require explicit Windows UAC approval. Disk cleanup is limited to classified targets selected by the user, requires confirmation, and revalidates targets before deletion.
