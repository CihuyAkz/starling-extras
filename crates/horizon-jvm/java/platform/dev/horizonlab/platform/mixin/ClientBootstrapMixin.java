package dev.horizonlab.platform.mixin;

import dev.horizonlab.platform.HorizonGpu;
import net.minecraft.client.ClientBootstrap;
import net.minecraft.client.color.item.ItemTintSources;
import net.minecraft.client.gui.screens.dialog.DialogScreens;
import net.minecraft.client.gui.screens.dialog.body.DialogBodyHandlers;
import net.minecraft.client.gui.screens.dialog.input.InputControlHandlers;
import net.minecraft.client.renderer.item.ItemModels;
import net.minecraft.client.renderer.item.properties.conditional.ConditionalItemModelProperties;
import net.minecraft.client.renderer.item.properties.numeric.RangeSelectItemModelProperties;
import net.minecraft.client.renderer.item.properties.select.SelectItemModelProperties;
import net.minecraft.client.renderer.special.SpecialModelRenderers;
import net.minecraft.client.renderer.texture.atlas.SpriteSources;
import org.spongepowered.asm.mixin.Mixin;
import org.spongepowered.asm.mixin.Shadow;
import org.spongepowered.asm.mixin.injection.At;
import org.spongepowered.asm.mixin.injection.Inject;
import org.spongepowered.asm.mixin.injection.callback.CallbackInfo;

@Mixin(ClientBootstrap.class)
public abstract class ClientBootstrapMixin {
    @Shadow private static volatile boolean isBootstrapped;

    @Inject(method = "bootstrap", at = @At("HEAD"), cancellable = true)
    private static void horizon$bootstrapWithoutVulkan(CallbackInfo ci) {
        if (!HorizonGpu.skipVulkanClientBootstrap()) return;
        ci.cancel();
        if (isBootstrapped) return;
        isBootstrapped = true;
        ItemModels.bootstrap();
        SpecialModelRenderers.bootstrap();
        ItemTintSources.bootstrap();
        SelectItemModelProperties.bootstrap();
        ConditionalItemModelProperties.bootstrap();
        RangeSelectItemModelProperties.bootstrap();
        SpriteSources.bootstrap();
        DialogScreens.bootstrap();
        InputControlHandlers.bootstrap();
        DialogBodyHandlers.bootstrap();
    }
}
