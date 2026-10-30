# Forge 1.8.9 inventory (work in progress)

`tools/forgespec/run.sh` regenerates these inventories from local Forge 11.15.1.2318, Minecraft 1.8.9, MCP mappings, LaunchWrapper, Mixin and Forge source patches. Input locations can be overridden with `RUSTMC_MAIN`, `PRISM_LIBS`, `JDK8` and `FORGE_SRC`. The script writes generated data into this directory.

The API and event catalogues are discovery aids, not a complete compatibility contract. The ASM 5 scanner skips five newer Mixin classes (`MixinLaunchPlugin`, `MixinTransformationService`, two `ContainerHandleModLauncherEx` classes and `module-info`); its event construction and patch-site detection is also incomplete. The loading/runtime specification, coverage mapping and validation against running Forge remain to be completed. A catalogue entry does not establish rustmc support.
