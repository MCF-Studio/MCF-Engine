// Constantes et fonctions communes aux passes FFT de l'océan (portées de `OceanCommon.hlsli` de SkyForge).

const N: u32 = 256u;
const LOG_N: u32 = 8u;
const PI: f32 = 3.14159265359;
const G: f32 = 9.81;

struct ParamsFft {
    tailles: vec4<f32>,      // taille du motif de chaque cascade (m)
    k_min: vec4<f32>,        // bande de nombres d'onde gérée par chaque cascade
    k_max: vec4<f32>,
    vent: vec2<f32>,         // direction du vent (plan XZ)
    u10: f32,                // vitesse du vent à 10 m (m/s)
    fetch: f32,              // fetch (m)
    choppiness: f32,         // déplacement horizontal (vagues pointues)
    decroissance_ecume: f32, // 1/s
    seuil_ecume: f32,        // jacobien sous lequel l'écume apparaît
    temps: f32,              // temps bouclé sur 200 s
    dt: f32,
    _r0: f32,
    _r1: f32,
    _r2: f32,
};

fn cmul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}
fn conj(a: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x, -a.y);
}
// Empaquetage de deux champs à transformée réelle : a + i b.
fn empaqueter(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x - b.y, a.y + b.x);
}

fn hachage(x_in: u32) -> u32 {
    var x = x_in;
    x ^= x >> 16u; x *= 0x7feb352du;
    x ^= x >> 15u; x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}
fn hachage3(v: vec3<u32>) -> u32 {
    return hachage(v.x ^ hachage(v.y ^ hachage(v.z)));
}
fn hachage_f(x: u32) -> f32 {
    return f32(hachage(x) & 0x00FFFFFFu) / 16777216.0;
}
