# Forge event bridge

`ClientTickBridge` posts Forge 1.8.9 client tick START and END events from the
Rust thread that owns the game loop. It takes the `rustmc.forge.EventBridge`
class from the Forge `LaunchClassLoader`, retains it as a JNI global reference,
and caches its `(Z)V` method ID. Java exceptions are returned to the caller.
The Java source is built against Forge and must be included in the loader's
classpath. The class and JVM must outlive the bridge.

This is an incremental component, not an enabled mod runtime. The client does
not yet start the JVM or call the bridge: lifecycle startup currently depends on
vanilla Java registry initialization, and Java-visible client/world state is not
backed by Rust. Wiring it into the live tick loop before those pieces exist would
make mods observe missing or divergent state. It does not support other event
classes, general mod loading, or third-party coremods.

The headless `tools/jvm-spike/run-shadow.sh` probe loads the real Forge bus and
our own test mod, posts START/END/START through this component, and checks the
mod received exactly that sequence. The probe uses Java registry bootstrap and a
limited client delegate; it does not open a game window or run the Minecraft
client loop. The test-only unattended client driver is disabled in this harness
because it expects an actual Minecraft client singleton.

`MovementView` is a narrow state bridge for the real Forge-loaded
`net.minecraft.util.MovementInput` class. A Rust owner retains one view, pushes
strafe, forward, jump and sneak before callbacks, and pulls all four fields
back afterward. The view retains Java object identity until it is dropped;
creating another view creates a distinct object. The probe exercises both
ways through a test mod and checks identity and replacement. This is not yet
connected to a Rust player or input loop; it does not mirror player, world,
settings or arbitrary fields, and the caller must choose synchronization points.

## API stubs

Rust-dispatched stubs of the whole mod API plus real vanilla GUI/event classes, and the loader that runs `@Mod` jars on them: see `docs/FORGE_STUBS.md` (build, run, implement a method, discover what to implement, test against a real mod).
