// Surface de l'océan : maillage radial centré sur la caméra, déplacé par 4 vagues de Gerstner (la houle, au pic du
// spectre) et par 3 cascades FFT (la mer du vent), éclairé par le PBR de Bevy (reflets du ciel / de l'atmosphère,
// soleil, brouillard). Porté de `ocean_draw.hlsl` de SkyForge, sans les vagues de bord (étape 6).
//
// LOD : la plus petite cascade ne déplace plus les sommets au-delà de `lod_proche` (elle reste dans la normale),
// les cascades s'effacent entre 0,7 et 1 × `lod_loin`, et les détails de la normale s'estompent avec la distance
// (anti-crénelage).

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    mesh_view_bindings::view,
};
import bevy_pbr::render::view_transformations::position_world_to_clip;

@if(PREPASS_PIPELINE)
import bevy_pbr::{
    prepass::io::{VertexOutput, FragmentOutput},
    deferred::functions::deferred_output,
};
@else
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct ExtensionOcean {
    vague_a: array<vec4<f32>, 4>,   // direction x, direction z, k (rad/m), ω (rad/s)
    vague_b: array<vec4<f32>, 4>,   // amplitude (m), raideur, phase, -
    niveau: f32,
    temps: f32,
    rayon: f32,          // rayon du maillage (m)
    part_lineaire: f32,  // part linéaire de la répartition des sommets
    quads: f32,          // quads par côté
    ecume: f32,          // intensité de l'écume
    vent: f32,           // vitesse du vent (m/s)
    _r: f32,
    tailles: vec4<f32>,  // taille du motif de chaque cascade FFT (m), w = 1 si la FFT est active
    lod_proche: f32,
    lod_loin: f32,
    _r1: f32,
    _r2: f32,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> ocean: ExtensionOcean;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var deplacement_fft: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var echantillonneur: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var derivees_fft: texture_2d_array<f32>;

// Poids de la FFT selon la distance : x = plus petite cascade dans le déplacement, y = toutes les cascades.
fn poids_fft(distance: f32) -> vec2<f32> {
    let actif = ocean.tailles.w;
    return vec2<f32>(
        1.0 - smoothstep(ocean.lod_proche, ocean.lod_proche * 3.0, distance),
        1.0 - smoothstep(ocean.lod_loin * 0.7, ocean.lod_loin, distance),
    ) * actif;
}

// Déplacement FFT (xyz) et écume (w) en un point au repos.
fn deplacement_cascades(xz: vec2<f32>, distance: f32) -> vec4<f32> {
    let w = poids_fft(distance);
    if (w.y <= 0.0) {
        return vec4<f32>(0.0);
    }
    let d0 = textureSampleLevel(deplacement_fft, echantillonneur, xz / ocean.tailles.x, 0, 0.0);
    let d1 = textureSampleLevel(deplacement_fft, echantillonneur, xz / ocean.tailles.y, 1, 0.0);
    let d2 = textureSampleLevel(deplacement_fft, echantillonneur, xz / ocean.tailles.z, 2, 0.0);
    let d = d0.xyz + d1.xyz + d2.xyz * w.x;
    return vec4<f32>(d * w.y, saturate(d0.w + d1.w + d2.w) * w.y);
}

struct Gerstner {
    deplacement: vec3<f32>,
    normale: vec3<f32>,
    crete: f32,          // compression horizontale (proche de 1 sur les crêtes qui se replient)
};

fn gerstner(xz: vec2<f32>) -> Gerstner {
    var g: Gerstner;
    g.deplacement = vec3<f32>(0.0);
    var n = vec3<f32>(0.0, 1.0, 0.0);
    var compression = 0.0;
    for (var i = 0; i < 4; i++) {
        let a = ocean.vague_a[i];
        let b = ocean.vague_b[i];
        let d = a.xy;
        let k = a.z;
        let amp = b.x;
        let q = b.y;
        let th = k * dot(d, xz) - a.w * ocean.temps + b.z;
        let s = sin(th);
        let c = cos(th);
        g.deplacement += vec3<f32>(q * amp * d.x * c, amp * s, q * amp * d.y * c);
        n -= vec3<f32>(d.x * k * amp * c, q * k * amp * s, d.y * k * amp * c);
        compression += q * k * amp * s;
    }
    g.normale = normalize(n);
    g.crete = compression;
    return g;
}

struct Sommet {
    @builtin(instance_index) instance_index: u32,
    @location(0) position: vec3<f32>,
};

@vertex
fn vertex(s: Sommet) -> VertexOutput {
    var out: VertexOutput;
    let u = s.position.xz;
    // Répartition radiale : dense au centre, lâche au bord.
    let local = ocean.rayon * (ocean.part_lineaire * u + (1.0 - ocean.part_lineaire) * u * u * u);
    // Alignement sur la cellule centrale pour limiter le « nagement » quand la caméra bouge.
    let cellule = ocean.rayon * ocean.part_lineaire * 2.0 / ocean.quads;
    let camera = view.world_position;
    let xz = floor(camera.xz / cellule) * cellule + local;

    let g = gerstner(xz);
    let fft = deplacement_cascades(xz, length(xz - camera.xz)).xyz;
    let monde = vec3<f32>(xz.x, ocean.niveau, xz.y) + g.deplacement + fft;
    out.world_position = vec4<f32>(monde, 1.0);
    out.position = position_world_to_clip(monde);
    out.world_normal = g.normale;
@if(VERTEX_UVS_A)
    out.uv = xz;
@if(VERTEX_UVS_B)
    out.uv_b = xz;
@if(VERTEX_TANGENTS)
    out.world_tangent = vec4<f32>(1.0, 0.0, 0.0, 1.0);
@if(VERTEX_COLORS)
    out.color = vec4<f32>(1.0);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = s.instance_index;
@if(VISIBILITY_RANGE_DITHER)
    out.visibility_range_dither = 0;
    return out;
}

fn hachage(x_in: u32) -> u32 {
    var x = x_in;
    x ^= x >> 16u; x *= 0x7feb352du;
    x ^= x >> 15u; x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}
fn valeur(c: vec2<i32>) -> f32 {
    return f32(hachage(u32(c.x) ^ hachage(u32(c.y) ^ 0x5bd1e995u)) & 0xFFFFu) / 65535.0;
}
fn bruit_valeur(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(valeur(i), valeur(i + vec2<i32>(1, 0)), u.x),
        mix(valeur(i + vec2<i32>(0, 1)), valeur(i + vec2<i32>(1, 1)), u.x),
        u.y,
    );
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    // Point au repos approché (on retire le déplacement horizontal de la houle), puis normale par pixel.
    let monde = in.world_position.xyz;
    let distance = length(view.world_position - monde);
    var repos = monde.xz;
    for (var i = 0; i < 2; i++) {
        let d = gerstner(repos).deplacement;
        repos = monde.xz - d.xz;
    }
    let g = gerstner(repos);
    // Pente de la houle.
    var pente = vec2<f32>(-g.normale.x, -g.normale.z) / max(g.normale.y, 0.05);
    // Pentes des cascades FFT, les petites atténuées au loin (anti-crénelage).
    let w = poids_fft(distance);
    var ecume_fft = 0.0;
    if (ocean.tailles.w > 0.0) {
        let n0 = textureSample(derivees_fft, echantillonneur, repos / ocean.tailles.x, 0);
        let n1 = textureSample(derivees_fft, echantillonneur, repos / ocean.tailles.y, 1);
        let n2 = textureSample(derivees_fft, echantillonneur, repos / ocean.tailles.z, 2);
        let f1 = 1.0 - smoothstep(150.0, 1200.0, distance);
        let f2 = 1.0 - smoothstep(25.0, 150.0, distance);
        let s = n0.xy + n1.xy * f1 + n2.xy * f2;
        let jx = n0.z + n1.z * f1 + n2.z * f2;
        let jz = n0.w + n1.w * f1 + n2.w * f2;
        pente += vec2<f32>(s.x / max(1.0 + jx, 0.2), s.y / max(1.0 + jz, 0.2)) * max(w.y, 0.35);
        ecume_fft = deplacement_cascades(repos, distance).w;
    }
    var n = normalize(vec3<f32>(-pente.x, 1.0, -pente.y));
    // Lissage lointain : la normale moyenne d'un pixel lointain tend vers le haut.
    n = normalize(mix(n, vec3<f32>(0.0, 1.0, 0.0), saturate(distance / 12000.0)));
    pbr_input.N = n;
    pbr_input.world_normal = n;

    // Rugosité : un peu plus forte au loin et par vent fort.
    pbr_input.material.perceptual_roughness = mix(0.04, 0.16, saturate(distance / 3000.0)) + ocean.vent * 0.003;

    // Écume : crêtes qui se replient (jacobien FFT, et compression de la houle), en motif de dentelle.
    let motif = bruit_valeur(monde.xz * 0.8 + ocean.temps * 0.05) * 0.6 + bruit_valeur(monde.xz * 3.1) * 0.4;
    let ecume_houle = saturate((g.crete - 0.45) * 2.5) * smoothstep(4.0, 12.0, ocean.vent);
    let ecume = saturate(max(ecume_fft * mix(0.5, 1.2, motif), ecume_houle * smoothstep(0.35, 0.75, motif)) * ocean.ecume);
    if (ecume > 0.0) {
        pbr_input.material.base_color = vec4<f32>(
            mix(pbr_input.material.base_color.rgb, vec3<f32>(0.85, 0.88, 0.9), ecume),
            1.0,
        );
        pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.8, ecume);
    }

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}
    return out;
}
