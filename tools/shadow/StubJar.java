import java.io.*;
import java.util.*;
import java.util.jar.*;
import java.util.regex.Pattern;
import org.objectweb.asm.*;
import static org.objectweb.asm.Opcodes.*;

/**
 * Local-only: copies every class of the input jars with every method body replaced by a call to
 * rustmc.forge.Stub.call("owner.name(desc)", this, args) -- Rust implements it or logs it as stubbed;
 * a null result becomes the type's zero value. Names, descriptors, fields, annotations and
 * inner-class data are untouched, so Mixin/ASM/reflection see the real shape. Enums are copied
 * unchanged: their clinit runs stubbed code that would otherwise NPE. A stub method that a real (kept or library)
 * ancestor already implements just calls super, so real code such as GuiScreen.mouseClicked still runs.
 * usage: StubJar out.jar keepRegex methods.txt lib.jar... -- in.jar...   (lib jars only resolve super constructors)
 */
public class StubJar {
    static final Map<String, ClassNode> universe = new HashMap<>();
    static class ClassNode {
        String superName; int access; List<String> ctorDescs = new ArrayList<>();
        boolean real;                              // kept as-is (or a library): its method bodies are genuine
        Set<String> concrete = new HashSet<>();    // name+desc of overridable, non-abstract instance methods
    }
    static Pattern keep;

    /** True if a real ancestor already implements name+desc; a stub must not shadow it with a no-op. */
    static boolean inheritsReal(String owner, String nameDesc) {
        for (ClassNode n = universe.get(owner); n != null; n = universe.get(n.superName)) if (n.real && n.concrete.contains(nameDesc)) return true;
        return false;
    }

    static void index(File f, boolean lib) throws IOException {
        try (JarFile jar = new JarFile(f)) {
            for (Enumeration<JarEntry> e = jar.entries(); e.hasMoreElements();) {
                JarEntry je = e.nextElement();
                if (!je.getName().endsWith(".class")) continue;
                ClassReader r = new ClassReader(jar.getInputStream(je));
                ClassNode n = new ClassNode(); n.superName = r.getSuperName(); n.access = r.getAccess();
                n.real = lib || keep.matcher(je.getName()).find() || (n.access & ACC_ENUM) != 0;
                r.accept(new ClassVisitor(ASM5) {
                    @Override public MethodVisitor visitMethod(int acc, String name, String desc, String sig, String[] ex) {
                        if (name.equals("<init>") && (acc & ACC_PRIVATE) == 0) n.ctorDescs.add(desc);
                        else if ((acc & (ACC_ABSTRACT | ACC_STATIC | ACC_PRIVATE)) == 0 && !name.startsWith("<")) n.concrete.add(name + desc);
                        return null;
                    }
                }, ClassReader.SKIP_CODE | ClassReader.SKIP_DEBUG | ClassReader.SKIP_FRAMES);
                universe.putIfAbsent(r.getClassName(), n);
            }
        }
    }

    /** Fewest parameters wins; ties go to the first declared. */
    static String pickCtor(String owner) {
        ClassNode n = universe.get(owner);
        if (n == null) throw new IllegalStateException("super class not on classpath: " + owner + " (needed by " + current + ")");
        String best = null;
        for (String d : n.ctorDescs) if (best == null || Type.getArgumentTypes(d).length < Type.getArgumentTypes(best).length) best = d;
        if (best == null) throw new IllegalStateException("no accessible constructor: " + owner);
        return best;
    }

    static void pushDefault(MethodVisitor mv, Type t) {
        switch (t.getSort()) {
            case Type.VOID: break;
            case Type.LONG: mv.visitInsn(LCONST_0); break;
            case Type.FLOAT: mv.visitInsn(FCONST_0); break;
            case Type.DOUBLE: mv.visitInsn(DCONST_0); break;
            case Type.OBJECT: case Type.ARRAY: mv.visitInsn(ACONST_NULL); break;
            default: mv.visitInsn(ICONST_0);
        }
    }

