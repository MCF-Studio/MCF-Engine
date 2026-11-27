// Reconstruction temporelle des nuages (demi résolution de l'écran). La marche n'a calculé qu'un pixel par bloc
// 2 × 2 à cette image ; pour les trois autres, on reprend l'image précédente au même point du monde (rayon
// courant × distance moyenne des nuages du bloc, reprojeté avec la matrice de l'image précédente), bornée par
// les valeurs fraîches du voisinage 3 × 3 de blocs (rejet des historiques périmés : objets qui passent,
// nuages qui changent, zones découvertes). Le motif fait le tour du bloc en 4 images.

import bevy_render::view::View;

struct ParamsReprojection {
    precedente: mat4x4<f32>,
    taille_cible: vec2<u32>,
    decalage: vec2<u32>,
    pas: u32,
    valide: u32,
    r0: u32,
    r1: u32,
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> r: ParamsReprojection;
@group(0) @binding(2) var frais: texture_2d<f32>;
@group(0) @binding(3) var distance_frais: texture_2d<f32>;
@group(0) @binding(4) var historique: texture_2d<f32>;
@group(0) @binding(5) var echantillonneur: sampler;
@group(0) @binding(6) var sortie: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn reconstruire(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= r.taille_cible) {
        return;
    }
    let taille_frais = vec2<i32>(textureDimensions(frais));
    let bloc = min(vec2<i32>(id.xy / r.pas), taille_frais - 1);
    let f = textureLoad(frais, bloc, 0);
    let marche = min(vec2<u32>(bloc) * r.pas + r.decalage, r.taille_cible - 1u);
    if r.pas == 1u || r.valide == 0u || all(id.xy == marche) {
        textureStore(sortie, id.xy, f);
        return;
    }

    var mini = f;
    var maxi = f;
    for (var j = -1; j <= 1; j++) {
        for (var i = -1; i <= 1; i++) {
            let v = textureLoad(frais, clamp(bloc + vec2(i, j), vec2(0), taille_frais - 1), 0);
            mini = min(mini, v);
            maxi = max(maxi, v);
        }
    }

    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(r.taille_cible);
    let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let loin = view.world_from_clip * vec4(ndc, 1.0, 1.0);
    let dir = normalize(loin.xyz / loin.w - view.world_position);
    let distance = textureLoad(distance_frais, bloc, 0).r;
    let monde = view.world_position + dir * distance;
    let clip = r.precedente * vec4(monde, 1.0);
    if clip.w <= 1e-6 {
        textureStore(sortie, id.xy, f);
        return;
    }
    let ndc_p = clip.xy / clip.w;
    let uv_p = vec2(ndc_p.x * 0.5 + 0.5, 0.5 - ndc_p.y * 0.5);
    if any(uv_p < vec2(0.0)) || any(uv_p > vec2(1.0)) {
        textureStore(sortie, id.xy, f);
        return;
    }
    let h = textureSampleLevel(historique, echantillonneur, uv_p, 0.0);
    textureStore(sortie, id.xy, clamp(h, mini, maxi));
}
