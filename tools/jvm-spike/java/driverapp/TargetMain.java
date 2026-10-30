package driverapp;
public class TargetMain {
    public static void main(String[] args) {
        ft.mixintest.Target target = new ft.mixintest.Target();
        spike.Run.install(target);
        int value = target.value();
        System.setProperty("rustmc.spike.value", Integer.toString(value));
        System.out.println("spike-value=" + value);
    }
}
