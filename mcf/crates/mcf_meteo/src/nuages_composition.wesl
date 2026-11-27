// Composition des nuages sur l'image HDR (au début de la passe transparente). Mélange réglé dans `nuages.rs` :
// couleur = nuages + transmittance × image.

import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput;

@group(0) @binding(0) var nuages: texture_2d<f32>;
@group(0) @binding(1) var echantillonneur: sampler;

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    // 4 lectures bilinéaires décalées d'un demi-texel de la marche : filtre en tente sur 3 × 3 texels, qui
    // efface le motif du décalage par pixel (pas encore de reprojection temporelle).
    let d = 0.5 / vec2<f32>(textureDimensions(nuages));
    let c = 0.25 * (textureSampleLevel(nuages, echantillonneur, in.uv + vec2(-d.x, -d.y), 0.0)
        + textureSampleLevel(nuages, echantillonneur, in.uv + vec2(d.x, -d.y), 0.0)
        + textureSampleLevel(nuages, echantillonneur, in.uv + vec2(-d.x, d.y), 0.0)
        + textureSampleLevel(nuages, echantillonneur, in.uv + vec2(d.x, d.y), 0.0));
    return vec4(c.rgb, c.a);
}
