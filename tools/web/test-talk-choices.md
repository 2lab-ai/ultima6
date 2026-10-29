# Session choices and contextual answers

`test-talk-choices.mjs` is a separate regression runner for spec 22's session choices
and contextual y/n answers. It does not change `test-web.mjs` or rebuild wasm.
The default invocation and `--help` print usage without starting a server or browser.

## Inputs and commands

Use the existing web-test dependencies and built `site/pkg` and `site/pkg-wide`.
The runner imports the existing harness, whose local disk paths come from
`tools/data-paths.mjs`: `U6_DATA_ROOT` or `U6_ORIGINAL_DIR` selects owner-supplied
disk images. No game assets, original conversation text, save fixture, personal
path or downloaded game data is included in this test.

```sh
node --check tools/web/test-talk-choices.mjs
node tools/web/test-talk-choices.mjs --help
U6_REQUIRE_ARCHIVE=1 node tools/web/test-talk-choices.mjs --engine
U6_REQUIRE_ARCHIVE=1 node tools/web/test-talk-choices.mjs --browser
```

`--site PATH` selects another existing built site directory. It is useful for a
worktree that should test a fixed integration build without copying or rebuilding
it. Both modes hash the glue/wasm before and after, assert they did not change, and
print their source-build hashes with the pass count. Missing disks fail with
`U6_REQUIRE_ARCHIVE=1`; without it they explicitly skip.

The two modes are separate evidence:

- `--engine` instantiates the real wasm engine in Node, boots from local disks,
  substitutes only the synthetic conversation in memory and drives the engine's
  actual input methods. No server, browser, OS window or screenshot is involved.
- `--browser` uses `h.setup`, the existing page and `h.screenPoint`. Mouse events
  pass through the actual HD/Wide compositor and page event handlers. It checks
  the canvas's header pixels as well as the engine's glyphs and input state.

Optional `U6_FRAMES_DIR` saves browser screenshots and a content-free receipt.
The directory must be outside the current checkout, the main checkout and every
other git worktree, including paths reached through symlinks. Screenshots may show
owner-supplied game art and must stay private. Engine mode never writes frames.

## Scenarios

Both modes run the same synthetic English and Korean conversations with Original
and Wide wasm. Browser mode uses the HD and Wide layouts with the HD font enabled.
The Korean pack is generated from the synthetic script's own offsets, hashes and
aliases; this is not a test of a separately supplied translation pack.

1. A new conversation has End, Name and Job in three columns without arrows.
2. Clicking Name prints eight marked keywords; discovery produces a full grid
   and overflow arrows. End stays in the first slot on the next page.
3. `alphabet` and `alphanumeric` have the same shortened label, distinct full
   hover previews, and distinct matching responses when selected. Leaving hover
   restores the portrait and name pixels (engine frame or browser canvas).
4. Pressing one shortened label and releasing on the other cancels without an
   answer. After a selection changes the sort order, the old slot loses that ID
   and the same stable ID remains selectable on its new page.
5. Used text has a different, neutral-grey ink; the coarse Amiga palette permits
   two channel steps of tint. Used choices can be selected again.
6. Job enters an actual f8 y/n reader. The question remains visible; the Yes and
   No targets each execute their own response and return to the choice grid.
7. End exits; talking again resets the grid, removes overflow and restores unused
   ink. Output comparisons exclude cursor symbols that blink between frames.

Waits follow reader, queue, frame and prompt state. The runner does not use fixed
wall-clock sleeps. Console output contains scenario labels, pass/fail, source
build hashes and error locations; it never prints original game content.

## Validation status

The initial portable runner passed syntax/help checks and all four scenarios in
`--engine` mode against the existing integration wasm. That verifies the actual
engine and the synthetic fixture's control flow, aliases, sorting and input.

The expanded **browser mode has not completed a run**. The available environment
failed Chromium's macOS Mach-service registration before page setup, including
one diagnosed retry. No browser launch was attempted while preparing this
portable runner. Run `--browser` after the execution environment is available;
do not treat the Node result as compositor, DOM, font-loading or screenshot
evidence. The existing `Conversation by click` browser suite is separate.
