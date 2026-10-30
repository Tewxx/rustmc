# LunarForge and AccountManager native ports: coverage ledger

These are partial ports. They are not replacements for either source mod yet. The
`.rmod` built by `lunarforge/build.sh` implements only the FPS HUD slice; selecting
that separate package enables it, while the original FPS module defaults off.
The current draw layout is provisional: it needs the host font-width API and Lunar
HUD positioning/settings before claiming visual parity. CPS and coordinate helpers
are tested native Rust logic but have no input/clock/config/draw bindings yet.
AccountManager is credential-free interaction logic, not a runnable component:
its screen, secure account service, add/login flows and host integration are missing.

## Artifact identity and method

- LunarForge 0.1.0, requested Codeberg release `lunarforge.jar`, SHA-256
  `ca840fda47675047d7861ecb933c46b2cb1aad270473b7f3a0f84250ca37e94c`.
- AccountManager 1.8, local `AccountManager-1.8.jar`, SHA-256
  `a4a5a662d00df612221655cacdb5f7331f2b600605eb14bba850a9bc794153af`.
- Both artifacts were statically reconstructed with Vineflower 1.11.0 and CFR
  0.152, separately, without running either mod. Own-class extracts and outputs
  remain under ignored `.mc-tools/mod-port/`; jars and decompiled sources are not
  redistributed. Neither jar contains a license file; reuse/redistribution terms
  are unknown. Code here is independently implemented from observed behavior.

## Current verified behavior

- FPS: English `N FPS` / `FPS: N`; static default 56×18, translucent background
  and shadow. Reverse setting and exact centered positioning are not yet wired.
- CPS: left/right mouse-down in a rolling 1000 ms window, ignored while a screen
  is open; separate left count while the player is not using an item. Label,
  reverse and right-click text logic are tested. Host mouse and millisecond clock
  inputs, config, and rendering remain to be connected.
- Coordinates: eight-direction yaw conversion is tested; coordinates display,
  biome, culling counters, child layout and copy key remain unported.
- Account Manager: selection, Shift+arrow reorder requests, activate/delete,
  launch-session restore eligibility, pending operations, ban status parsing and
  labels are represented without storing credentials. These are guest-independent
  state transitions; no account, file, token, cookie or session API is connected.

## Remaining LunarForge modules

The following own `Module*.class` names were inventoried from the target jar.
Names group work; they are not evidence that each class has been analyzed or
ported. Child/helper classes, GUI, cosmetics, network clients, transformers and
assets outside this inventory remain part of the requested scope.

### hud

`ModuleActionBar`, `ModuleArmorStatus`, `ModuleBossbar`, `ModuleClock`, `ModuleCombo`, `ModuleCoordinates`, `ModuleCoordinatesChild`, `ModuleCps`, `ModuleDayCounter`, `ModuleDirectionHud`, `ModuleFps`, `ModuleKeystrokes`, `ModuleMemory`, `ModulePackDisplay`, `ModulePing`, `ModulePingHud`, `ModulePingNametag`, `ModulePotionCounter`, `ModulePotionEffects`, `ModulePvpInfo`, `ModulePvpInfoHealth`, `ModulePvpInfoMelee`, `ModulePvpInfoProjectile`, `ModuleReachDisplay`, `ModuleScoreboard`, `ModuleServerAddress`, `ModuleShaderPackDisplay`, `ModuleStopwatch`, `ModuleTitles`

### mechanic

`ModuleAutoTextActions`, `ModuleAutoTextHotkey`, `ModuleChat`, `ModuleChatCommandAliases`, `ModuleCrosshair`, `ModuleDamageTint`, `ModuleFog`, `ModuleFov`, `ModuleFreelook`, `ModuleHurtCam`, `ModuleLighting`, `ModuleNickHider`, `ModuleSnaplook`, `ModuleTab`, `ModuleTimeChanger`, `ModuleTntCountdown`, `ModuleToggleSneak`, `ModuleToggleSneakHud`, `ModuleWeatherChanger`, `ModuleZoom`

### server

`ModuleHypixelBedwars`, `ModuleHypixelMods`, `ModuleQuickplay`

### visual

`Module3dSkins`, `ModuleBlockOutline`, `ModuleColorSaturation`, `ModuleGlintColorizer`, `ModuleHeightLimit`, `ModuleHitColor`, `ModuleHitbox`, `ModuleItemPhysics`, `ModuleItems2d`, `ModuleLightOverlay`, `ModuleMenuBlur`, `ModuleMobSize`, `ModuleMotionBlur`, `ModuleOneSevenVisuals`, `ModuleParticleChanger`, `ModuleSaturation`, `ModuleScrollableTooltips`, `ModuleShinyPots`

## Remaining AccountManager behavior

The source exposes offline, Microsoft OAuth, cookie import, refresh-token and
access-token add/login modes; saved account ordering, removal, launch-session
restore, account-switch buttons, clipboard, ban tracking and notifications. It
writes `accounts.json` with tokens. The port must keep secrets in the host, use
opaque handles across WASM, preserve import/refresh behavior with explicit user
action, and avoid transmitting the launcher session to any mod. The currently
reserved account WIT does not yet implement those host services or screens.

## Other required work

LunarForge features include rendering/HUD editing and settings, camera/movement
hooks, chat/packets, world overlays, animations, cosmetics, Lunar network services
and GUI. Each needs the corresponding WIT and host behavior before it can run.
Security-sensitive network/auth features need explicit user-controlled host APIs;
no credentials or third-party network requests are made by the current port.
