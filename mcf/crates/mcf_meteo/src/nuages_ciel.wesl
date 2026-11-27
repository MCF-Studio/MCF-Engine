// Ajoute les nuages à la carte d'environnement de l'atmosphère de Bevy (étape 16) : ciel = nuages + T × ciel.
// `ciel_atmosphere` est une copie de la carte telle que l'atmosphère vient de l'écrire ; `nuages` est le cube
// de nuages marché une face par image (`marcher_ciel` dans `nuages_marche.wesl`), en plus basse résolution.
// Les deux sont en luminance non exposée, comme la carte d'environnement.

@group(0) @binding(0) var ciel_atmosphere: texture_2d_array<f32>;
@group(0) @binding(1) var nuages: texture_2d_array<f32>;
@group(0) @binding(2) var echantillonneur: sampler;
@group(0) @binding(3) var sortie: texture_storage_2d_array<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn composer_ciel(@builtin(global_invocation_id) id: vec3<u32>) {
    let taille = textureDimensions(sortie);
    if any(id.xy >= taille) || id.z >= 6u {
        return;
    }
    let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(taille);
    let ciel = textureLoad(ciel_atmosphere, id.xy, id.z, 0);
    let n = textureSampleLevel(nuages, echantillonneur, uv, id.z, 0.0);
    textureStore(sortie, id.xy, id.z, vec4(n.rgb + n.a * ciel.rgb, ciel.a));
}
