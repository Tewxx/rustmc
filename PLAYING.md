# Playing the current local build

Double-click `Play-RustMC.cmd` (or run `play.sh`) to open the title screen, then use Multiplayer.
Singleplayer is out of scope: the integrated server, worldgen and local commands are behind the off-by-default
`singleplayer` cargo feature (`cargo build --release -p mc-client --features singleplayer`, then `--play` / `--world`).
In the default build the title screen shows the Singleplayer button disabled.

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

Multiplayer is the current focus. Every play-state packet is handled: particles (`S2A`)
simulate the vanilla `EntityFX` types, and see `HANDOFF.md` for the remaining renderers
(signs, skulls, chests, maps, world border, lightning, minecarts/boats) and the
movement/anti-cheat audit. Server resource packs
work: the first offer shows the vanilla Yes/No prompt, accepted packs download with SHA-1
verification, apply immediately (models, atlases, font, GUI and a full world remesh), and are
cached and re-applied on later joins. `--pack-mode enabled|disabled|prompt` overrides the
remembered per-server choice. Pack sound overrides are not loaded yet (sounds come from
`.minecraft/assets`).

## Joining real servers

The title screen's **Multiplayer** button opens the direct-connect screen: type `host[:port]`,
edit the username (prefilled from `--name`), then **Join Server** for offline-mode servers or
**Join (Microsoft)** for online-mode servers. The client relaunches itself with the matching
`--connect`/`--online` flags; the saved-server list and in-process server switching are not
ported yet.

Offline-mode (cracked) servers accept any name:

```
rustmc --connect play.example.com --name ASUISSU
rustmc --connect 203.0.113.5:25566 --name ASUISSU
```

`--connect` takes `host[:port]` (default 25565) and resolves the host normally. Servers that
publish a `_minecraft._tcp` SRV record must be reached through their SRV target/port, which the
client cannot query yet; use `nslookup -type=SRV _minecraft._tcp.<host>` and connect to the
target directly.

Online-mode servers (Hypixel, most public servers) reject offline names: log in with

```
rustmc --connect mc.hypixel.net --online
```

`--online` opens the Microsoft device-code flow once and caches the refresh token in
`rustmc_account.json`; the in-game name is the account's profile name, not `--name`.
If the server asks for encryption without a Microsoft session, the client now reports
"This server is online-mode…" instead of a bare auth error. `rustmc --help` lists every flag.

This is not full 1.8.9 parity. World edits and inventory are session-only in singleplayer.
The integrated server has scheduled/random block ticks and fluids but no mobs or full
survival. Special block renderers, tile entities and several placement rules remain
incomplete. Visual parity is incomplete: demo generation, mipmaps, texture animation and complete
player/item lighting are still missing. Water and lava now render through the fluid path. Item model transforms and
overworld sky values have Java golden checks; rendered scenes have not passed a Java pixel
comparison.

Visual checks: menu screens need no world (`rustmc --screenshot shots/title.png --screen title`); in-game screens
(`hud`, `debug`, `chat`, `pause`, `body`, ...) need a server: start the mock server and add `--connect 127.0.0.1:25599`.
Screens that run local commands (`death`, `breaking`, `drops`, `glass`) only work in a `--features singleplayer` build.
Network checks: start the mock server (see `HANDOFF.md`), then
`rustmc --connect 127.0.0.1:25599 --screenshot shots/net.png --screen net`.
