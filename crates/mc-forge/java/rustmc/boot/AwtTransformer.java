package rustmc.boot;

import java.util.Iterator;
import net.minecraft.launchwrapper.IClassTransformer;
import org.objectweb.asm.ClassReader;
import org.objectweb.asm.ClassWriter;
import org.objectweb.asm.Opcodes;
import org.objectweb.asm.tree.AbstractInsnNode;
import org.objectweb.asm.tree.ClassNode;
import org.objectweb.asm.tree.LdcInsnNode;
import org.objectweb.asm.tree.MethodInsnNode;
import org.objectweb.asm.tree.MethodNode;

/** Points mods' AWT clipboard and Desktop calls at rustmc.forge.AwtShim (the JVM runs headless; see AwtShim). */
public final class AwtTransformer implements IClassTransformer {
    private static final String SHIM = "rustmc/forge/AwtShim";

    public byte[] transformedClass(String name, byte[] bytes) { return transform(name, name, bytes); }

    @Override public byte[] transform(String name, String transformedName, byte[] bytes) {
        if (bytes == null || name.startsWith("rustmc.") || name.startsWith("java.") || name.startsWith("org.spongepowered.") || !contains(bytes, "java/awt/")) return bytes;
        ClassNode node = new ClassNode();
        new ClassReader(bytes).accept(node, 0);
        boolean changed = false;
        for (Object each : node.methods) { // ASM 5 returns raw lists
            MethodNode m = (MethodNode) each;
            for (Iterator<?> it = m.instructions.iterator(); it.hasNext();) {
                AbstractInsnNode insn = (AbstractInsnNode) it.next();
                if (insn instanceof LdcInsnNode && "java.awt.Desktop".equals(((LdcInsnNode) insn).cst)) {
                    ((LdcInsnNode) insn).cst = "rustmc.forge.AwtShim$DesktopShim";
                    changed = true;
                } else if (insn instanceof MethodInsnNode) {
                    MethodInsnNode c = (MethodInsnNode) insn;
                    if (c.owner.equals("java/awt/Toolkit") && c.name.equals("getSystemClipboard")) {
                        redirect(c, "clipboard", "(Ljava/awt/Toolkit;)Ljava/awt/datatransfer/Clipboard;");
                        changed = true;
                    } else if (c.owner.equals("java/awt/Toolkit") && c.name.equals("getScreenSize")) {
                        redirect(c, "screenSize", "(Ljava/awt/Toolkit;)Ljava/awt/Dimension;");
                        changed = true;
                    } else if (c.owner.equals("java/awt/Desktop") && c.name.equals("getDesktop")) {
                        redirect(c, "getDesktop", "()Ljava/awt/Desktop;");
                        changed = true;
                    } else if (c.owner.equals("java/awt/Desktop") && c.name.equals("isDesktopSupported")) {
                        redirect(c, "isDesktopSupported", "()Z");
                        changed = true;
                    } else if (c.owner.equals("java/awt/Desktop") && c.name.equals("browse")) {
                        redirect(c, "browse", "(Ljava/awt/Desktop;Ljava/net/URI;)V");
                        changed = true;
                    }
                }
            }
        }
        if (!changed) return bytes;
        ClassWriter w = new ClassWriter(0);
        node.accept(w);
        return w.toByteArray();
    }

    private static void redirect(MethodInsnNode c, String name, String desc) {
        c.setOpcode(Opcodes.INVOKESTATIC);
        c.owner = SHIM;
        c.name = name;
        c.desc = desc;
        c.itf = false;
    }

    private static boolean contains(byte[] haystack, String needle) {
        byte[] n = needle.getBytes(java.nio.charset.StandardCharsets.ISO_8859_1);
        outer:
        for (int i = 0; i + n.length <= haystack.length; i++) {
            for (int j = 0; j < n.length; j++) if (haystack[i + j] != n[j]) continue outer;
            return true;
        }
        return false;
    }
}
