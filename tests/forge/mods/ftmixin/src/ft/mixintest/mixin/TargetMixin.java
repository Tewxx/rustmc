package ft.mixintest.mixin;
import ft.mixintest.Target;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;
@Mixin(value=Target.class, remap=false)
public abstract class TargetMixin {
    @Inject(method="value", at=@At("RETURN"), cancellable=true, remap=false)
    private void alter(CallbackInfoReturnable<Integer> callback) { callback.setReturnValue(2); }
}
