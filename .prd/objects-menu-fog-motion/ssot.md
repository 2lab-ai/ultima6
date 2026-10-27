---
title: ultima6 objects-menu-fog-motion — SSOT
type: ssot
tags: [ultima6]
---

# SSOT — objects-menu-fog-motion

User command (2026-10-27, /goal, verbatim):

> ultima6 개선
>
> 1. object중에 포팅안된것 마저 모두 포팅. (예 그전에 도개교 사용이나 드래곤이 제대로 작동안하고 있던것 처럼 니가 빼먹은 번역있으면 모두 확인하고 추가
> 2. object에 우클릭시에 문맥 메뉴가 나오게 해줘.
> ㄴ 예: 문에 우크릭시에 Use를 선택할수 있고 해당 object에서 할수 있는 행동은 모두 보여주고 클릭할 수 있도록 개선
> 3. 보이다가 시야에서 안보이면 지금 안보이는 부분이 그냥 사라지는데, 회색 표시로 전환해줘. (움직이는건 안보이고 안보이게 됐을때의 모습으로 b/w 어둡게 표시하도록 개선. (화면 밖으로 나가면 당연히 렌더링에서 제거.
> 4. 턴에 대해서 리서치하고 /using-dotprd로 자세히 기술해줘. 이거 지금 되게 옛날 게임 같은 턴 처리가 되고 있음.
> 5. 유저가 이동하거나 턴을 넘길때 아바타와 동료 그리고 npc들의 움직임이 점프를 하는데, 울티마7처럼 보통 턴제 게임 처럼 아바타나 npc가 위치와 위치를 자연스럽게 걷거나 뛰는것 처럼 여러 프레임에 걸쳐서 이동하도록 개선. 이건 리서치를 충분히해주고 너무 느리거나 너무 빠르지 않게 적용해줘.

Specs: item 1 = `16-object-completion.md`, item 2 = `17-context-menu.md`, item 3 = `18-remembered-sight.md`,
item 4 = `19-turn-model.md`, item 5 = `20-smooth-motion.md`.

Decisions taken here (reversible; the owner vetoes on review):

- **Item 1 covers three kinds of gap.** "포팅안된것" and "빼먹은 번역" are read as code not yet carried over and lines
  not yet translated, both:
  - (a) original routines that game code still reaches through a `todo_stub!`. This includes a stub whose callee
    *is* ported elsewhere but whose caller was never wired (a stale seam: `port-map.md` hides these, because a
    ported address outranks its stub, `tools/rea/port_map.py:40,112`);
  - (b) routines that are ported but act unlike the original, the drawbridge and dragon class;
  - (c) lines that object code prints and the Korean pack does not cover.

  Debug-only paths (the debug teleport, 0x104140) are out of scope and named as such. The ending (0x133d1e /
  0x13513a) is in scope: it is the last object handler, the vortex cube's.
- **Item 1c's upload authority** is the owner's own words above, "빼먹은 번역있으면 모두 확인하고 추가": the lines
  found missing are sent to the model for translation, as the item 3 run of `hd-layout-font-register/ssot.md` was.
  Nothing it makes enters the repository.
- **"배포" is production** (`https://ultima6.2lab.ai`), once per item as soon as its receipt is green, as in
  `hd-layout-font-register/ssot.md`. A pack build goes on top of the live pack (ko-1026a at the time of writing),
  after a message to the session that holds `hd-layout-font-register` row 25.
- **Item 2: a right press on the map opens the menu.** In the original the right button repeats the underlined
  command on the cell (0xa9), else walks like the left (`crates/u6-core/src/ui/click.rs:116-148`). The menu takes
  the right press on the map and on the panels' objects. The underlined command, when there is one, is the menu's
  first entry, so the original's repeat stays one click away. Left-button walking is unchanged.
- **Item 3: memory is render state only.** It is keyed by world cell and level, kept in the port, outside the
  original's savegame format. A cell that was seen and is no longer seen, for any reason (sight line or darkness),
  shows its last seen ground and still objects in dark greyscale. Actors are not shown there. (Amended
  2026-10-27 by the owner's follow-up below: a cell is no longer dropped when it leaves the view.)

Owner's follow-up on item 3 (2026-10-27, verbatim):

> 화면에 안보이면 바로 없는게 아니고 한번이라도 본 지형은 계속 기억하도록해줘. 그리고 게임내 턴수에 따라서 천천히 없어지는데 1에서 0이 되는게 아니라 1에서 하루가 지날때마다 50%씩 까여 그럼 해당 타일의 50%만 그 디더링하듯 격자로 출력. 이틀 지나면 25% 남고 이렇게 3일 4일 6.25% 5일째되면 검게 표시하도록해줘 이렇게 구현

Decisions on it (reversible):

- **Kept off screen and on other levels.** A remembered cell stays remembered after it leaves the window and while
  the party is on another level; it shows again in grey when it is back in the window and out of sight. The window
  reset (death, arrest, the healer) no longer forgets it.
- **Fades by game days, not turns.** The age is the game time since the cell's last sight, on the original's
  calendar (60 minutes, 24 hours, 28 days, 12 months). After d whole days (1440 minutes each) the cell shows
  2^-d of its pixels: 100, 50, 25, 12.5, 6.25 % for d = 0..4. From day 5 it is black and forgotten.
- **The dither is a fixed 4 × 4 ordered pattern** on the original's pixel grid, anchored to the world, each
  level's pixels a subset of the one before (50 % is a checkerboard). A pixel outside the pattern is black. HD
  draws the same pattern at the original's pixel grid.
- **Saves.** The memory is written beside the savegame as a port-only file and read back with it (unit C3, after
  C2); until C3 ships, a restore forgets it.
- **Item 4's deliverable is the spec.** "리서치하고 … 자세히 기술해줘" asks for research and a written
  description, so `19-turn-model.md` ends with options and a recommendation. The owner's words read as a complaint, so the
  cheapest step of that recommendation is built in round 2 beside item 5, and the deploy report names it.
- **Item 5 is presentation only.** The engine stays tile-based and on the original's timing. A slide between two
  cells lasts a share of the step's own interval, scales with the speed setting and collapses at Max.

Owner's follow-ups (2026-10-28, verbatim, in order):

> 문맥 메뉴 인게임 ui로 해줘 시발 왜 웹페이지 ui로 만듬?
> 그리고 인게임에서 마우스 표시되니까 게임 화면에서는 마우스 숨겨줘

> 그리고 우클릭시 여기로 이동도 하나 추가해줘 해당 위치까지 a*로 검색해서 이동하는 명령

> 그리고 대화시 나오는 사람패널 밑에 job 대화랑 name 대화 버튼 만들고 (인게임 ui로) 클릭시 해당 대사 입력 그리고 대화창에서 빨간색 키워드 클릭시 해당 대사 하도록해줘. 그리고 메세지창에서 스크롤로 지난 대화 볼수 있게 해줘 오른족에 스크롤바도 추가해줘

- **Item 2 is in-game UI.** The deploy-30 menu (a page element over the canvas) was the wrong reading. A game's
  right-click menu is the game's own UI: the engine draws it in the game's font and window style, it is
  operated by the game's pointer and keys, and no page element is used (unit M3, spec 17 reworked).
- **Port UI is in-game UI from here on.** Every new control in this workstream (the menu, the talk buttons,
  the scrollbar) is drawn by the engine in the game's own style. Page elements stay for page concerns
  only (the chooser, packs, settings).
- **The OS cursor is hidden over the game screen**, where the game draws its own pointer (unit K3).
- **"여기로 이동" walks, it never teleports.** A* over the leader's real step rule. Each step is a real
  move entered as a direction key would be, so every rule and turn applies. It stops on arrival, a block,
  combat or a hostile in view, or any input (unit M4, spec 21).
- **Talk by click** (unit D1, spec 22): Job and Name buttons under the conversation portrait, and a press on a
  coloured keyword. Each types the word as conversation input would.
- **Message scrollback** (unit D2, spec 23): the wheel scrolls through past lines, with a scrollbar on the
  right. With the view at the live bottom the frame equals the original's.

