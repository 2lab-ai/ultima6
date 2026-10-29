# Live checks

Run from any checkout with `tools/web` dependencies installed. Use your own game archive
via `U6_ARCHIVE` or the shared `U6_DATA_ROOT` layout described in the repository README.
The URL is explicit; a script does not deploy or change the hosted site. It interacts with
a fresh browser context and imports the archive locally, as a player would.

```sh
node tools/web/live/menu.mjs https://your-site.example /private/out/menu
```

`menu.mjs` replaces the old local `live30-menu.mjs` (page menu) and `live37-menu.mjs`.
It follows the launcher, requires the hosted Korean and HD packs, checks the engine menu's
`korean === true`, uses Go here once, checks paced travel, then opens another menu and closes
it with Escape. Missing assets or a failed check fail the run. It prints codes, timing and
counts only. The earlier local files are retained as historical evidence; do not run the
retired page-menu check against the engine menu.

The output prefix is required. Screenshots contain the owner's game content and must remain
private, outside **every** Git worktree. The script refuses an output directory inside Git
and uses private file permissions. Do not attach screenshots or a save to a public issue.

For a 1× held-key performance comparison, record machine load before and after the run and
run without other game/browser/build jobs. A loaded run may diagnose behavior, but cannot be
reported as an idle baseline.
