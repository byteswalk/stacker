# Phase 1 Task 3 Report

## Summary

- Centralized known developer-cache, JetBrains history, and temporary-directory rules in `space_analysis::known`.
- Added stable `KnownCandidate`, `SafetyClass`, and `CleanupKind` contracts and converted measured candidates into `KnownSpaceItem` values.
- Reused Task 2 `measure_path`, cancellation, progress, link refusal, hard-link accounting, and error aggregation.
- Preserved the legacy `CacheItem` shape and cleanup command signatures through a compatibility adapter.
- Revalidated every delete request against a freshly measured, visible candidate set and canonical path before deleting.
- Applied one legacy visibility predicate to scan, delete, safe-delete, aged-stat, and aged-delete eligibility.
- Preserved the 1 GiB temporary-directory visibility threshold and moved the JetBrains parsing tests with the rules.

## Files

- `src-tauri/src/space_analysis/known.rs`
- `src-tauri/src/space_analysis/mod.rs`
- `src-tauri/src/cleanup.rs`
- `.superpowers/sdd/phase-1-task-3-report.md`

## TDD Evidence

- Initial `cargo test --manifest-path src-tauri/Cargo.toml space_analysis::known::tests` failed because `known_candidates`, `SafetyClass`, `version_key`, and `split_versioned_dir` did not yet exist in `known.rs`.
- After implementation, the same filter passed all original 4 known-rule tests.
- The existing JetBrains product/version parsing tests were removed from `cleanup.rs` only after they were running from `known.rs`.
- Review regression: `legacy_visibility_excludes_small_temp_directories` initially failed because the shared pure visibility predicate did not exist, then passed after both scanner output and cleanup whitelisting used that predicate.
- The regression test uses synthetic candidates and byte counts; it does not inspect or modify real user temporary directories.

## Validation

- `cargo test --manifest-path src-tauri/Cargo.toml cleanup`: passed (build and command filter; no cleanup-named unit tests remain after migrating the JetBrains tests).
- `cargo test --manifest-path src-tauri/Cargo.toml space_analysis::known`: 5 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 35 passed.
- `rustfmt --edition 2021 --check src-tauri/src/cleanup.rs src-tauri/src/space_analysis/known.rs src-tauri/src/space_analysis/mod.rs`: passed.
- `git diff --check`: passed.

## Notes

- The brief's combined `cargo test ... cleanup space_analysis::known` command is not accepted by Cargo because it allows one test filter. The two filters were run as separate commands.
- Full tests retain expected dead-code warnings for Phase 1 contracts used by later tasks, including `Rebuildable`, `ViewOnly`, task model types, and `CancellationToken::cancel`.
