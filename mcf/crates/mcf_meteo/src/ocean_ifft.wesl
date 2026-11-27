// IFFT 2D : radix-2, une ligne (ou colonne) entière par groupe de travail, en mémoire partagée. Porté de
// `ocean_fft.hlsl` de SkyForge. Chaque texel RGBA32F contient deux complexes : on transforme 4 complexes par
// thread (textures A et B), soit 8 champs réels. Entrée en ordre bit-inversé puis log2(N) étages de papillons.
// Mémoire partagée : 2 × 256 × 2 × 16 octets = 16 Kio (limite par défaut de wgpu).

import package::ocean_commun::{N, LOG_N, PI, ParamsFft, cmul};

@group(0) @binding(0) var<uniform> p: ParamsFft;
@group(0) @binding(1) var entree_a: texture_2d_array<f32>;
@group(0) @binding(2) var entree_b: texture_2d_array<f32>;
@group(0) @binding(3) var sortie_a: texture_storage_2d_array<rgba32float, write>;
@group(0) @binding(4) var sortie_b: texture_storage_2d_array<rgba32float, write>;

var<workgroup> partage_a: array<array<vec4<f32>, 256>, 2>;
var<workgroup> partage_b: array<array<vec4<f32>, 256>, 2>;

fn bits_inverses(x: u32) -> u32 {
    return reverseBits(x) >> (32u - LOG_N);
}

fn tourner(v: vec4<f32>, w: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(cmul(v.xy, w), cmul(v.zw, w));
}

fn transformer(ligne: u32, i: u32, cascade: u32, vertical: bool) {
    let r = bits_inverses(i);
    let source = select(vec2<u32>(r, ligne), vec2<u32>(ligne, r), vertical);
    partage_a[0][i] = textureLoad(entree_a, source, cascade, 0);
    partage_b[0][i] = textureLoad(entree_b, source, cascade, 0);
    workgroupBarrier();

    var ping = 0u;
    for (var m = 2u; m <= N; m = m << 1u) {
        let h = m >> 1u;
        let k = i & (m - 1u);
        let j = i - k;
        let kk = k & (h - 1u);
        let angle = 2.0 * PI * f32(kk) / f32(m);
        let w = vec2<f32>(cos(angle), sin(angle));
        let s = select(-1.0, 1.0, k < h);
        let ea = partage_a[ping][j + kk];
        let oa = partage_a[ping][j + kk + h];
        let eb = partage_b[ping][j + kk];
        let ob = partage_b[ping][j + kk + h];
        partage_a[1u - ping][i] = ea + s * tourner(oa, w);
        partage_b[1u - ping][i] = eb + s * tourner(ob, w);
        ping = 1u - ping;
        workgroupBarrier();
    }

    let destination = select(vec2<u32>(i, ligne), vec2<u32>(ligne, i), vertical);
    textureStore(sortie_a, destination, cascade, partage_a[ping][i]);
    textureStore(sortie_b, destination, cascade, partage_b[ping][i]);
}

@compute @workgroup_size(256, 1, 1)
fn lignes(@builtin(workgroup_id) groupe: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>) {
    transformer(groupe.x, local.x, groupe.z, false);
}

@compute @workgroup_size(256, 1, 1)
fn colonnes(@builtin(workgroup_id) groupe: vec3<u32>, @builtin(local_invocation_id) local: vec3<u32>) {
    transformer(groupe.x, local.x, groupe.z, true);
}
