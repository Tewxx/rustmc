package ft.liveprobe.mixin;

import ft.liveprobe.Probe;
import net.minecraft.client.Minecraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(value = Minecraft.class, remap = false)
public abstract class TickMixin {
    @Inject(method = "func_71407_l()V", at = @At("HEAD"), cancellable = true, remap = false)
    private void beforeTick(CallbackInfo ci) {
        Probe.mark("tick:head");
        if (Probe.cancelTick) ci.cancel();
    }

    @Inject(method = "func_71407_l()V", at = @At("RETURN"), remap = false)
    private void afterTick(CallbackInfo ci) { Probe.mark("tick:return"); }
}
