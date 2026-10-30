package ft.shadow.mixin;
import ft.shadow.NativeBridge;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;
@Mixin(targets="net.minecraft.util.MathHelper", remap=false)
public abstract class MathMixin {
    @Inject(method="func_76128_c(D)I", at=@At("RETURN"), cancellable=true, remap=false)
    private static void afterFloor(double value, CallbackInfoReturnable<Integer> result) {
        result.setReturnValue(result.getReturnValueI() + NativeBridge.offset());
    }
}
