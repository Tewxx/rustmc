package ft.core;

import net.minecraft.launchwrapper.IClassTransformer;
import org.objectweb.asm.*;

/** Changes a controlled test class after LaunchClassLoader reads it. */
public class Transformer implements IClassTransformer {
    public byte[] transform(String name, String transformedName, byte[] bytes) {
        if (!"ft.core.Target".equals(transformedName) || bytes == null) return bytes;
        ClassReader reader = new ClassReader(bytes);
        ClassWriter writer = new ClassWriter(reader, 0);
        reader.accept(new ClassVisitor(Opcodes.ASM5, writer) {
            @Override public MethodVisitor visitMethod(int access, String name, String desc, String signature, String[] exceptions) {
                MethodVisitor next = super.visitMethod(access, name, desc, signature, exceptions);
                if (!name.equals("value") || !desc.equals("()I")) return next;
                return new MethodVisitor(Opcodes.ASM5, next) {
                    @Override public void visitInsn(int opcode) {
                        super.visitInsn(opcode == Opcodes.ICONST_1 ? Opcodes.ICONST_2 : opcode);
                    }
                };
            }
        }, 0);
        return writer.toByteArray();
    }
}
