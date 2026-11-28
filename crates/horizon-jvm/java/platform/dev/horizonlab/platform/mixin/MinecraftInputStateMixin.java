package dev.horizonlab.platform.mixin;

import dev.horizonlab.platform.HorizonClient;
import net.minecraft.client.Minecraft;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

/** Minecraft's static key-state helpers poll LWJGL, which is not present on Android. */
@Mixin(Minecraft.class)
public abstract class MinecraftInputStateMixin {
    @Inject(method = "hasShiftDown", at = @At("HEAD"), cancellable = true, require = 0)
    private static void horizon$shiftDown(CallbackInfoReturnable<Boolean> cir) {
        Integer modifiers = HorizonClient.screenInputModifiers();
        if (HorizonClient.headless() && modifiers != null) cir.setReturnValue((modifiers & 3) != 0);
    }

    @Inject(method = "hasControlDown", at = @At("HEAD"), cancellable = true, require = 0)
    private static void horizon$controlDown(CallbackInfoReturnable<Boolean> cir) {
        Integer modifiers = HorizonClient.screenInputModifiers();
        if (HorizonClient.headless() && modifiers != null) cir.setReturnValue((modifiers & 192) != 0);
    }

    @Inject(method = "hasAltDown", at = @At("HEAD"), cancellable = true, require = 0)
    private static void horizon$altDown(CallbackInfoReturnable<Boolean> cir) {
        Integer modifiers = HorizonClient.screenInputModifiers();
        if (HorizonClient.headless() && modifiers != null) cir.setReturnValue((modifiers & 768) != 0);
    }
}
