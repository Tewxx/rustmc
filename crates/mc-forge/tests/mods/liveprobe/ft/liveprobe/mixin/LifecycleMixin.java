package ft.liveprobe.mixin;

import ft.liveprobe.Probe;
import net.minecraft.client.Minecraft;
import net.minecraft.client.gui.GuiScreen;
import net.minecraft.client.multiplayer.WorldClient;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

/**
 * World and screen transitions the Rust client owns must still enter Java's methods, or Mixin handlers such as these
 * never run. The markers prove the call went through {@code Minecraft.loadWorld}/{@code displayGuiScreen}.
 */
@Mixin(value = Minecraft.class, remap = false)
public abstract class LifecycleMixin {
    @Inject(method = "func_71403_a(Lnet/minecraft/client/multiplayer/WorldClient;)V", at = @At("HEAD"), remap = false)
    private void beforeLoadWorld(WorldClient world, CallbackInfo ci) {
        Probe.mark("load:" + (world == null ? "null" : "world"));
    }

    @Inject(method = "func_147108_a(Lnet/minecraft/client/gui/GuiScreen;)V", at = @At("HEAD"), remap = false)
    private void beforeDisplay(GuiScreen screen, CallbackInfo ci) {
        Probe.mark("screen:" + (screen == null ? "null" : screen.getClass().getSimpleName()));
    }
}
