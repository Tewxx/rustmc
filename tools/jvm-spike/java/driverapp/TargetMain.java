package driverapp;
public class TargetMain {
    public static void main(String[] args) { System.out.println("loader=" + ft.mixintest.Target.class.getClassLoader()); System.out.println("transformers=" + net.minecraft.launchwrapper.Launch.classLoader.getTransformers()); int value = new ft.mixintest.Target().value(); System.setProperty("rustmc.spike.value", Integer.toString(value)); System.out.println("spike-value=" + value); }
}
