package dev.horizonlab.platform.mixin;

import dev.horizonlab.platform.HorizonPackets;
import dev.horizonlab.platform.HorizonServer;
import net.minecraft.core.Holder;
import net.minecraft.server.level.ServerPlayer;
import net.minecraft.world.effect.MobEffect;
import net.minecraft.world.effect.MobEffectInstance;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.LivingEntity;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfoReturnable;

@Mixin(LivingEntity.class)
public abstract class LivingEntityEffectMixin {
    @Inject(method = "addEffect(Lnet/minecraft/world/effect/MobEffectInstance;Lnet/minecraft/world/entity/Entity;)Z", at = @At("RETURN"))
    private void horizon$effectAdded(MobEffectInstance effect, Entity source, CallbackInfoReturnable<Boolean> cir) {
        if (!cir.getReturnValueZ()) return;
        if ((Object) this instanceof ServerPlayer player && HorizonServer.isMirror(player)) {
            HorizonPackets.effect(player, effect);
        }
    }

    @Inject(method = "removeEffect(Lnet/minecraft/core/Holder;)Z", at = @At("RETURN"))
    private void horizon$effectRemoved(Holder<MobEffect> effect, CallbackInfoReturnable<Boolean> cir) {
        if (!cir.getReturnValueZ()) return;
        if ((Object) this instanceof ServerPlayer player && HorizonServer.isMirror(player)) {
            HorizonPackets.removeEffect(player, effect);
        }
    }
}
