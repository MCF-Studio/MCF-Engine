// Cuisson au démarrage des textures de bruit des nuages (approche Schneider / Horizon Zero Dawn), portée de
// `cloud_noise.hlsl` de SkyForge. Toutes périodiques, échantillonnées en mode répétition.
//  * Forme 128³ : R = Perlin-Worley, G/B/A = Worley fBm à fréquences croissantes.
//  * Détail 32³ : R/G/B = Worley fBm haute fréquence (érosion des bords).
//  * Carte météo 512² : R = couverture, G = type de nuage, B = humidité locale.

import package::nuages_commun::{perlin3, worley3, remapper};

@group(0) @binding(0) var sortie_forme: texture_storage_3d<rgba8unorm, write>;
@group(0) @binding(1) var sortie_detail: texture_storage_3d<rgba8unorm, write>;
@group(0) @binding(2) var sortie_carte: texture_storage_2d<rgba8unorm, write>;

fn worley_fbm(p: vec3<f32>, frequence: i32) -> f32 {
    let f = f32(frequence);
    return worley3(p * f, frequence) * 0.625
        + worley3(p * f * 2.0, frequence * 2) * 0.25
        + worley3(p * f * 4.0, frequence * 4) * 0.125;
}

fn perlin_fbm(p: vec3<f32>, frequence_initiale: i32, octaves: i32) -> f32 {
    var somme = 0.0;
    var amplitude = 0.5;
    var norme = 0.0;
    var frequence = frequence_initiale;
    for (var o = 0; o < octaves; o++) {
        somme += (perlin3(p * f32(frequence), frequence) * 0.5 + 0.5) * amplitude;
        norme += amplitude;
        amplitude *= 0.5;
        frequence *= 2;
    }
    return somme / norme;
}

@compute @workgroup_size(4, 4, 4)
fn cuire_forme(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = (vec3<f32>(id) + 0.5) / 128.0;
    let perlin = perlin_fbm(p, 4, 4);
    let w4 = worley_fbm(p, 4);
    // Perlin-Worley : le Perlin « gonflé » par les cellules de Worley.
    let pw = saturate(remapper(perlin, 0.0, 1.0, w4, 1.0));
    textureStore(sortie_forme, id, vec4(pw, w4, worley_fbm(p, 8), worley_fbm(p, 16)));
}

@compute @workgroup_size(4, 4, 4)
fn cuire_detail(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = (vec3<f32>(id) + 0.5) / 32.0;
    textureStore(sortie_detail, id, vec4(worley_fbm(p, 2), worley_fbm(p, 4), worley_fbm(p, 8), 1.0));
}

@compute @workgroup_size(8, 8, 1)
fn cuire_carte(@builtin(global_invocation_id) id: vec3<u32>) {
    let p = vec3((vec2<f32>(id.xy) + 0.5) / 512.0, 0.37);
    var couverture = perlin_fbm(p, 5, 5);
    couverture = saturate(remapper(couverture, 0.25, 0.75, 0.0, 1.0));
    couverture = saturate(couverture * 0.75 + worley3(p * 9.0, 9) * 0.35);
    let type_nuage = perlin_fbm(p + 0.5, 3, 3);
    let humidite = perlin_fbm(p + 0.25, 6, 3);
    textureStore(sortie_carte, id.xy, vec4(couverture, type_nuage, humidite, 1.0));
}
