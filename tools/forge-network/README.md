# Forge network probe

The client detects the `modinfo.type = FML` status response and advertises the FML
marker on login. If status is unavailable, `RUSTMC_FORGE=1` forces Forge login;
`RUSTMC_FORGE=0` disables detection. Detection adds a bounded status query before
login, and falls back to vanilla if the server does not answer.

The current implementation supports the handshake with an unmodified Forge
1.8.9 server using the vanilla registries. It validates the registry snapshot
before acknowledging it and rejects additional mods, remapped or extra registry
entries, custom dimensions, and unsupported Forge runtime messages. It tracks
REGISTER and UNREGISTER channel names, but does not yet deliver arbitrary mod
messages or implement multipart payloads. This is not general Forge mod support;
those features require the mod runtime and registry remapping described in the
Forge architecture document.

Run `tools/forge-network/test-server.sh` for a headless integration test against
Forge 11.15.1.2318. It requires Java 8 and an installed server at
`.mc-tools/forge-server` (or `FORGE_SERVER_DIR`); `JDK8` and `RUSTMC_INPUTS`
select alternate locations. It creates a temporary offline-mode loopback server,
checks both explicit and status-detected negotiation through JoinGame, stops it,
and prints the log location. No Minecraft client window is opened.
