# Forge 1.8.9 loading contract: verified subset

This describes observed source behavior in Forge 11.15.1.2318, not implemented rustmc behavior. It is a starting point; network handshake, registries, class loading details, Mixin interaction and a complete coverage map remain open. Source paths below are relative to the Forge 1.8.9 source tree used by `tools/forgespec`.

## Discovery and ordering

- `fml/relauncher/CoreModManager.java` discovers root plugins and coremods before normal mod discovery. A jar manifest's `FMLCorePlugin` identifies an `IFMLLoadingPlugin`; `FMLCorePluginContainsFMLMod` causes the same jar to be examined for ordinary mods too. `ModSide` and `ModType` influence whether a jar is considered. A mismatching `@MCVersion` causes a core plugin to be ignored. The manager applies transformer exclusions and records `@DependsOn` and `@SortingIndex` before enqueueing the plugin. These mechanisms must remain observable to coremods; simply scanning `@Mod` classes is insufficient.
- `fml/common/Loader.java` (`identifyMods`, `sortModList`) discovers classpath, mods-directory and version-specific-directory candidates, rejects duplicate IDs and checks required mods and version ranges before sorting the active list. Missing requirements and incompatible versions fail loading. Exact ordering and error behavior need differential tests.
- `Loader.loadMods`, `preinitializeMods`, and `initializeMods` distribute construction, preinitialization, initialization, inter-mod communication, postinitialization and available state messages in that order. Object holders and item-stack holders are applied around preinitialization; registry data is frozen after the available event. A bridge must preserve the actual callbacks and ordering, not just expose equivalent names.

## Evidence and limits

`tests/forge/mods/ftlifecycle` and `ftlifecycle2` exercise part of lifecycle ordering. `ftcore` contains a controlled class transformer; on a local headless Forge dedicated server, `CORE-01` recorded value 2 after transforming a method that originally returned 1. `tools/forgetest/check-core.sh` additionally checks the transformer directly. This does not establish client loading, arbitrary transformers, or Mixin compatibility. The catalogues in this directory are generated inventories, with scanner limitations described in README.md.
