Diffy 0.2.6 — GitHub pull requests and simpler project imports

- Switch between worktrees and read-only GitHub pull request diffs. Browse open and draft PRs, search by title or number, choose a remote, and load more results as needed. Uses your existing GitHub CLI login for public and private repositories.
- See loading feedback while fetching PR lists, refreshing, loading a diff, or loading another page. Returning to Worktrees preserves your local selection and reading position.
- Fix the crash when refreshing project imports. Herdr, Codex, and Claude Code projects now appear in one deduplicated list, ordered by recent activity. Repositories in system temporary directories are hidden; you can still open them manually.
- Update the landing-page features to describe PR review and the improved import flow.

Windows and Linux are early alphas with limited hardware testing. Windows installers have no publisher signature. Linux initially targets Ubuntu 24.04+ with Wayland or X11 and a working Vulkan driver. Git is required on every platform.
