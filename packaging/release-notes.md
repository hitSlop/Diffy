Diffy 0.2.3 brings Git worktrees into one project and adds a little something while you wait for changes.

- Switch between the main checkout and linked worktrees using tabs across the top or ⌘← / ⌘→. Projects with one checkout keep the same header layout, with Staged/Changes in the row below.
- Keep each checkout's selected file, staged/unstaged view, diff mode, and reading position. Project navigation remembers the last checkout you reviewed.
- Discover new and removed worktrees automatically. Show detached checkouts explicitly, keep long tab lists accessible through the overflow menu, and fall back to an available checkout when one disappears.
- Group linked checkouts into one project icon with a combined change count. Project appearance is shared, while staging and external-app actions target the selected checkout.
- Drag the window from the project header and double-click its empty area for the macOS title-bar action.
- Play Snake in an empty diff pane: Enter starts or pauses, and arrow keys or WASD steer.
- Update the website with worktrees, project customization, review controls, and the waiting game.

Existing saved projects migrate automatically. Git remains the default status client and handles previews and staging; worktree discovery uses gix with a Git fallback. SSH review and full gix replacement remain development studies.

Install by dragging Diffy to Applications. Requires Git, Apple Silicon, and macOS 13 or later. Windows and Intel Mac builds are deferred.
