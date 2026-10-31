package ft.stubmixin;

import net.minecraft.util.MathHelper;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Mixes into a stubbed vanilla class: the Rust handler runs, then this RETURN injection adds 100 to its result. */
@Mixin(value = MathHelper.class, remap = false)
public abstract class MathMixin {
    @Inject(method = "func_76128_c", at = @At("RETURN"), cancellable = true, remap = false)
    private static void floorPlus100(double value, CallbackInfoReturnable<Integer> cir) {
        cir.setReturnValue(cir.getReturnValue() + 100);
    }

    /** HEAD injection: replaces the result before any body (stub or Rust) runs. */
    @Inject(method = "func_76135_e", at = @At("HEAD"), cancellable = true, remap = false)
    private static void absOverride(float value, CallbackInfoReturnable<Float> cir) {
        cir.setReturnValue(-1.0F);
    }
}