    static String current;
    static final String STUB = "rustmc/forge/Stub";
    static final String[][] BOX = { {"Z", "java/lang/Boolean"}, {"B", "java/lang/Byte"}, {"C", "java/lang/Character"}, {"S", "java/lang/Short"},
                                    {"I", "java/lang/Integer"}, {"J", "java/lang/Long"}, {"F", "java/lang/Float"}, {"D", "java/lang/Double"} };

    static void box(MethodVisitor m, Type t) {
        if (t.getSort() >= Type.ARRAY) return;
        for (String[] b : BOX) if (b[0].equals(t.getDescriptor())) m.visitMethodInsn(INVOKESTATIC, b[1], "valueOf", "(" + b[0] + ")L" + b[1] + ";", false);
    }

    /** Stub.asX turns a (possibly null) result into the primitive; references are just cast. */
    static void unbox(MethodVisitor m, Type t) {
        switch (t.getSort()) {
            case Type.VOID: m.visitInsn(POP); break;
            case Type.BOOLEAN: m.visitMethodInsn(INVOKESTATIC, STUB, "asBool", "(Ljava/lang/Object;)Z", false); break;
            case Type.LONG: m.visitMethodInsn(INVOKESTATIC, STUB, "asLong", "(Ljava/lang/Object;)J", false); break;
            case Type.FLOAT: m.visitMethodInsn(INVOKESTATIC, STUB, "asFloat", "(Ljava/lang/Object;)F", false); break;
            case Type.DOUBLE: m.visitMethodInsn(INVOKESTATIC, STUB, "asDouble", "(Ljava/lang/Object;)D", false); break;
            case Type.OBJECT: case Type.ARRAY:
                if (!t.getInternalName().equals("java/lang/Object")) m.visitTypeInsn(CHECKCAST, t.getInternalName()); break;
            default: m.visitMethodInsn(INVOKESTATIC, STUB, "asInt", "(Ljava/lang/Object;)I", false); // byte, short, char, int
        }
    }

    /** Real classes in which only these owner/Class.name methods (any overload) are replaced by Rust-dispatched stubs. */
    static final Set<String> forced = new HashSet<>();

    static void stub(ClassReader r, JarOutputStream out) throws IOException {
        stub(r, out, false);
    }

