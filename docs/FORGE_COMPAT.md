# Forge compatibility: API surface and implementation order

Static analysis only. No mod code was executed, loaded in a JVM, or contacted over the network. Inventory tool:
`tools/modscan/` (`modscan.py` per jar -> JSON, `report.py` merges JSON into the ranked tables below,
`scan_all.sh CORPUS OUT`). Re-run on any mod:

    tools/modscan/scan_all.sh .mc-tools/mod-corpus .mc-tools/scan
    python3 -I tools/modscan/report.py .mc-tools/scan/*.json --top 30

The scanner parses class files itself (constant pool, annotations) and shells out to `javap -c -p` (JDK 8, disassembler
only) for call-site counts. Raw JSON for the corpus lives in `.mc-tools/scan/` (gitignored).

See `docs/FORGE_STUBS.md` for the generated stub layer and how to implement and discover methods.

## Architecture recap

rustmc embeds HotSpot (Java 8) via JNI and loads Forge 1.8.9 mods against stand-in `net.minecraft.*`,
`net.minecraftforge.*` and `net.minecraftforge.fml.*` classes whose methods call into Rust. Rust owns world, entities,
networking and rendering. Mod GL goes to a GL-compatible draw layer. The goal is general compatibility, not special-casing mods.

## Corpus

| Jar | sha256 | Loader type | Class file version | Size |
|---|---|---|---|---|
| `RavenB4-u20+.jar` | `67049782264b77aac3bf8b32a219677919509417b46157e9b233640052f1ef02` | mixin tweaker + `@Mod` (cheat client) | 52 (Java 8); shaded Mixin is 49/50 | 2.2 MB |
| `lunarforge.jar` | `ca840fda47675047d7861ecb933c46b2cb1aad270473b7f3a0f84250ca37e94c` | coremod (`FMLCorePlugin`, 7 ASM `IClassTransformer`s) + `@Mod` | 52 | 11 MB |
| `Meowtils-2.0.1.jar` | `bd79abf372bc251f6ae461f690e89968799f55c2aa7a2b6b99e56d5d4e672916` (matches expected) | mixin tweaker only, no `@Mod`, no `mcmod.info` | 52; shaded Mixin/ASM 49/50 | 5.7 MB |
| `AccountManager-1.8.jar` | `a4a5a662d00df612221655cacdb5f7331f2b600605eb14bba850a9bc794153af` | plain `@Mod` | 52 | 507 KB |

Nothing in the corpus needs Java above 8. No `Multi-Release` jars. Native code: only Meowtils bundles JNA (`com/sun/jna/*/jnidispatch.*`,
all platforms, unused by mod logic as far as `javap` shows beyond JNA's own loader). No other native libs. No access transformers
(`*_at.cfg` / `FMLAT`) in any jar. No `SimpleNetworkWrapper`.

### Per-mod summary

**AccountManager 1.8** (`me.ksyz.accountmanager`, client-only `@Mod`, 37 classes). Adds account-switching GUIs to the
main menu. Uses `GuiScreenEvent.InitGuiEvent.Post` to inject buttons, `ActionPerformedEvent`, `DrawScreenEvent.Post`,
`TickEvent.Client/Render`, `WorldEvent.Load`. Minecraft surface is tiny (15 classes, 52 members): `Minecraft`, `Session` (+`Session$Type`),
`GuiScreen`, `GuiButton`, `GuiTextField`, `GuiSlot`, `FontRenderer`, `Gui.drawRect`, `GlStateManager`, `ServerData`. Replaces the
session by reflection (`Field.setAccessible` on `Minecraft.session`, `getDeclaredField("session")` in `CrackedAuth`). Handles
Microsoft/Xbox/Minecraft tokens: Microsoft OAuth (opens a local `http://localhost:%d/callback` server), cookie import, refresh
token, raw access token, cracked. Contains `accountmanager-ssl.jks` and `callback.html`. Only contacts `login.live.com`,
`*.xboxlive.com`, `api.minecraftservices.com`, `www.minecraft.net`. Uses `ProcessBuilder` for a native file chooser
(`ModernFileChooser`). No LWJGL GL calls (only `Keyboard.enableRepeatEvents`). This is the easiest mod to load and the best first
GUI test. **It touches credentials: rustmc owns auth (Prism), so `Session` must be a facade and the real access token must not
be handed to mods by default.**

**Meowtils 2.0.1** (`wtf.tatp.meowtils`, 447 own classes). `MixinTweaker` via manifest `TweakClass` + `MixinConfigs=mixins.meowtils.json`,
`ForceLoadAsMod=true`, `ModSide=CLIENT`, `FMLCorePluginContainsFMLMod=false`. **Zero references to `net.minecraftforge.*` or
`cpw.mods.fml` in its own code** and no `@SubscribeEvent`: it has its own event bus (`wtf.tatp.meowtils.event.api`) fed by 35 mixins
(81 injections) into 33 distinct `net.minecraft` classes. Features: Hypixel stats/overlay (Hypixel/Abyss/Urchin APIs), HUD editor,
chat commands (`/namemc`, `/laby`, `/plancke`...), nick bot, sound loader, auto-updater (`UpdateManager` downloads a jar from GitHub and
launches it with `ProcessBuilder`; must be neutralised), runtime `ExtensionClassLoader` (loads further jars), log uploader
(`api.mclo.gs`). `Main-Class` is an installer Swing frame (irrelevant at runtime). Refmap maps MCP selectors to SRG.

