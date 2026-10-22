# Playing the current local build

Double-click `Play-RustMC.cmd`, or run `cargo run --release --offline --bin rustmc -- --play`.
Without `--play`, choose Singleplayer on the title screen.

Multiplayer: `rustmc --connect <host>[:port] --name <name>` joins an offline-mode 1.8.9 server
(protocol 47). Add `--online` to sign in with a Microsoft account for online-mode servers
(device-code flow; the refresh token is cached in the git-ignored `rustmc_account.json`).
Online auth has not yet been tried with a real account.

- WASD: walk; Space: jump; Ctrl: sprint; Shift: sneak at ledges.
- Double-tap Space: toggle flight where the server allows it (creative/spectator). The toggle
  is sent as `C13 PacketPlayerAbilities`, so servers accept the movement state change.
- E: creative inventory. Click an item, then a hotbar slot. Shift-click gives a full stack;
  hover an item and press 1–9 to assign it directly. Tabs, search and scrolling work.
- 1–9 or wheel: select a hotbar slot. Left click: break/swing; right click: place a block;
  middle click: pick the targeted block. Placement checks the player's collision box.
- F5: rear third person, front third person, first person. F1: hide HUD/hand; F3: debug.
- T: chat; `/`: commands; Tab: command completion (the server's list when connected).
  Escape: pause.

Commands execute locally in singleplayer: `/help`, `/gamemode creative`, `/gamemode survival`,
`/give Player stone 64`, `/give Player planks 64 2`, `/tp ~ ~3 ~`,
`/setblock ~ ~-1 ~ stone`, `/fill x1 y1 z1 x2 y2 z2 stone`, `/clear`, `/kill`.
Only loaded chunks and height 0–255 can be edited. `/kill` opens the death/respawn screen.

Multiplayer is the current focus. The client handles every play-state packet except particles
(`S2A`) and server resource packs (`S48`); see `HANDOFF.md` for the remaining renderers
(signs, skulls, chests, maps, world border, lightning, fireballs) and the movement/anti-cheat
audit. Server resource packs are not applied yet, so Hypixel-style custom textures will show
as vanilla textures until that lands.

This is not full 1.8.9 parity. World edits and inventory are session-only in singleplayer.
The integrated server has scheduled/random block ticks and fluids but no mobs or full
survival. Special block renderers, tile entities and several placement rules remain
incomplete. Visual parity is incomplete: demo generation, fluid rendering, mipmaps, texture
animation and complete player/item lighting are still missing. Item model transforms and
overworld sky values have Java golden checks; rendered scenes have not passed a Java pixel
comparison.

Visual checks: `rustmc --radius 2 --screenshot shots/hand.png --screen hud`.
Other screens: `body`, `body-back`, `creative`, `held`, `commands`.
Network checks: start the mock server (see `HANDOFF.md`), then
`rustmc --connect 127.0.0.1:25599 --screenshot shots/net.png --screen net`.
