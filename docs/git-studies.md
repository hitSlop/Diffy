# Git client and remote review studies — October 14, 2026

The production change is repository grouping and a fixed project/worktree header. gix 0.89.0 performs read-only worktree enumeration, with Git porcelain fallback. Status still defaults to Git, and previews and index mutations still use Git. The studies below are isolated from the application.

## Full gix replacement: retain Git for now

The current adapter cannot replace every operation without losing compatibility. This is a decision about the implemented backend, not a claim that gix's plumbing could never support the missing behaviors.

| Operation | Current implementation | Replacement evidence / remaining work |
|---|---|---|
| Discovery and worktree enumeration | Git discovery; gix topology with Git fallback | Main/linked, bare owners, locked/missing checkouts and literal newline/Unicode paths tested against Git. Environment overrides still delegate. |
| Status and cache revisions | Git default; opt-in gix adapter | Existing 20 comparison tests pass, including complete entries and cache identities. Several cases pass through Git fallback, so these are not proof of a Git-free backend. |
| Branch labels | Git in both status adapters | gix reads HEAD, but exact ambiguous-ref shortening and detached abbreviations still use Git in production. |
| Selected-file previews | Git diff, plus local untracked/conflicted reads | Native blob diff is available, but an equivalent adapter into the existing DiffFile model has not been implemented and verified. |
| Watcher ignore decisions | Incremental matcher plus Git confirmation | Native replacement must preserve force-added ignored files, conditional config, and stale-rule handling. |
| Whole-file/bulk stage and unstage | Git add/restore/rm | A narrow Git-free gix index-write probe matches plain-text staging and preserves working-tree bytes and unrelated index entries. It fails object parity for CRLF conversion. Additions/deletions, rename pairs, unborn unstage, conflict/sparse entries, filters, and concurrent index changes still need a complete implementation. |

The mutation probe deliberately writes raw file bytes. Git stages normalized LF bytes for a `text eol=crlf` file; the probe stages CRLF bytes and produces a different blob id. gix provides filter plumbing, so this identifies missing integration rather than proving conversion is impossible. The child runs with no programs on PATH; Git is used only by the parent fixture/oracle.

Index mutation also requires careful cache and transaction handling. The probe clears the tree-cache before writing; leaving it valid after changing entries can make later Git tree construction use stale contents. Its read-then-write approach is insufficient for concurrent production writes. See the [gix index write API](https://docs.rs/gix-index/latest/gix_index/struct.File.html#method.write).

Native status coverage still conservatively delegates conflicts, submodules, sparse indexes, intent-to-add, inherited repository/config overrides, embedded repositories, uncertain similarity renames, and content conversion. Existing fixtures demonstrate correct fallback, not native coverage. Worktree enumeration is independently useful and does not require removing these gates.

### Measurements

One optimized run, 30 iterations after warm-up, using the same 50,000-line/1,000-file fixtures as the existing performance report:

| Workload/backend | Median | p95 |
|---|---:|---:|
| Untracked / Git | 14.77 ms | 15.75 ms |
| Untracked / gix | 11.05 ms | 12.53 ms |
| Mixed / Git | 39.73 ms | 56.50 ms |
| Mixed / gix | 33.48 ms | 47.14 ms |

Separate process measurements with `/usr/bin/time -l` gave peak RSS of 15.4/18.0 MiB for Git and 17.6/20.6 MiB for gix across untracked/mixed fixtures. These include fixture setup, exclude the app/window/GPU, and are separate runs from the timing table. Results vary with system load and filesystem caching. Faster metadata does not satisfy the full-replacement compatibility gate.

Reproduce:

```sh
cargo test --locked --test gix_comparison
cargo run --release --locked --example gix_replacement
/usr/bin/time -l target/release/examples/gix_replacement --benchmark git untracked
/usr/bin/time -l target/release/examples/gix_replacement --benchmark gix untracked
/usr/bin/time -l target/release/examples/gix_replacement --benchmark git mixed
/usr/bin/time -l target/release/examples/gix_replacement --benchmark gix mixed
```

The study writes only disposable fixtures. No production index-write implementation or default-backend change is introduced.

## SSH working-tree review: feasible; keep it a study

`scripts/spikes/ssh_review.py` uses system OpenSSH and remote Git, without uploading a helper or installing remote software. It accepts a host alias and absolute repository path, returns staged/unstaged/untracked/conflicted entries and worktrees as JSON, and retrieves one selected preview. OpenSSH configuration, agent/key authentication, and known_hosts remain authoritative. A private per-run control socket reuses the connection and is closed on exit.

All arguments are POSIX-shell quoted because OpenSSH sends a command string to the remote shell. Git paths use literal pathspecs and NUL-delimited records. Untracked/conflicted reads are capped at 4 MiB and symlink targets are read without changing their newline bytes. Tracked diffs use the app's existing full-context Git options. Network work stays outside the production viewer.

The disposable localhost SSH fixture verified:

- Staged, unstaged, untracked, partially staged, and conflicted previews; two registered worktrees.
- Repository/file names containing spaces, quotes, Unicode, newlines, and shell metacharacters; symlink targets ending in newlines.
- Unchanged index bytes and modification time after reads.
- Recoverable missing-file/repository/Git errors, timeout cancellation, disconnects, rejected authentication, and rejected host-key mismatches.

The latest 30-iteration localhost run measured a 1.89 s cold refresh, warm refresh median/p95 of 0.84/2.01 s, and selected-preview median/p95 of 0.84/1.96 s. These include two remote command invocations per operation, remote shell startup, and concurrent development load. They are not WAN measurements or a polling target. Request batching or a persistent request process should be evaluated before production if this latency remains representative.

Reproduce with only generated fixture keys and repositories:

```sh
python3 scripts/spikes/ssh_review.py --self-test --report target/ssh-study.json
```

To inspect an existing trusted POSIX host using its installed Git:

```sh
python3 scripts/spikes/ssh_review.py my-host /absolute/repository
python3 scripts/spikes/ssh_review.py my-host /absolute/repository --preview src/main.rs
python3 scripts/spikes/ssh_review.py my-host /absolute/repository --preview src/main.rs --staged
```

This spike requires trusted known_hosts and non-interactive key/agent authentication. Connection UI, password prompts, WAN behavior, polling, and remote mutation remain deferred. Git transport SSH used for clone/fetch does not expose a remote index or uncommitted working-tree files; the [upstream built-in transport proposal](https://github.com/GitoxideLabs/gitoxide/discussions/2767) is a separate concern.
