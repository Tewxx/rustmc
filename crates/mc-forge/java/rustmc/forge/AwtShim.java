package rustmc.forge;

import java.awt.Desktop;
import java.awt.Toolkit;
import java.awt.datatransfer.Clipboard;
import java.awt.datatransfer.ClipboardOwner;
import java.awt.datatransfer.DataFlavor;
import java.awt.datatransfer.StringSelection;
import java.awt.datatransfer.Transferable;
import java.net.URI;

/**
 * Stand-ins for the AWT calls mods make (clipboard, browser). The JVM runs headless so AWT cannot steal the window
 * system's app delegate; rustmc.boot.AwtTransformer rewrites mod bytecode to call these, and Rust does the real work.
 */
public final class AwtShim {
    private static final Clipboard CLIPBOARD = new Clipboard("rustmc") {
        @Override public synchronized void setContents(Transferable contents, ClipboardOwner owner) {
            try {
                if (contents != null && contents.isDataFlavorSupported(DataFlavor.stringFlavor))
                    Stub.call("rustmc/forge/AwtShim.setClipboard", null, new Object[] { contents.getTransferData(DataFlavor.stringFlavor) });
            } catch (Exception e) {
                System.err.println("[forge] clipboard: " + e);
            }
            super.setContents(contents, owner);
        }

        @Override public synchronized Transferable getContents(Object requestor) {
            Object text = Stub.call("rustmc/forge/AwtShim.getClipboard", null, new Object[0]);
            return text instanceof String ? new StringSelection((String) text) : super.getContents(requestor);
        }
    };

    /** Replaces Toolkit.getSystemClipboard() (the receiver is passed as the argument). */
    public static Clipboard clipboard(Toolkit toolkit) { return CLIPBOARD; }

    /** Replaces Desktop.getDesktop()/isDesktopSupported(): there is no AWT Desktop, only {@link #browse}. */
    public static Desktop getDesktop() { return null; }
    public static boolean isDesktopSupported() { return true; }

    /** Replaces Desktop.browse(URI) (the receiver is passed first and ignored). */
    public static void browse(Desktop ignored, URI uri) { Stub.call("rustmc/forge/AwtShim.browse", null, new Object[] { uri.toString() }); }

    /** For mods that reach Desktop by reflection: the transformer swaps the class name "java.awt.Desktop" for this one. */
    public static final class DesktopShim {
        public static DesktopShim getDesktop() { return new DesktopShim(); }
        public static boolean isDesktopSupported() { return true; }
        public void browse(URI uri) { AwtShim.browse(null, uri); }
    }
}
