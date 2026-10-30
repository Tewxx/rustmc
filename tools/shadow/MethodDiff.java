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
        Map<String, String> before = methods(Paths.get(args[0])), after = methods(Paths.get(args[1]));
        Set<String> names = new TreeSet<>(before.keySet()); names.addAll(after.keySet());
        for (String name : names) if (!Objects.equals(before.get(name), after.get(name))) System.out.println(name);
    }
}
