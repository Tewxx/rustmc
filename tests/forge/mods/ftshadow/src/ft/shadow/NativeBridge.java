package ft.shadow;
/** A controlled Rust callback, registered with JNI before the hooked method is called. */
public final class NativeBridge {
    public static native int offset();
}