**LunarForge 0.1.0** (`com.example.lunarforge`, 549 own classes + shaded TwelveMonkeys ImageIO ~400 classes). "Lunar Client remake".
`FMLCorePlugin=...LunarForgeCorePlugin` with `FMLCorePluginContainsFMLMod=true`; seven hand-written ASM transformers (see Coremod
section) plus a normal `@Mod` with the biggest `@SubscribeEvent` footprint (about 45 distinct event classes). 32 HUD modules (FPS, CPS, keystrokes, armor status,
potion effects, scoreboard, coordinates...), 36 "mechanic" modules (crosshair builder, zoom, freelook, fog, FOV, toggle sneak, chat tools,
chat image preview), 22 "visual" modules (item physics, 2D items, 3D skins, motion blur, menu blur, hit colour, 1.7 animations), Hypixel
Bedwars/quickplay helpers, a full custom GUI (home screen, locker, emote wheel) and a cosmetics system (OBJ/Geo models, Bobj emotes) that
talks to Lunar's real servers: `authenticator.lunarclientprod.com` (login using the Minecraft session via an encryption-request/joinServer
flow), `websocket.lunarclientprod.com` (protobuf cosmetic + heartbeat services, sends os.name/os.arch/os.version, impersonates a "Lunar
Client" User-Agent), `api.lunarclientprod.com/launcher/launch`, `textures.lunarclientcdn.com`, `thirdpartycache.lunarclientprod.com`,
`skins.mcstats.com`, `i.imgur.com`, `lunr.pics`. Plugin channels `lunar:apollo`, `apollo:json`, `lunarclient:pm` (via `C17PacketCustomPayload`
and `FMLNetworkEvent.CustomPacketRegistrationEvent` / `ClientCustomPacketEvent`, `FMLEventChannel`). Probes OptiFine reflectively
(`Class.forName("net.optifine.shaders.Shaders")`, `shadersmod.client.Shaders`, `net.optifine.gui.`) and must see `ClassNotFoundException`.

**Raven B4 (ClickGUI) u20+** (`xyz.daikirai`, ~280 own classes + shaded Mixin). Mixin tweaker (`TweakClass=MixinTweaker`,
`MixinConfigs=clickgui.mixins.json`) + client-only `@Mod modid=clickgui`. A combat/movement cheat client: combat (KillAura, AutoBlock, Reach,
Velocity, AutoClicker, WTap...), movement (Fly, Speed, Scaffold, Timer, LongJump...), network (Blink, FakeLag, Backtrack, Limbo), render
(ESP, Chams, Xray, Tracers, Radar, Nametags, shaders/glow), player, minigames, plus a scripting engine (`xyz.daikirai.script.*`,
reflection-based `ScriptInvoker`/`ScriptReflection`/`ScriptCrypto`/`ScriptManager` which creates `URL`s) and `java.awt.Robot` in ClickAssist. 15 mixins / 55 injections;
builds and sends raw packets (`C03PacketPlayer` variants, `C07/C08/C02/C0B/C0E/C10`...) and inspects incoming ones
(`S12/S08/S14/S27/S06/S22/S23...`). Few distinct external URLs (plancke.io only), no obvious exfiltration in strings, but the script engine makes
that unverifiable statically.

## Merged API surface (ranked)

Numbers: "mods" = how many of the 4 jars reference it, "sites" = total `javap` call sites (shaded Mixin/ASM/JNA/ImageIO excluded from
the ranking). Source tables regenerate with `report.py`.

### Naming: SRG, not MCP, not notch

Every mod is compiled and reobfuscated for the Forge 1.8.9 runtime. In `net.minecraft.*` references, class names are the deobfuscated
vanilla names (`net.minecraft.client.Minecraft`), while members that have an SRG id use it: **`func_71410_x`, `field_71439_g`**.
Reference counts by namespace (rest are unmapped names such as `<init>`, enum constants, Forge-added members):

| Mod | SRG members | other (`<init>`, enum consts, Forge-added) |
|---|---|---|
| Raven | 1059 | 165 |
| Lunar | 738 | 91 |
| Meowtils | 488 | 86 |
| AccountManager | 44 | 8 |

Mods that use reflection pass both names (`ReflectionHelper.findField(cls, "pressed", "field_74513_e")`), so the stand-ins need the
SRG names; MCP names are only needed for mixin selectors (the refmap converts them) and for the dev-name arm of the coremod lookups.
Mixin refmaps in both mixin mods map MCP selectors to SRG (`"func_78256_a(Ljava/lang/String;)I"`), so the **runtime environment must present as "searge"**.

### Top net.minecraft members (SRG)

| # | Member | Mods | Sites | Meaning |
|---|---|---|---|---|
| 1 | `Minecraft.func_71410_x()` | 4 | 642 | `getMinecraft()` |
| 2 | `Minecraft.field_71439_g` | 3 | 1758 | `thePlayer` (EntityPlayerSP) |
| 3 | `Minecraft.field_71441_e` | 3 | 456 | `theWorld` (WorldClient) |
| 4 | `Minecraft.field_71474_y` | 3 | 188 | `gameSettings` |
| 5 | `Minecraft.field_71462_r` | 4 | 148 | `currentScreen` |
| 6 | `Minecraft.field_71466_p` | 4 | 146 | `fontRendererObj` |
| 7 | `Minecraft.func_110434_K()` | 3 | 147 | `getTextureManager()` |
| 8 | `Minecraft.func_147108_a(GuiScreen)` | 4 | 71 | `displayGuiScreen` |
| 9 | `IChatComponent.func_150260_c()` / `func_150254_d()` | 4 | 68 / 11 | `getUnformattedText` / `getFormattedText` |
| 10 | `FontRenderer.func_78256_a(String)` / `field_78288_b` | 4 | 63 / 49 | `getStringWidth` / `FONT_HEIGHT` |
| 11 | `WorldRenderer.func_181662_b(DDD)` + `func_181675_d()` | 3 | 281 each | `pos(x,y,z)` + `endVertex()` |
| 12 | `ItemStack.func_77973_b()` | 3 | 278 | `getItem` |
| 13 | `GlStateManager.func_179140_f/131_c/121_F/094_E/114_b/147_l` | 3-4 | 20-136 | disableLighting, color, popMatrix, pushMatrix, rotate, enableBlend |
| 14 | `Gui.func_73734_a(IIIII)` | 4 | 33 | `drawRect` |
| 15 | `EntityPlayerSP.field_71071_by` (`inventory`), `Minecraft.field_71412_D` (`mcDataDir`), `Minecraft.func_152344_a(Runnable)` (`addScheduledTask`), `GuiScreen.func_73863_a(IIF)` (`drawScreen`), `Minecraft.func_147104_D()` (`getCurrentServerData`) | 3-4 | 12-113 | |

Top classes by number of mods then referencing classes: `Minecraft`, `GlStateManager`, `FontRenderer`, `IChatComponent`, `GuiScreen`, `Gui`,
`ServerData` (all 4); `EntityPlayerSP`, `WorldClient`, `ItemStack`, `EntityPlayer`, `ResourceLocation`, `GameSettings`, `Entity`, `BlockPos`,
`TextureManager`, `InventoryPlayer`, `MovingObjectPosition`, `Items`, `EntityLivingBase`, `IBlockState`, `MathHelper`, `KeyBinding`, `Item`,
`Block`, `Blocks`, `ScaledResolution`, `PlayerCapabilities`, `Vec3` (3). Package weight (distinct classes referenced, 4 mods merged):
`client.renderer` 76, `network.play` 75 (packets), `client.gui` 69, `client.resources` 15, `entity.monster` 15, `entity.item` 13, `client.multiplayer` 12,
`entity.player` 12, `client.particle` 10. Raven alone references 296 distinct MC classes / 1212 members; Lunar 238 / 829; Meowtils 157 / 562.

Packets referenced (as classes, constructed or inspected): `C03PacketPlayer` (+`C04/C05/C06`), `C01PacketChatMessage`, `C02PacketUseEntity`,
`C07PacketPlayerDigging`, `C08PacketPlayerBlockPlacement`, `C09PacketHeldItemChange`, `C0A/C0B/C0D/C0E/C0F`, `C10`, `C13`, `C16`, `C17PacketCustomPayload`
(Lunar, Raven), `C19PacketResourcePackStatus`; server side `S02PacketChat`, `S19PacketEntityStatus` (all 3), `S3B/3C/3D/3E` scoreboard packets, `S25`, `S2A`,
`S45PacketTitle`, `S47`, `S48PacketResourcePackSend`, `S12`, `S08`, `S14`, `S06`, `S27`, `S22`, `S23`, `S29`.

### Top Forge events (`@SubscribeEvent`)

| Event | Mods | Handlers |
|---|---|---|
| `fml.common.gameevent.TickEvent$ClientTickEvent` | 3 | 51 |
| `fml.common.gameevent.TickEvent$RenderTickEvent` | 3 | 20 |
| `event.entity.EntityJoinWorldEvent` | 2 | 32 |
| `client.event.RenderWorldLastEvent` | 2 | 28 |
| `client.event.MouseEvent` | 2 | 21 |
| `client.event.GuiOpenEvent` | 2 | 9 |
| `event.world.WorldEvent$Load` | 2 | 8 |
| `event.entity.player.AttackEntityEvent` | 2 | 8 |
| `client.event.ClientChatReceivedEvent` | 2 | 7 |
| `fml.common.gameevent.InputEvent$KeyInputEvent` | 2 | 4 |
| `client.event.GuiScreenEvent$MouseInputEvent$Pre` / `DrawScreenEvent$Post` | 2 | 4 / 2 |
| `PlayerUseItemEvent$Finish`, `DrawBlockHighlightEvent`, `RenderLivingEvent$Specials$Pre`, `RenderPlayerEvent$Pre` | 2 | 2 each |
| `client.event.RenderGameOverlayEvent$Pre/Post/Text` (+`ElementType`) | 1 (Lunar) | 4/6/1 |
| `FMLNetworkEvent$ClientConnectedToServerEvent`, `ClientDisconnectionFromServerEvent`, `ClientCustomPacketEvent`, `CustomPacketRegistrationEvent` | 1 | 5/5/1/1 |
| `EntityViewRenderEvent$FOVModifier/FogColors/FogDensity/RenderFogEvent`, `FOVUpdateEvent`, `RenderHandEvent`, `TextureStitchEvent$Pre/Post`, `PlaySoundEvent`, `ItemTooltipEvent`, `PlayerInteractEvent`, `ChunkEvent$Load`, `LivingUpdateEvent`, `PlayerTickEvent`, `InputEvent$MouseInputEvent`, `GuiScreenEvent$InitGuiEvent$Post`/`ActionPerformedEvent`/`KeyboardInputEvent` | 1 | 1-2 |

Event members read most: `EntityJoinWorldEvent.entity/world`, `MouseEvent.button/buttonstate/dwheel` + `setCanceled`, `GuiOpenEvent.gui`,
`AttackEntityEvent.target/entityPlayer`, `ClientChatReceivedEvent.message/type`, `RenderWorldLastEvent.partialTicks`, `TickEvent.phase`
(`START`/`END`), `RenderTickEvent.renderTickTime`, `GuiScreenEvent.*.gui`, `InitGuiEvent$Post.buttonList`.

Other Forge/FML API used: `MinecraftForge.EVENT_BUS.register/unregister/post` (3 mods), `FMLCommonHandler.instance().bus()`, `ReflectionHelper.findField/
getPrivateValue/findMethod/setPrivateValue` (3 mods, 25 sites), `ClientRegistry.registerKeyBinding`, `FMLPreInitializationEvent.getModLog/
getSuggestedConfigurationFile`, `@Mod`/`@Mod.EventHandler`, `Side`, `FMLLaunchHandler.side()`, `GuiButtonExt`, `GuiModList`, `ProgressManager`
(+`$ProgressBar`), `SplashProgress`, `ForgeHooksClient.handleCameraTransforms`, `GuiIngameForge`, `FMLEventChannel`, `NetworkRegistry`, `FMLProxyPacket`,
`IFMLLoadingPlugin` (`@Name`, `@MCVersion`, `@SortingIndex`, `@TransformerExclusions`). No `cpw.mods.fml` (1.7) references anywhere:
all four are 1.8.9 `net.minecraftforge.fml` mods. No blocks/items/entities/recipes registered, no `GameRegistry`, no
capabilities, no config GUI factories: **the corpus is entirely client-utility style**; "content mod" registry surface is not covered by it.

### GL entry points (LWJGL 2)

Fixed-function immediate mode dominates; no VBO/VAO use:

- State: `GL11.glEnable/glDisable` (113/118), `glBlendFunc`, `glDepthMask`, `glDepthFunc`, `glShadeModel`, `glLineWidth`, `glColor4f`, `glColorMask`, `glCullFace`, `glPolygonOffset`, `glScissor`, `glHint`, `glIsEnabled`
- Matrices: `glPushMatrix/glPopMatrix`, `glTranslated/f`, `glScaled/f`, `glRotated/f`, `glMatrixMode`, `glLoadIdentity`, `glPushAttrib/glPopAttrib`, `glPushClientAttrib/glPopClientAttrib`
- Immediate mode: `glBegin/glEnd` (40), `glVertex3d` (91), `glVertex2d/2f/3f`, `glTexCoord2f/2d`, `glNormal3f`
- Display lists: `glNewList/glEndList/glCallList`
- Textures: `glTexParameteri` (49), `glBindTexture`, `glGenTextures`, `glPixelStorei`, `glGetTexLevelParameteri`, `GL13.glActiveTexture`
- Queries: `glGetInteger/glGetFloat`
- Shaders (`GL20`): `glCreateShader/glShaderSource/glCompileShader/glGetShaderi/glCreateProgram/glAttachShader/glLinkProgram/glUseProgram/glGetUniformLocation/glUniform1i/1f/2f/3f/4f/glDeleteShader/glDeleteProgram` (Raven's ESP/glow/blur shaders, Lunar's post shader)
- FBO: `ARBFramebufferObject.glBindFramebuffer`, `ContextCapabilities.GL_ARB_framebuffer_object/GL_EXT_framebuffer_object` (capability probing: stand-ins must report them)
- LWJGL non-GL: `Keyboard` (`isKeyDown`, `getKeyName`, `getKeyIndex`, `enableRepeatEvents`, `getEventKey/State`, `isRepeatEvent`), `Mouse` (`isButtonDown`, `getX/Y`, `getEventDWheel`, `getEventButton/State`, `getEventX/Y`), `Display.getWidth/getHeight/isCreated`, `BufferUtils.create*Buffer`, `Sys.openURL`, `util.glu.Project.gluProject`, `util.vector.Vector3f`

### Mixin targets (class, then points)

34 distinct classes; Minecraft-level hot paths first (shared by Raven and Meowtils = the real-world common core):
`Minecraft` (`runTick` HEAD/RETURN + INVOKE `EntityRenderer.getMouseOver`; `clickMouse` HEAD + INVOKE `PlayerControllerMP.clickBlock`; `rightClickMouse`,
`middleClickMouse`, `startGame` HEAD/TAIL, `runGameLoop` HEAD/TAIL, `loadWorld`, `displayGuiScreen`, `sendClickBlockToController`), `EntityRenderer`
(`orientCamera` HEAD/RETURN + INVOKE `WorldClient.rayTraceBlocks`/`Vec3.distanceTo`, `getMouseOver` INVOKE `List.size()` ordinal 0, `renderWorldPass`
INVOKE `disableLightmap`, `setupOverlayRendering` TAIL, `hurtCameraEffect` ModifyConstant), `ItemRenderer.renderItemInFirstPerson` (FIELD `itemToRender`
ordinal 0, INVOKE `getItemUseAction`/`getSwingProgress`), `NetworkManager` (`channelRead0` HEAD + INVOKE `Packet.processPacket`, `sendPacket` HEAD),
`PlayerControllerMP` (`attackEntity`, `clickBlock`, `onPlayerRightClick` HEAD), `EntityPlayerSP` (`onUpdate` INVOKE super+RETURN, `onUpdateWalkingPlayer`, `onLivingUpdate` FIELD
`MovementInput.moveForward/moveStrafe`, `updateEntityActionState`), `EntityPlayer` (`attackTargetEntityWithCurrentItem` with FIELD `motionX/motionZ` and
INVOKE `setSprinting`, `isBlocking`, `getItemInUseCount`, `getDisplayName`), `Entity` (`moveEntity` ModifyVariable STORE, `moveFlying` FIELD `rotationYaw`, `setAngles`),
`EntityLivingBase.jump`, `RenderPlayer.doRender`, `RendererLivingEntity` (`doRender` FIELD `rotationPitch`, `renderLayers` INVOKE `doRenderLayer`), `FontRenderer`
(`getStringWidth`, `renderString`/`renderStringAtPos` ModifyVariable HEAD), `MovementInputFromOptions.updatePlayerMoveState`.
Meowtils only: `World.setBlockState`, `Chunk.fillChunk` TAIL, `Block.getActualState`, `WorldInfo.getWorldTime`, `WorldClient.spawnEntityInWorld`,
`KeyBinding` (`isKeyDown`, `isPressed`, `setKeyBindState`), `GuiChat`, `GuiContainer` (`drawSlot`, `handleMouseClick`), `GuiIngame.renderHotbarItem`,
`GuiPlayerTabOverlay.getPlayerName`, `EffectRenderer`, `AbstractClientPlayer` (cape/skin), `ActiveRenderInfo.updateRenderInfo`, `NetHandlerPlayClient.handleResourcePack`,
`LayerArmorBase.renderLayer`, `RenderEntityItem.doRender`, `RenderManager.cacheActiveRenderInfo`, `ItemArmor`, `SoundHandler`. `@Accessor`/`@Invoker`/`@Shadow` on:
`Minecraft.timer/leftClickCounter/clickMouse/rightClickMouse`, `PlayerControllerMP.curBlockDamageMP/blockHitDelay/syncCurrentPlayItem`,
`EntityPlayerSP.serverSprintState/serverSneakState/lastReportedPos*/positionUpdateTicks/sprintToggleTimer`, `EntityLivingBase.jumpTicks`,
`GuiNewChat.drawnChatLines/scrollPos`, `GuiChest.lowerChestInventory`, `SoundHandler.sndManager`, `GuiContainer.handleMouseClick`.

### Reflection targets (SRG)

`KeyBinding.pressed`/`field_74513_e` and `field_74512_d`, `field_151474_i`; `Minecraft.rightClickDelayTimer`/`field_71467_ac`, `leftClickCounter`/`field_71429_W`;
`EntityLivingBase.jumpTicks`/`field_70773_bE`; `PlayerControllerMP.curBlockDamageMP`/`field_78770_f`, `blockHitDelay`/`field_78781_i`; `EntityRenderer.thirdPersonDistance`/`field_78490_B`,
`shaderIndex`/`field_147713_ae`, `shaderResourceLocations`/`field_147712_ad`, `loadShader`/`func_175069_a`; `NetHandlerPlayClient.doneLoadingTerrain`/`field_147309_h`;
`World.updateLCG`/`field_73005_l`, `activeChunkSet`/`field_72993_I`; `RenderGlobal.renderInfos`/`field_72755_R` + `ContainerLocalRenderInformation.renderChunk`/`field_178036_a`;
`RendererLivingEntity.layerRenderers`/`field_177097_h`; `EntityList.stringToClassMapping`/`field_75625_b`; `GuiScreenBook.bookPages`/`field_146483_y`, `bookObj`/`field_146474_h`;
`ItemPotion/ItemFood.potionId`/`field_77851_ca`, `alwaysEdible`/`field_77852_bZ`; `EntityArrow.inGround`/`field_70254_i`; `SoundManager.sndSystem`/`field_148620_e`;
`GuiNewChat`/`S02` `message`/`field_146304_f` (chat); `Mouse` internal `button/buttons/buttonstate` (LWJGL internals!); `Minecraft.session`.
Beyond named targets, Raven's `ScriptReflection`/`ScriptInvoker` and Meowtils' `ReflectUtil`/`HudEntry`/`ConfigManager` do arbitrary `setAccessible` over mod-owned and
MC classes. **Real field storage matters: reflection reads/writes Java fields, so a field backed only by Rust getters will not satisfy these.**

### Network channels

`lunar:apollo`, `apollo:json`, `lunarclient:pm` (Lunar, `C17PacketCustomPayload` + `REGISTER`), `FMLEventChannel.register`, `FMLProxyPacket`. No `SimpleNetworkWrapper`
users and no FML handshake dependence in the corpus. Raven `ViewPacketsModule` only inspects `C17`.

## Loader types and what each needs

1. **Plain `@Mod`** (AccountManager; the `@Mod` halves of Lunar and Raven): `mcmod.info` + `@Mod` annotation scan, `@Mod.EventHandler` for
   `FMLPreInitializationEvent`, `MinecraftForge.EVENT_BUS`, `FMLCommonHandler.bus()`, `@SubscribeEvent` dispatch (ASM-generated listeners in real Forge,
   reflection fine for a start).
2. **Mixin tweaker, no coremod** (Meowtils, Raven): manifest `TweakClass: org.spongepowered.asm.launch.MixinTweaker`. **This needs a real
   `net.minecraft.launchwrapper` (`Launch.classLoader`, `LaunchClassLoader`, `ITweaker`, `IClassTransformer`, `LogWrapper`)** because the shaded Mixin
   (0.7.x, `MixinServiceLaunchWrapper`, `MixinPlatformAgentFML`, `LaunchClassLoaderUtil` reflecting into `LaunchClassLoader` fields) hard-codes them. The
   jar also carries `META-INF/services/...IMixinService` and an `ObfuscationServiceMCP`. Practical consequence: stand-ins must be loaded *through*
   `LaunchClassLoader` so the transformer chain sees them.
3. **FML coremod** (Lunar): `FMLCorePlugin` -> `IFMLLoadingPlugin` (`getASMTransformerClass`, `@MCVersion`, `@Name`, `@SortingIndex`, `@TransformerExclusions`),
   transformers are `IClassTransformer`s taking raw `byte[]` (ASM tree API) keyed by transformed name.

### Lunar coremod: what is patched (inferred from the transformers' string constants and descriptors, not from decompiled logic)

All patching is by SRG name **with an MCP fallback** (e.g. `hurtCameraEffect`/`func_78482_e`, `clickMouse`/`func_147116_af`, `orientCamera`/`func_78467_g`):

- `OneSevenTransformer`: `Minecraft.clickMouse/func_147116_af`, `rightClickMouse/func_147121_ag`, `sendClickBlockToController`, `ItemRenderer.renderItemInFirstPerson`,
  `EntityRenderer.hurtCameraEffect/func_78482_e`, `RenderItem.renderItem/func_180454_a`, `LayerHeldItem`, `getMaterial/func_149688_o`; redirects block-hit and use-action logic to `OneSevenHooks`.
- `ModuleHooksTransformer`: `RenderEntityItem.doRender/func_76986_a`, `GameSettings.saveOptions/func_74303_b`, `setOptionFloatValue/func_74304_a`, `EntityArrow.onUpdate/func_70071_h_`,
  `attackEntityFrom/func_70097_a`, `ForgeHooksClient.handleCameraTransforms`, `GlStateManager.translate/rotate`.
- `CameraCombatTransformer`: `EntityRenderer.orientCamera/updateCameraAndRender`, `Entity.setAngles`, `hurtCameraEffect`, `GuiIngame.renderScoreboard`,
  `NetHandlerPlayClient.handleEntityStatus(S19)`, fields `rotationYaw/Pitch`, `prevRotationYaw/Pitch`, `mouseSensitivity/field_74341_c`.
- `RenderHooksTransformer`: the widest one: `EffectRenderer.addBlockDestroyEffects/addBlockHitEffects`, `TileEntitySkull` rendering, `RenderPlayer`, `RendererLivingEntity.doRender/
  renderLayers`, `WorldRenderer` colour/position, `EntityFX`, `RenderGlobal` bounding boxes, `GuiScreen` tooltips (`drawHoveringText`), `FontRenderer.drawString` with
  position/colour; `/Shaders` interop.
- `PortHooksTransformer`: `GuiNewChat.drawChat/setChatLine/func_146237_a` (chat tagging, `ChatSendEvent`), `GuiIngame.drawActivePotionEffects`, `drawLine`, `FontRenderer.drawString*`,
  `NetHandlerPlayClient` window packets (`S2EPacketCloseWindow`), `NetworkPlayerInfo` names, `MathHelper.floor_float`.
- `TabListTransformer`: `GuiPlayerTabOverlay.renderPlayerlist/getPlayerName/drawPing`, `Gui.drawRect/func_73734_a`, `FontRenderer.getStringWidth/drawStringWithShadow`,
  `KeyBinding.isKeyDown`, `Ordering` sort call, **`net.minecraftforge.client.GuiIngameForge`** (a Forge class gets patched too).
- `SplashTransformer`: **`net.minecraftforge.fml.client.SplashProgress`, `ProgressManager`, `ProgressManager$ProgressBar`** (takes over the FML loading screen).

## Implementation order for the stand-in layer

### Stage 0: host (needed by everything)

JNI invocation of Java 8, a faithful `LaunchClassLoader` + `Launch.blackboard` + tweaker/transformer chain, `mcmod.info`/`@Mod` discovery, the FML lifecycle events
(`FMLPreInitializationEvent` with `getModLog`/`getSuggestedConfigurationFile`, init, postinit), `EventBus` (`register`, `unregister`, `post`, `Event`, `Cancelable`, `@SubscribeEvent`, `EventPriority`),
`MinecraftForge.EVENT_BUS`, `FMLCommonHandler`, `Side`, `ReflectionHelper`/`ObfuscationReflectionHelper`, `Loader`/`Loader.isModLoaded`. Open question to settle in FG2: reuse the real Forge universal jar's non-Minecraft
classes (EventBus, fml.common.*) unmodified vs reimplement. Reusing is far less work but those classes reference a large `net.minecraft` surface (`ForgeHooks`, `GuiIngameForge`, `ForgeHooksClient`) that the stand-ins must then satisfy at link time (lazily resolved, so only paths actually used matter).

### Stage 1: plain event-driven mod loads

Minimum `net.minecraft` set (SRG names, real Java fields because of reflection): `Minecraft` (singleton, `thePlayer`, `theWorld`, `currentScreen`, `gameSettings`, `fontRendererObj`, `mcDataDir`,
`getTextureManager`, `getCurrentServerData`, `addScheduledTask`, `timer`, counters, `session`), `Entity`/`EntityLivingBase`/`EntityPlayer`/`EntityPlayerSP` (position, rotation, motion, `inventory`, `capabilities`, `isSprinting/Sneaking`), `World`/`WorldClient`
(`getBlockState`, `rayTraceBlocks`), `ItemStack`/`Item`/`Items`/`Blocks`/`Block`/`IBlockState`, `BlockPos`, `Vec3`, `MovingObjectPosition`, `MathHelper`, `IChatComponent`/`ChatComponentText`/`ChatStyle`/`EnumChatFormatting`,
`ResourceLocation`, `KeyBinding` + `ClientRegistry.registerKeyBinding`, `GameSettings`, `Session`. Events to fire: `ClientTickEvent` (START/END), `RenderTickEvent`, `PlayerTickEvent`, `InputEvent.Key/Mouse`, `MouseEvent`, `EntityJoinWorldEvent`,
`WorldEvent.Load/Unload`, `ChunkEvent.Load`, `ClientChatReceivedEvent`, `AttackEntityEvent`, `PlayerInteractEvent`, `PlayerUseItemEvent`, `LivingUpdateEvent`, `FMLNetworkEvent.*`. `LWJGL` `Keyboard`/`Mouse`/`Display` shims.
Milestone: AccountManager minus rendering (its buttons need Stage 2).

### Stage 2: GUI/HUD mods

`GuiScreen` (full lifecycle: `initGui`, `drawScreen` func_73863_a, `mouseClicked` func_73864_a, `keyTyped`, `handleMouseInput` func_146274_d, `actionPerformed`, `buttonList`, `width/height` fields), `Gui.drawRect` func_73734_a,
`drawString/drawCenteredString/drawTexturedModalRect`, `GuiButton` (id `field_146127_k`, enabled `field_146124_l`, visible `field_146125_m`), `GuiTextField`, `GuiSlot`, `GuiYesNo` family, `FontRenderer`
(`getStringWidth`, `drawString`, `drawStringWithShadow`, `FONT_HEIGHT`, `listFormattedStringToWidth`), `ScaledResolution`, `GlStateManager` (full surface; trivially mapped to the draw layer), `Tessellator`/`WorldRenderer` (`begin`,
`pos`/func_181662_b, `tex`, `color`, `endVertex`/func_181675_d, `draw`), `TextureManager.bindTexture`, `DynamicTexture`/`TextureUtil` (Lunar cosmetics, skins), `RenderItem`, `RenderHelper`, `EnumChatFormatting`.
Events: `GuiScreenEvent` (Init/Draw/Mouse/Keyboard/ActionPerformed), `GuiOpenEvent`, `RenderGameOverlayEvent` (`Pre`/`Post`/`Text`, `ElementType` including `ALL`, `HOTBAR`, `CROSSHAIRS`, `TEXT`), `RenderWorldLastEvent`, `RenderLivingEvent`, `RenderPlayerEvent`,
`DrawBlockHighlightEvent`, `EntityViewRenderEvent`, `FOVUpdateEvent`, `RenderHandEvent`, `TextureStitchEvent`, `ItemTooltipEvent`. GL layer to provide for these mods, in decreasing order of use: state toggles
and matrix stack, `glBegin/glVertex*/glEnd` recorded into batches, textures (`glTexParameteri`, `glBindTexture`), `glScissor`, display lists (record + replay), then `GL20` shaders and FBOs.
**GL20 is the cliff**: mods hand-write GLSL (Raven ESP/glow/blur, Lunar post-process), usually `#version 120`, which wgpu/naga cannot consume directly and which has no Metal equivalent without a GLSL 1.20 -> WGSL/MSL translation step. Defer: report
`ContextCapabilities.GL_ARB_framebuffer_object=false`/shaders unsupported so mods take their fallback branch where they probe (Lunar does; Raven's `ShadersModule` can simply be disabled).
Milestone: AccountManager fully working, then Lunar's HUD modules (GuiIngameForge overlay pipeline), then the Lunar custom GUIs.

### Stage 3: mixin / coremod mods

**Core implication**: Mixin and ASM transformers rewrite the *bytecode* of the target classes. Therefore the stand-ins must be real classes whose
**names, method names, descriptors, and (for non-HEAD points) internal instruction shapes match vanilla 1.8.9 in the SRG namespace**:

1. Class names stay `net.minecraft.*` (deobfuscated vanilla names). Methods/fields keep their `func_`/`field_` names; descriptors are bit-exact (including `Lnet/minecraft/...;` types). Any class a mod mixes into must be loadable by `LaunchClassLoader` and not `final`.
2. `HEAD`/`RETURN`/`TAIL` injects, `@Accessor`, `@Invoker`, `@Shadow` and `@Overwrite` only need the method/field to exist with the right descriptor and real Java storage.
   That is the majority of the 136 mixin annotations found and is cheap even if the body just calls `native`.
3. `INVOKE`/`FIELD`/`STORE`/ordinal injection points and `@Redirect` need the vanilla **call graph inside the body**: e.g. `Minecraft.runTick` must contain a call to
   `EntityRenderer.getMouseOver(F)`, `EntityPlayer.attackTargetEntityWithCurrentItem` must read `motionX/motionZ` and call `setSprinting`, `EntityRenderer.getMouseOver` must call `List.size()` at ordinal 0,
   `ItemRenderer.renderItemInFirstPerson` must read `itemToRender`. A body that is just a JNI trampoline fails with `InvalidInjectionException` (mixin `required: true` in both configs, so the game would abort).
   Mitigation: for the ~25 "shape-critical" methods listed above, ship vanilla-shaped Java bodies (derived from the user's own legally obtained 1.8.9 client jar at setup time, as rustmc already does for assets) with the inner calls delegating to Rust-backed stand-ins. Do not redistribute Mojang bytecode.
4. Coremod transformers work on `byte[]` by *name*: the stand-ins must be served to `IClassTransformer`s under their vanilla names, and `GuiIngameForge`, `ForgeHooksClient`, `SplashProgress`, `ProgressManager` must exist as real, patchable classes.
5. Callbacks cross JNI per call. Hot-path mixin targets (`runTick`, `channelRead0`/`sendPacket`, `World.setBlockState`, `Chunk.fillChunk`, `Entity.moveEntity`, `FontRenderer.getStringWidth`, `KeyBinding.isKeyDown`) are called thousands of times per second: keep the Java side as thin state mirrors, with Rust pushing updates, instead of a JNI call per read.

Order within stage 3: (a) Meowtils-style accessor/HEAD-only mixins on GUI classes; (b) `NetworkManager` + packet classes (Rust decodes -> constructs Java packet objects -> `channelRead0` path -> `processPacket`; outbound `sendPacket` -> Rust encodes), which Raven requires to function at all (about 40 packet classes with getters); (c) `Minecraft`, `EntityPlayerSP`, `PlayerControllerMP` input/movement shapes; (d) render-path mixins (`EntityRenderer`, `ItemRenderer`, `RendererLivingEntity`, `RenderPlayer`, layers); (e) world/chunk mixins; (f) Lunar's coremod stack.

### Difficulty ranking

1. **LunarForge** (hardest). Seven ASM transformers that patch rendering internals by name with SRG+MCP fallbacks, plus Forge-class patching (`GuiIngameForge`, `SplashProgress`), 45 event types, an enormous
   `GlStateManager`/`WorldRenderer`/`RenderItem`/model surface (cosmetics are OBJ/Geo models drawn with raw GL), shaded ImageIO (needs `java.awt` + `javax.imageio` in the JRE, fine on a full JRE8 but not a trimmed one),
   protobuf/WebSocket networking and Lunar-server auth. Patch failure is probably non-fatal per transformer (they log "N hook(s)"), so degraded loading is feasible.
2. **Raven B4**. Fewest mixins by count but they are the most intrusive: movement (`moveEntity`, `moveFlying`, `onLivingUpdate` field redirects), camera, combat (`attackTargetEntityWithCurrentItem` field redirects), network (`channelRead0` `processPacket` redirect). Needs a faithful Java-visible packet model and 296 MC classes. Plus GL20 shaders and a script engine with arbitrary reflection. It is also a cheat client: whether rustmc should support it at all is a policy question, not a technical one; technically it is the best stress test for the packet and movement path.
3. **Meowtils**. 35 mixins / 33 targets but mostly HEAD injects and accessors, so the cost is breadth (world, chunk, block, GUI, render, network classes must all be real and mixin-patchable) rather than depth. Needs the LaunchWrapper/Mixin bootstrap but no Forge API at all. Its auto-updater and extension loader need to be disabled.
4. **AccountManager**. Trivial surface; the only real work is `GuiScreen`/`GuiButton`/`GuiTextField`/`GuiSlot` and the `Session` facade.

## Loader policy: never load OptiFine

Hard rule: rustmc must never load OptiFine, nor any jar that contains it. Check **before** any class from the jar is defined and before its manifest `TweakClass`/`FMLCorePlugin`
is honoured. A jar is refused if **any** of the following holds (case-insensitive):

- file name contains `optifine` (e.g. `OptiFine_1.8.9_HD_U_M5.jar`, `preview_OptiFine_*.jar`);
- manifest values (`TweakClass`, `FMLCorePlugin`, `Main-Class`, `Implementation-Title`...) contain `optifine` (real OptiFine: `TweakClass: optifine.OptiFineForgeTweaker`);
- `mcmod.info` or `@Mod` has modid or name `optifine`;
- the jar contains a top-level or nested `optifine/` or `net/optifine/` package, `optifine/OptiFineClassTransformer`, `optifine/OptiFineForgeTweaker`, or `shadersmod/` classes bundled as OptiFine's shaders;
- a nested jar inside it (`META-INF/libraries`, `jars/`) matches any of the above (OptiFine installer-style wrappers).

On refusal log exactly one clear line at WARN and skip the jar (no partial load):

    [modloader] REFUSING to load <path>: identified as OptiFine (<reason>). rustmc never loads OptiFine.

Mods that depend on OptiFine: declared dependencies (`@Mod(dependencies = "required-after:optifine")` or `after:optifine`) -> refuse the dependent with
`[modloader] <mod> requires OptiFine, which rustmc never loads; skipping`. Soft/reflective users must be satisfied by absence: stand-ins must **not** define
`Config` (OptiFine's `Config.isShaders()` etc.), `net.optifine.*`, `shadersmod.*`; `Class.forName` for those must throw `ClassNotFoundException`. In the corpus, LunarForge probes `net.optifine.shaders.Shaders`,
`shadersmod.client.Shaders` and `net.optifine.gui.` via reflection for its shader-pack display and menu blur and has fallbacks. `tools/modscan` already reports `optifine_identity`
(file name, package, `OptiFineClassTransformer`, manifest/mcmod.info text) per jar, so the same check can be done offline in CI. Mods that merely have a module *named* "Shaders" (Raven) are not OptiFine and are not refused.

## Security and safety notes (observed statically)

- Every loaded mod runs with the full privileges of the rustmc process (in-process JVM): file system, sockets, `ProcessBuilder`. Observed process spawns: AccountManager (`ModernFileChooser`), Meowtils (`UpdateManager`: downloads and launches a jar), LunarForge (`ModuleAutoTextActions$Desktop`). Observed `java.awt.Robot` (Raven `ClickAssistModule`), `Instrumentation` agent classes (shaded Mixin `MixinAgent`, inert unless used as a javaagent).
- Plan accordingly: deny-by-default `SecurityManager`/policy, block `ProcessBuilder`/`Runtime.exec` and outbound sockets except an allow-list, withhold the real access token (`Session.func_148254_d`/`getToken`) from mods, and let Rust-side auth hand mods a session facade only.
- Endpoints seen in strings (never contacted): Lunar `authenticator.lunarclientprod.com`, `websocket.lunarclientprod.com`, `api.lunarclientprod.com`, `textures.lunarclientcdn.com`, `thirdpartycache.lunarclientprod.com`; Meowtils `api.hypixel.net`, `api.abyssoverlay.com`, `api.urchin.gg`, `api.mclo.gs`, `api.github.com`/`github.com/femboytatp/meowtils-auto-update`, `api.mojang.com`, `docs.tatp.wtf`; AccountManager Microsoft/Xbox/Mojang login hosts only. No unknown hosts or raw IPs in any jar.
