import java.nio.file.*;
import ft.core.Transformer;
public class FTCoreCheck extends ClassLoader {
    public static void main(String[] args) throws Exception {
        byte[] input = Files.readAllBytes(Paths.get(args[0]));
        byte[] result = new Transformer().transform("ft.core.Target", "ft.core.Target", input);
        Class<?> target = new FTCoreCheck().defineClass("ft.core.Target", result, 0, result.length);
        if (!target.getMethod("value").invoke(null).equals(2)) throw new AssertionError();
        System.out.println("transformer changed target return value");
    }
}
