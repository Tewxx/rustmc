package ft.liveprobe.mixin;

import ft.liveprobe.Probe;
import net.minecraft.client.entity.EntityPlayerSP;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(value = EntityPlayerSP.class, remap = false)
public abstract class WalkingMixin {
    @Inject(method = "func_175161_p()V", at = @At("HEAD"), cancellable = true, remap = false)
    private void beforeWalking(CallbackInfo ci) {
        Probe.mark("walk:head");
        if (Probe.cancelWalking) ci.cancel();
    }

    @Inject(method = "func_175161_p()V", at = @At("RETURN"), remap = false)
    private void afterWalking(CallbackInfo ci) { Probe.mark("walk:return"); }
}
