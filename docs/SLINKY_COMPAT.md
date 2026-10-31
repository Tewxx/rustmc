# Slinky compatibility inspection (2026-10-30)

This is a bounded, offline inspection for interoperability. No launcher, injector,
or native client payload was executed. No authentication or licensing behavior was
changed or tested. It does **not** establish that Slinky can inject into or work
with rustmc.

## Selected local artifacts

| Artifact | SHA-256 |
|---|---|
| `%USERPROFILE%/Desktop/Clients/Slinky_non_beta.exe` | `b3cd77aa7039e61e131a1d5a0ce2056b0aa2d94e9430d280cb603a2483643faa` |
| `%USERPROFILE%/Desktop/Clients/Slinky.exe` | `012ca9b77b3991e2c5cf9429d356ba220132bfb2759c0ffc72909ed528766ac5` |
| `%USERPROFILE%/.slinky/bin/slinky` and identical `slinky.dll` | `be03bbecbf14b03458de36c8b3848c1b7325ad0aeb6a61b6e0f9206fc40b4af7` |
| `%USERPROFILE%/.slinky/bin/slinky_beta` | `8d07328ca0c281e76aaaada90374255cf40273307632f338356995db1fe0e640` |

The stable launcher is a PE x86-64 Go artifact. REA's pure metadata inspection
returned Evidence `ev_f26ddcad1dc69c3f197a49229d3ede8a5d633693b3a726a1e7450dc6de73c821`
for its identity. This operation establishes embedded metadata, not runtime
behavior or authenticity.

## Confirmed launcher discovery requirement

The stable launcher has an existing local disassembly whose recorded input hash
matches the selected executable exactly:
`%USERPROFILE%/Downloads/asd/analysis/launcher/metadata.json` and
`disassembly/github.com_slinkygg_loader_inject.FindMinecraft.annotated.asm`.
The relevant instructions were independently read back from the current file
with GNU objdump and a PE RVA-to-file-offset parser:

- At RVA `0x253623`, a RIP-relative `LEA` points to ASCII `LWJGL` at RVA
  `0x34232f`, followed by length 5 and a call to the helper at RVA `0x253540`.
- At RVA `0x253662`, the same helper receives ASCII `GLFW30` at RVA
  `0x34257f`, followed by length 6.
- The helper calls the API-procedure object at image VA `0x1405dc868` with the
  class argument and a null second argument. The matching initialization
  disassembly binds that object through the `FindWindowW` literal at RVA
  `0x343b7b`.
- The caller then uses the procedure object corresponding to
  `GetWindowThreadProcessId` to obtain a process ID from the window handle.

This is static evidence of **window-class-based discovery**, rather than evidence
that merely naming the process `javaw.exe`, setting the window title to
`Minecraft 1.8.9`, or embedding HotSpot is sufficient. The inspected rustmc window
creation path in `crates/mc-client/src/input.rs` sets a title but does not specify
an LWJGL/GLFW window class.

An explicitly enabled compatibility window class may address this first gate.
It would not establish Java class-loader, hook, packet, or rendering compatibility.
Multiple real Minecraft windows could also make class-only discovery ambiguous;
the selected launcher was not run to determine its behavior in that situation.

## Payload requirements remain partly unknown

Historical native-client analysis under `Downloads/asd/analysis/reports/STATIC_JNI_BINDINGS.md`
reports Minecraft class loading with deobfuscated names and obfuscated fallback
names, accesses to player/world/settings/renderer/controller fields, a Netty
pipeline hook before `packet_handler`, and LWJGL GL function bindings. These are
useful compatibility hypotheses, **not confirmed facts about the current payloads**:
their identity has not been matched to the currently installed DLLs above.

Consequences to investigate with a controlled interoperability test:

1. Persistent, populated Java player/world/entity/inventory objects are necessary
   for field readers; a non-null Minecraft singleton is insufficient.
2. SRG names alone may be insufficient for some versions. Determine which
   deobfuscated, MCP, SRG, and vanilla-obfuscated lookup fallbacks that exact
   payload actually attempts before adding aliases or remapping.
3. A Rust packet queue is not itself a Netty `ChannelPipeline`. A client requiring
   `pipeline().addBefore("packet_handler", ...)` needs a compatible interception
   surface with actual incoming/outgoing packets flowing through it.
4. Java hook methods must execute at the real Rust tick/render/network boundaries.
   Loading or transforming a class without calling its methods proves little.
5. Native OpenGL/SwapBuffers hooks cannot be assumed to work with a wgpu-rendered
   window. A Java GL command translator and a native OpenGL context are different
   interoperability mechanisms.
6. JNI/JVMTI discovery, retransformation capability, and the required class loader
   must be tested with this exact payload. No payload-specific confirmation of
   `JNI_GetCreatedJavaVMs`, a required JVMTI startup option, or a Java-agent route
   was obtained in this inspection.

## Tooling limitations

REA CLI 6.7.0 is installed. Its doctor reported no connected Codex MCP
registration, no configured native-analysis provider, and a Windows native
self-test filesystem failure in this sandbox. Configuration and installed
engines were left unchanged. The Go artifact identity above is actual REA
Evidence; the focused disassembly/byte checks are explicitly ordinary shell-tool
evidence. A Go-tool objdump attempt was unavailable in the installed Go tool set;
the installed GNU objdump supplied the fresh static instruction check instead.

No claim of general injected-client compatibility should be made from these
results. The next acceptance test needs the exact selected client version, an
isolated rustmc process, observed discovery/class/hook failures, and validation
that ordinary multiplayer behavior is unchanged.
