# Performance pass — October 14, 2026

File listings now use Git metadata; source parsing and highlighting happen only for the selected preview. Inactive scopes/projects release source documents and retain reading anchors. Parsed previews and prepared presentations use shared, bounded caches with a combined 64 MiB estimated budget; oversized documents bypass the caches.

The app reuses repository locations until invalidated and resolves Apple's Git tool dispatcher once. Preference saves run on one background writer, coalesce pending snapshots, and flush before closing, quitting, or updating.

## Initial performance-pass measurements

Thirty measured iterations per case after a warm-up. Both workloads contain 50,000 source lines and 1,000 small files. The mixed workload commits those files, modifies the large file, stages 100 small files, edits 50 of them again, and adds 100 untracked files. The eager comparison uses the retained full-load implementation; both paths benefit from the Git executable lookup improvement.

| Operation | Untracked median / p95 | Mixed median / p95 |
|---|---|---|
| Eager full load | 83.02 / 103.02 ms | 177.05 / 240.51 ms |
| Metadata refresh with cached repository location | 21.79 / 28.74 ms | 59.51 / 77.05 ms |
| Cold selected large-file preview | 19.55 / 22.88 ms | 93.45 / 122.16 ms |
| Cached small-file source lookup | 0.17 / 0.21 µs | 0.25 / 0.42 µs |
| Read-only gix status prototype | 3.28 / 4.50 ms | 40.87 / 66.04 ms |

Metadata refresh is approximately 74% faster for the untracked workload and 66% faster for the mixed workload than eager loading. First selection still incurs preview parsing; cached source lookup excludes layout, highlighting, and GPU rendering. These are local wall-clock measurements, not a frame-rate guarantee. Results vary with filesystem and system load.

Isolating each operation in a separate process with `/usr/bin/time -l`, peak RSS across both workloads was 68.3 MiB for eager loading, 19.0 MiB for metadata, 67.6 MiB for cold selected previews, and 23.5 MiB for gix status. These are benchmark-process peaks including fixture setup; they do not measure the full application's GPU/window memory.

Raw timing and memory output is available locally in `target/performance-results`. Reproduce with:

```sh
cargo bench --locked --bench performance
DIFFY_BENCH_REPORT=1 cargo bench --locked --bench performance
DIFFY_BENCH_REPORT=1 DIFFY_BENCH_ONLY=metadata cargo bench --locked --bench performance
DIFFY_TRACE_PATH="$PWD/target/diffy-trace.json" cargo run --locked -- --check .
```

## Complete adapter measurements

A second release-build run measured the production adapter (30 measured iterations after warm-up). Unlike the filename-only prototype, this includes complete entries, cache revisions, watch locations, configuration handling and Git's exact branch-label read.

| Metadata backend | Untracked median / p95 | Mixed median / p95 |
|---|---|---|
| Git metadata | 21.07 / 29.69 ms | 62.69 / 73.42 ms |
| Complete gix adapter | 23.70 / 28.47 ms | 32.28 / 50.09 ms |
| Earlier filename-only gix prototype | 3.87 / 4.60 ms | 15.91 / 20.87 ms |

The complete adapter is about 49% faster on the mixed workload, and about 12% slower on the unborn/untracked workload. In particular, the retained Git branch-label subprocess consumes much of the prototype's untracked advantage. These results support keeping the rollout opt-in while measuring real repositories. They do not establish smooth scrolling: the metadata benchmark excludes interactive rendering.

## gix rollout

The prototype compares scope membership, discovers the repository, reads its branch, and performs HEAD/tree/index/worktree status reads without persisting index changes. Compatibility fixtures cover unborn and mixed scopes, partially staged paths, literal Unicode/newline names, renames, detached HEAD, and linked worktrees. These comparisons check scope paths, not complete preview or mutation equivalence.

gix 0.89.0 is now a pinned runtime dependency behind `DIFFY_GIT_BACKEND=gix`. Git remains the default; `DIFFY_GIT_BACKEND=git` or removing the variable restores it. `--check` reports the backend actually used and any fallback category. Source previews, branch labels (including ambiguous refs and detached abbreviations), watcher ignore checks, and all staging/unstaging still use Git.

The production adapter returns complete `ChangedFile` entries and repository/watch context. Both backends hash the same semantic object ids, modes, status and original path, plus disk stamps for worktree entries. Switching backend or falling back therefore preserves preview-cache keys. The gix repository is opened on the refresh worker with strict configuration and ownership checks, and its suggested index updates are never persisted. Thread fan-out for tracked status is capped at two per refresh.

The 20-test comparison suite includes complete-field parity for untracked and mixed workloads, unborn staging, partial staging, cache revisions, literal Unicode/newline names, exact and empty-file renames, ignore files and conditional configuration includes, rename/copy configuration overrides, linked worktrees, nested discovery, detached/ambiguous branch labels, symlinks, executable/type changes, assume-unchanged/skip-worktree flags, missing indexes, and cross-backend preview/staging actions. Fallback tests cover conflicts, submodules and their ignore configuration, sparse indexes, intent-to-add, embedded repositories, uncertain/similarity renames, native-only configuration, external index/global-config environment overrides, and content conversion. Invalid UTF-8 paths produce the same display error. Reads preserve index bytes and mtime and create no lock files.

Compatibility work caught two concrete issues: `status.showUntrackedFiles=no` disables gix's directory walk before the platform override can act, so the adapter overrides it in memory. The iterator's FastEq checks raw disk size before applying content conversion, so converted worktree modifications conservatively fall back to Git. Harmless attributes (such as this project's whitespace rule) stay native. Git's diff auto-refresh can touch the index timestamp during normalized-content reads; read commands now disable it, and the Git metadata reader uses numstat alongside raw status to exclude stat-only changes without keeping patches.

Promote gix to the default only after opt-in use across real repositories retains complete-field parity and repeatable end-to-end refresh improvements. Expand native coverage for content conversion, submodules, conflicts and fuzzy renames with differential fixtures before removing their fallback gates. This switch is a metadata rollout, not a replacement for Git's patch or write operations.

The first uncached benchmark-profile build took 11m 05s on this machine, including the GPUI and other release dependencies. That time is not an isolated measurement of gix compilation cost. The packaged runtime inventory now includes gix and its MIT/Apache-2.0 license texts; Criterion remains excluded.

## Verification

Native checks cover lazy loading, cached navigation, stale-response rejection, unrelated edits, path-copy actions, physical zoom shortcuts, wrapping, preference persistence, and reading-anchor retention after unloading a scope. Existing rendering, staging, import, launch, selection/copying, scrollbar, and Git tests remain in place. Repository tests cover conflicts, index preservation, byte-budget eviction, and repository-location invalidation.

The signed installation exercise is explicitly ignored in routine checks and runs as a required release-script gate with the release's expected version. The existing signed 0.2.0 archive was also installed, signature-verified, and relaunched successfully in a disposable app copy using its explicit expected version.

Website builds pass with checkout absent and configured. The 480px screenshot variant is approximately 23 KiB versus the original 129 KiB; the original remains the full-resolution fallback. CI builds these configurations without deploying.
