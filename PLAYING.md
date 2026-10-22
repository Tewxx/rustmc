# Playing the current local build

Double-click `Play-RustMC.cmd`, or run `cargo run --release --offline --bin rustmc -- --play`.
Without `--play`, choose Singleplayer on the title screen.

- WASD: walk; Space: jump; Ctrl: sprint; Shift: sneak at ledges.
- Double-tap Space: toggle creative flight. Space/Shift: fly up/down.
- E: creative inventory. Click an item, then a hotbar slot. Shift-click gives a full stack;
  hover an item and press 1–9 to assign it directly. Tabs, search and scrolling work.
- 1–9 or wheel: select a hotbar slot. Left click: break/swing; right click: place a block;
  middle click: pick the targeted block. Placement checks the player's collision box.
- F5: rear third person, front third person, first person. F1: hide HUD/hand; F3: debug.
- T: chat; `/`: commands; Escape: pause.

Commands execute locally: `/help`, `/gamemode creative`, `/gamemode survival`,
`/give Player stone 64`, `/give Player planks 64 2`, `/tp ~ ~3 ~`,
`/setblock ~ ~-1 ~ stone`, `/fill x1 y1 z1 x2 y2 z2 stone`, `/clear`, `/kill`.
Only loaded chunks and height 0–255 can be edited. `/kill` opens the death/respawn screen.

This is a playable local milestone, not full 1.8.9 parity. World edits and inventory are
session-only. There is no server connection or integrated server yet; item use, crafting,
containers, drops, mobs, fluids, redstone simulation and complete survival rules remain to port.
Special block renderers, NBT-based item variants and some placement rules remain incomplete.
Visual parity is incomplete: demo generation, fluid rendering, mipmaps/texture animation and
complete player/item lighting are still missing. Item model transforms and overworld sky values
have Java golden checks; rendered scenes have not passed a Java pixel comparison.

Visual checks: `rustmc --radius 2 --screenshot shots/hand.png --screen hud`.
Other screens: `body`, `body-back`, `creative`, `held`, `commands`.