    static void stub(ClassReader r, JarOutputStream out, boolean onlyForced) throws IOException {
        current = r.getClassName();
        ClassWriter w = new ClassWriter(ClassWriter.COMPUTE_MAXS);
        r.accept(new ClassVisitor(ASM5, w) {
            @Override public MethodVisitor visitMethod(int acc, String name, String desc, String sig, String[] ex) {
                if ((acc & ACC_ABSTRACT) != 0) return super.visitMethod(acc, name, desc, sig, ex);
                if (onlyForced && !forced.contains(r.getClassName() + "." + name)) return super.visitMethod(acc, name, desc, sig, ex);
                if ((acc & ACC_ABSTRACT) != 0) return super.visitMethod(acc, name, desc, sig, ex);
                final MethodVisitor m = super.visitMethod(acc & ~ACC_NATIVE, name, desc, sig, ex);
                // A real ancestor already implements this: keep the method (Mixin targets it by name) but run the ancestor's code.
                final boolean forward = !onlyForced && (acc & (ACC_ABSTRACT | ACC_STATIC | ACC_PRIVATE)) == 0 && !name.startsWith("<") && inheritsReal(r.getSuperName(), name + desc);
                // Annotations pass through; the original body is dropped and a stub is emitted in visitEnd.
                return new MethodVisitor(ASM5) {
                    @Override public AnnotationVisitor visitAnnotation(String d, boolean v) { return m.visitAnnotation(d, v); }
                    @Override public AnnotationVisitor visitParameterAnnotation(int p, String d, boolean v) { return m.visitParameterAnnotation(p, d, v); }
                    @Override public AnnotationVisitor visitAnnotationDefault() { return m.visitAnnotationDefault(); }
                    @Override public void visitParameter(String n, int a) { m.visitParameter(n, a); }
                    @Override public void visitEnd() {
                        m.visitCode();
                        if (forward) {
                            m.visitVarInsn(ALOAD, 0);
                            int at = 1;
                            for (Type t : Type.getArgumentTypes(desc)) { m.visitVarInsn(t.getOpcode(ILOAD), at); at += t.getSize(); }
                            m.visitMethodInsn(INVOKESPECIAL, r.getSuperName(), name, desc, false);
                            m.visitInsn(Type.getReturnType(desc).getOpcode(IRETURN));
                            m.visitMaxs(0, 0);
                            m.visitEnd();
                            return;
                        }
                        boolean ctor = name.equals("<init>"), isStatic = (acc & ACC_STATIC) != 0;
                        if (ctor) {
                            String sup = r.getSuperName(), sd = pickCtor(sup);
                            m.visitVarInsn(ALOAD, 0);
                            for (Type t : Type.getArgumentTypes(sd)) pushDefault(m, t);
                            m.visitMethodInsn(INVOKESPECIAL, sup, "<init>", sd, false);
                        }
                        m.visitLdcInsn(r.getClassName() + "." + name + desc);
                        if (isStatic) m.visitInsn(ACONST_NULL); else m.visitVarInsn(ALOAD, 0);
                        Type[] args = Type.getArgumentTypes(desc);
                        m.visitLdcInsn(args.length); m.visitTypeInsn(ANEWARRAY, "java/lang/Object");
                        int slot = isStatic ? 0 : 1;
                        for (int i = 0; i < args.length; i++) {
                            m.visitInsn(DUP); m.visitLdcInsn(i); m.visitVarInsn(args[i].getOpcode(ILOAD), slot);
                            box(m, args[i]); m.visitInsn(AASTORE);
                            slot += args[i].getSize();
                        }
                        m.visitMethodInsn(INVOKESTATIC, STUB, "call", "(Ljava/lang/String;Ljava/lang/Object;[Ljava/lang/Object;)Ljava/lang/Object;", false);
                        Type ret = ctor ? Type.VOID_TYPE : Type.getReturnType(desc);
                        unbox(m, ret);
                        m.visitInsn(ret.getOpcode(IRETURN));
                        m.visitMaxs(0, 0);
                        m.visitEnd();
                    }
                };
            }
        }, ClassReader.SKIP_DEBUG);
        out.putNextEntry(new JarEntry(r.getClassName() + ".class"));
        out.write(w.toByteArray());
        out.closeEntry();
    }

    public static void main(String[] a) throws Exception {
        keep = Pattern.compile(a[1]);
        int sep = Arrays.asList(a).indexOf("--");
        List<File> libs = new ArrayList<>(), ins = new ArrayList<>();
        for (String l : java.nio.file.Files.readAllLines(new File(a[2]).toPath())) if (!l.startsWith("#") && !l.trim().isEmpty()) forced.add(l.trim());
        for (int i = 3; i < a.length; i++) if (i != sep) (i < sep ? libs : ins).add(new File(a[i]));
        for (File f : ins) index(f, false);
        for (File f : libs) index(f, true);
        int n = 0, kept = 0;
        Set<String> seen = new HashSet<>();
        try (JarOutputStream out = new JarOutputStream(new FileOutputStream(a[0]))) {
            for (File f : ins) try (JarFile jar = new JarFile(f)) {
                for (Enumeration<JarEntry> e = jar.entries(); e.hasMoreElements();) {
                    JarEntry je = e.nextElement();
                    String name = je.getName();
                    // Default-package names are notch leftovers whose outer classes do not exist.
                    if (!name.endsWith(".class") || !name.contains("/") || !seen.add(name)) continue;
                    byte[] bytes = ForgeSpec.readAll(jar.getInputStream(je));
                    ClassReader cr = new ClassReader(bytes);
                    boolean real = keep.matcher(name).find() || (cr.getAccess() & ACC_ENUM) != 0;
                    if (real && forced.stream().anyMatch(fm -> fm.startsWith(cr.getClassName() + "."))) { stub(cr, out, true); kept++; }
                    else if (real) { out.putNextEntry(new JarEntry(name)); out.write(bytes); out.closeEntry(); kept++; }
                    else try { stub(new ClassReader(bytes), out); n++; } catch (IllegalStateException x) { System.err.println("skipped: " + x.getMessage()); }
                }
            }
        }
        System.out.println("stubbed " + n + " classes, kept " + kept + " -> " + a[0]);
    }
}
