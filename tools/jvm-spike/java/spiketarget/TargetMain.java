package spiketarget;
public class TargetMain {
    public static void main(String[] args) { int value = new ft.mixintest.Target().value(); System.setProperty("rustmc.spike.value", Integer.toString(value)); System.out.println("spike-value=" + value); }
}
