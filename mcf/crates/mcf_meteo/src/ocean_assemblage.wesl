// Assemblage après IFFT, porté de `ocean_assemble.hlsl` de SkyForge : correction de signe, déplacement final,
// dérivées et écume. Le spectre est centré (k = 0 au texel N/2) : la transformée inverse est multipliée par
// (-1)^(x+y). Le jacobien J = (1 + λ dDx/dx)(1 + λ dDz/dz) - (λ dDx/dz)² passe sous 1 quand la crête se replie :
// on y dépose l'écume, qui décroît ensuite exponentiellement.

import package::ocean_commun::{N, ParamsFft};

@group(0) @binding(0) var<uniform> p: ParamsFft;
@group(0) @binding(1) var entree_a: texture_2d_array<f32>;
@group(0) @binding(2) var entree_b: texture_2d_array<f32>;
@group(0) @binding(3) var ecume_precedente: texture_2d_array<f32>;
@group(0) @binding(4) var sortie_deplacement: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(5) var sortie_derivees: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(6) var sortie_ecume: texture_storage_2d_array<rgba16float, write>;

@compute @workgroup_size(8, 8, 1)
fn assembler(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= vec2<u32>(N))) {
        return;
    }
    let c = id.z;
    let signe = select(1.0, -1.0, ((id.x + id.y) & 1u) == 1u);
    let a = textureLoad(entree_a, id.xy, c, 0) * signe; // Dx, Dy, Dz, Sx
    let b = textureLoad(entree_b, id.xy, c, 0) * signe; // Sz, Jxx, Jzz, Jxz
    let l = p.choppiness;

    let deplacement = vec3<f32>(l * a.x, a.y, l * a.z);
    let jxx = 1.0 + l * b.y;
    let jzz = 1.0 + l * b.z;
    let jxz = l * b.w;
    let j = jxx * jzz - jxz * jxz;

    var ecume = textureLoad(ecume_precedente, id.xy, c, 0).x * exp(-p.decroissance_ecume * p.dt);
    ecume = saturate(max(ecume, saturate((p.seuil_ecume - j) * 6.0)));

    textureStore(sortie_deplacement, id.xy, c, vec4<f32>(deplacement, ecume));
    textureStore(sortie_derivees, id.xy, c, vec4<f32>(a.w, b.x, l * b.y, l * b.z));
    textureStore(sortie_ecume, id.xy, c, vec4<f32>(ecume, 0.0, 0.0, 0.0));
}
