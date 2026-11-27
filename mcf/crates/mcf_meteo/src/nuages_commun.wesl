// Code commun des nuages volumétriques : paramètres, hachage et bruits périodiques (Perlin, Worley), portés de
// `Common.hlsli` de SkyForge (mêmes valeurs que `bruit.rs` côté CPU).

const PI: f32 = 3.14159265359;

// Même ordre que `ParamsNuages` dans `nuages.rs`.
struct ParamsNuages {
    soleil_direction: vec3<f32>,
    soleil_lux: f32,
    soleil_couleur: vec3<f32>,
    ambiance: f32,
    decalage: vec2<f32>,
    temps: f32,
    couverture: f32,
    densite: f32,
    type_nuages: f32,
    precipitations: f32,
    bas: f32,
    haut: f32,
    echelle_forme: f32,
    echelle_detail: f32,
    force_detail: f32,
    extinction: f32,
    g_avant: f32,
    g_arriere: f32,
    poudre: f32,
    pas_proche: f32,
    pas: u32,
    pas_lumiere: u32,
    distance_max: f32,
    distance_fondu: f32,
    r0: f32,
}

fn hachage(x_in: u32) -> u32 {
    var x = x_in;
    x = x ^ (x >> 16u);
    x = x * 0x7feb352du;
    x = x ^ (x >> 15u);
    x = x * 0x846ca68bu;
    x = x ^ (x >> 16u);
    return x;
}

fn hachage3(v: vec3<u32>) -> u32 {
    return hachage(v.x ^ hachage(v.y ^ hachage(v.z)));
}

fn hachage_f(x: u32) -> f32 {
    return f32(hachage(x) & 0x00ffffffu) / 16777216.0;
}

fn hachage33(p: vec3<i32>) -> vec3<f32> {
    let h = hachage3(bitcast<vec3<u32>>(p));
    return vec3(hachage_f(h), hachage_f(h ^ 0x9e3779b9u), hachage_f(h ^ 0x85ebca6bu));
}

fn replier(c: vec3<i32>, periode: i32) -> vec3<i32> {
    if periode <= 0 {
        return c;
    }
    let p = vec3(periode);
    return ((c % p) + p) % p;
}

fn produit_gradient(cellule: vec3<i32>, periode: i32, f: vec3<f32>) -> f32 {
    let g = normalize(hachage33(replier(cellule, periode)) * 2.0 - 1.0 + 1e-4);
    return dot(g, f);
}

// Bruit de Perlin 3D (~[-1, 1]), périodique si `periode > 0`.
fn perlin3(p: vec3<f32>, periode: i32) -> f32 {
    let base = floor(p);
    let i = vec3<i32>(base);
    let f = p - base;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let n000 = produit_gradient(i + vec3(0, 0, 0), periode, f - vec3(0.0, 0.0, 0.0));
    let n100 = produit_gradient(i + vec3(1, 0, 0), periode, f - vec3(1.0, 0.0, 0.0));
    let n010 = produit_gradient(i + vec3(0, 1, 0), periode, f - vec3(0.0, 1.0, 0.0));
    let n110 = produit_gradient(i + vec3(1, 1, 0), periode, f - vec3(1.0, 1.0, 0.0));
    let n001 = produit_gradient(i + vec3(0, 0, 1), periode, f - vec3(0.0, 0.0, 1.0));
    let n101 = produit_gradient(i + vec3(1, 0, 1), periode, f - vec3(1.0, 0.0, 1.0));
    let n011 = produit_gradient(i + vec3(0, 1, 1), periode, f - vec3(0.0, 1.0, 1.0));
    let n111 = produit_gradient(i + vec3(1, 1, 1), periode, f - vec3(1.0, 1.0, 1.0));
    let x00 = mix(n000, n100, u.x);
    let x10 = mix(n010, n110, u.x);
    let x01 = mix(n001, n101, u.x);
    let x11 = mix(n011, n111, u.x);
    return mix(mix(x00, x10, u.y), mix(x01, x11, u.y), u.z);
}

// Worley périodique : 1 - distance au point le plus proche (cellules « gonflées »).
fn worley3(p: vec3<f32>, periode: i32) -> f32 {
    let base = floor(p);
    let i = vec3<i32>(base);
    let f = p - base;
    var d = 1e9;
    for (var z = -1; z <= 1; z++) {
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let c = vec3(x, y, z);
                let o = vec3<f32>(c) + hachage33(replier(i + c, periode)) - f;
                d = min(d, dot(o, o));
            }
        }
    }
    return 1.0 - saturate(sqrt(d));
}

fn remapper(v: f32, l0: f32, h0: f32, l1: f32, h1: f32) -> f32 {
    return l1 + (v - l0) * (h1 - l1) / max(h0 - l0, 1e-5);
}
