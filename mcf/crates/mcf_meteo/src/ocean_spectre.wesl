// Spectre d'océan JONSWAP (Hasselmann 1973) et évolution temporelle (Tessendorf 2001), porté de
// `ocean_spectrum.hlsl` de SkyForge. Une couche de texture par cascade (global_invocation_id.z).
//
//  * Dispersion en eau profonde : ω(k) = sqrt(g |k|).
//  * JONSWAP : S(ω) = α g² / ω⁵ · exp(-5/4 (ωp/ω)⁴) · γ^r.
//  * Étalement directionnel : D(θ) = N(s) |cos(θ/2)|^(2s).
//  * h0(k) = ξ / √2 · sqrt(2 S(k) dk²), ξ gaussienne complexe.
//  * h(k, t) = h0(k) e^{iωt} + conj(h0(-k)) e^{-iωt} (symétrie hermitienne : transformée inverse réelle).

import package::ocean_commun::{N, PI, G, ParamsFft, cmul, conj, empaqueter, hachage3, hachage_f};

@group(0) @binding(0) var<uniform> p: ParamsFft;
@group(0) @binding(1) var sortie_h0: texture_storage_2d_array<rgba32float, write>;
@group(0) @binding(2) var entree_h0: texture_2d_array<f32>;
@group(0) @binding(3) var sortie_a: texture_storage_2d_array<rgba32float, write>;
@group(0) @binding(4) var sortie_b: texture_storage_2d_array<rgba32float, write>;

fn jonswap(w: f32) -> f32 {
    let u = max(p.u10, 0.5);
    let f = max(p.fetch, 1000.0);
    let alpha = 0.076 * pow(u * u / (f * G), 0.22);
    let wp = 22.0 * pow(G * G / (u * f), 1.0 / 3.0);
    let gamma = 3.3;
    let sigma = select(0.09, 0.07, w <= wp);
    let r = exp(-(w - wp) * (w - wp) / (2.0 * sigma * sigma * wp * wp));
    return alpha * G * G / pow(w, 5.0) * exp(-1.25 * pow(wp / w, 4.0)) * pow(gamma, r);
}

fn etalement(theta: f32) -> f32 {
    // Plus le vent est fort, plus les vagues sont alignées.
    let s = mix(4.0, 14.0, saturate(p.u10 / 25.0));
    let norme = sqrt(s + 0.25) / (2.0 * sqrt(PI));
    return norme * pow(abs(cos(theta * 0.5)), 2.0 * s);
}

fn amplitude(k: vec2<f32>, cascade: u32) -> f32 {
    let kl = length(k);
    if (kl < max(p.k_min[cascade], 1e-4) || kl >= p.k_max[cascade]) {
        return 0.0;
    }
    let w = sqrt(G * kl);
    let dwdk = G / (2.0 * w);
    let theta = atan2(k.y, k.x) - atan2(p.vent.y, p.vent.x);
    var s = jonswap(w) * etalement(theta) * dwdk / kl;
    s *= exp(-kl * kl * 1e-4); // amortit les ondes capillaires (< ~6 cm)
    let dk = 2.0 * PI / p.tailles[cascade];
    return sqrt(2.0 * s * dk * dk);
}

fn gaussienne(id: vec2<u32>, cascade: u32) -> vec2<f32> {
    let graine = hachage3(vec3<u32>(id, cascade * 7919u + 17u));
    let u1 = max(hachage_f(graine), 1e-6);
    let u2 = hachage_f(graine ^ 0xA511E9B3u);
    let r = sqrt(-2.0 * log(u1));
    return vec2<f32>(r * cos(2.0 * PI * u2), r * sin(2.0 * PI * u2));
}

fn vecteur_onde(id: vec2<u32>, cascade: u32) -> vec2<f32> {
    return 2.0 * PI * (vec2<f32>(id) - f32(N / 2u)) / p.tailles[cascade];
}

// Passe 1 (quand le vent change) : spectre initial.
@compute @workgroup_size(8, 8, 1)
fn initialiser(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= vec2<u32>(N))) {
        return;
    }
    let c = id.z;
    let id_neg = (vec2<u32>(N) - id.xy) % vec2<u32>(N);
    let h0 = gaussienne(id.xy, c) * amplitude(vecteur_onde(id.xy, c), c) * 0.70710678;
    let h0n = gaussienne(id_neg, c) * amplitude(-vecteur_onde(id.xy, c), c) * 0.70710678;
    textureStore(sortie_h0, id.xy, c, vec4<f32>(h0, conj(h0n)));
}

// Passe 2 (chaque image) : spectre à l'instant t pour les 8 champs utiles.
@compute @workgroup_size(8, 8, 1)
fn evoluer(@builtin(global_invocation_id) id: vec3<u32>) {
    if (any(id.xy >= vec2<u32>(N))) {
        return;
    }
    let c = id.z;
    let h0 = textureLoad(entree_h0, id.xy, c, 0);
    let k = vecteur_onde(id.xy, c);
    let kl = max(length(k), 1e-4);

    // Fréquence quantifiée : le motif boucle exactement toutes les 200 s.
    let w0 = 2.0 * PI / 200.0;
    let w = floor(sqrt(G * kl) / w0) * w0;
    let ph = w * p.temps;
    let e = vec2<f32>(cos(ph), sin(ph));

    let h = cmul(h0.xy, e) + cmul(h0.zw, conj(e));
    let ih = vec2<f32>(-h.y, h.x);

    let dx = ih * (k.x / kl);
    let dz = ih * (k.y / kl);
    let sx = ih * k.x;                  // pente dDy/dx
    let sz = ih * k.y;                  // pente dDy/dz
    let jxx = -h * (k.x * k.x / kl);    // dDx/dx
    let jzz = -h * (k.y * k.y / kl);    // dDz/dz
    let jxz = -h * (k.x * k.y / kl);    // dDx/dz

    textureStore(sortie_a, id.xy, c, vec4<f32>(empaqueter(dx, h), empaqueter(dz, sx)));
    textureStore(sortie_b, id.xy, c, vec4<f32>(empaqueter(sz, jxx), empaqueter(jzz, jxz)));
}
