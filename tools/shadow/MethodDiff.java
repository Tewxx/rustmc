import java.io.*;
import java.nio.file.*;
import java.util.*;
import org.objectweb.asm.*;
import org.objectweb.asm.tree.*;
import org.objectweb.asm.util.*;

/** Compare normalized method bodies in two class files, ignoring debug metadata. */
public class MethodDiff {
    static Map<String, String> methods(Path path) throws IOException {
        ClassNode node = new ClassNode();
        new ClassReader(Files.readAllBytes(path)).accept(node, ClassReader.SKIP_DEBUG | ClassReader.SKIP_FRAMES);
        Map<String, String> result = new TreeMap<>();
        for (Object entry : node.methods) {
            MethodNode method = (MethodNode) entry;
            Textifier text = new Textifier();
            method.accept(new TraceMethodVisitor(text));
            StringWriter out = new StringWriter();
            text.print(new PrintWriter(out));
            result.put(method.name + method.desc, method.access + "\n" + out.toString());
        }
        return result;
    }
    public static void main(String[] args) throws IOException {
        if (args.length != 2) throw new IllegalArgumentException("before.class after.class");
        Path beforePath = Paths.get(args[0]), afterPath = Paths.get(args[1]);
        String beforeClass = new ClassReader(Files.readAllBytes(beforePath)).getClassName();
        String afterClass = new ClassReader(Files.readAllBytes(afterPath)).getClassName();
        if (!beforeClass.equals(afterClass)) throw new IllegalArgumentException("different classes: " + beforeClass + " and " + afterClass);
        Map<String, String> before = methods(beforePath), after = methods(afterPath);
        Set<String> names = new TreeSet<>(before.keySet()); names.addAll(after.keySet());
        for (String name : names) if (!Objects.equals(before.get(name), after.get(name))) System.out.println(name);
    }
}
