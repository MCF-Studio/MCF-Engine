// Carte d'ombre des nuages, vue du soleil (étape 15). Chaque texel est un rayon parallèle au soleil : on traverse
// la couche [bas, haut] et on écrit la transmittance exp(-∫ σt ds). Bevy l'applique ensuite à la lumière du
// soleil sur tous les matériaux (`DirectionalLightTexture`) : même projection que `directional_light` dans
// `pbr_lighting.wesl` (local = monde → lumière, uv = local.xy × (-0,5 ; 0,5) + 0,5).
//
// La densité est la version grossière de `densite()` de `nuages_marche.wesl` (sans l'érosion par le détail,
// invisible à 62 m par texel) : les deux doivent rester identiques.

import package::nuages_commun::{ParamsNuages, remapper, gradient_hauteur};

// Même ordre que `ParamsOmbre` dans `ombre_nuages.rs`.
struct ParamsOmbre {
    centre: vec3<f32>,
    demi_taille: f32,
    droite: vec3<f32>,
    pas: u32,
    haut_lumiere: vec3<f32>,
    r0: f32,
    vers_soleil: vec3<f32>,
    r1: f32,
}

@group(0) @binding(0) var<uniform> p: ParamsNuages;
@group(0) @binding(1) var<uniform> o: ParamsOmbre;
@group(0) @binding(2) var forme: texture_3d<f32>;
@group(0) @binding(3) var carte: texture_2d<f32>;
@group(0) @binding(4) var echantillonneur: sampler;
@group(0) @binding(5) var sortie: texture_storage_2d<rgba8unorm, write>;

fn densite_grossiere(pos: vec3<f32>) -> f32 {
    let h = (pos.y - p.bas) / (p.haut - p.bas);
    if h <= 0.0 || h >= 1.0 {
        return 0.0;
    }
    let mc = textureSampleLevel(carte, echantillonneur, (pos.xz + p.decalage) / 30000.0, 0.0);
    let couverture = saturate(p.couverture + (mc.r - 0.5) * 0.9 + p.precipitations * 0.15);
    let type_nuage = saturate(p.type_nuages + (mc.g - 0.5) * 0.4);
    let decalage3 = vec3(p.decalage.x, 0.0, p.decalage.y);
    let s = textureSampleLevel(forme, echantillonneur, (pos + decalage3) * p.echelle_forme, 0.0);
    let fbm = s.g * 0.625 + s.b * 0.25 + s.a * 0.125;
    var base = remapper(s.r, -(1.0 - fbm), 1.0, 0.0, 1.0);
    base = saturate(remapper(base, 0.77, 0.89, 0.0, 1.0));
    base *= gradient_hauteur(h, type_nuage);
    return saturate(remapper(base, 1.0 - couverture, 1.0, 0.0, 1.0)) * couverture;
}

@compute @workgroup_size(8, 8, 1)
fn ombrer(@builtin(global_invocation_id) id: vec3<u32>) {
    let taille = textureDimensions(sortie);
    if any(id.xy >= taille) {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(taille);
    let local = vec2(1.0 - 2.0 * uv.x, 2.0 * uv.y - 1.0) * o.demi_taille;
    let origine = o.centre + o.droite * local.x + o.haut_lumiere * local.y;
    let d = o.vers_soleil;
    var transmittance = 1.0;
    if d.y > 1e-3 {
        // Traversée de la couche le long du rayon (les deux sens : le rayon est une droite).
        let t0 = (p.bas - origine.y) / d.y;
        let t1 = (p.haut - origine.y) / d.y;
        let dt = (t1 - t0) / f32(o.pas);
        var od = 0.0;
        for (var i = 0u; i < o.pas; i++) {
            od += densite_grossiere(origine + d * (t0 + (f32(i) + 0.5) * dt));
        }
        transmittance = exp(-od * dt * p.extinction * p.densite);
    }
    textureStore(sortie, id.xy, vec4(transmittance, transmittance, transmittance, 1.0));
}
