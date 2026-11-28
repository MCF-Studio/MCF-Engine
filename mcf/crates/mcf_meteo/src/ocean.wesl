// Surface de l'océan : maillage radial centré sur la caméra, déplacé par 4 vagues de Gerstner (la houle, au pic du
// spectre) et par 3 cascades FFT (la mer du vent), éclairé par le PBR de Bevy (reflets du ciel / de l'atmosphère,
// soleil, brouillard). Porté de `ocean_draw.hlsl` de SkyForge, avec les vagues de bord (`ShoreWaves.hlsli`) quand
// le jeu fournit le fond (voir `rivage.rs`, dont les fonctions sont la copie exacte côté CPU).
//
// LOD : la plus petite cascade ne déplace plus les sommets au-delà de `lod_proche` (elle reste dans la normale),
// les cascades s'effacent entre 0,7 et 1 × `lod_loin`, et les détails de la normale s'estompent avec la distance
// (anti-crénelage).

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    mesh_view_bindings::view,
};
import bevy_pbr::render::view_transformations::position_world_to_clip;
import package::nuages_commun::{perlin3, worley3};

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
    amplitude_houle: f32, // amplitude crête totale de la houle (m)
    tailles: vec4<f32>,  // taille du motif de chaque cascade FFT (m), w = 1 si la FFT est active
    lod_proche: f32,
    lod_loin: f32,
    pluie: f32,          // intensité de la pluie [0, 1]
    _r2: f32,
    rivage: vec4<f32>,      // grille du rivage : origine x, origine z, côté (m), 1 si active
    bord: vec4<f32>,        // vagues de bord : amplitude, longueur d'onde, jet de rive, pulsation
    couleur_fond: vec4<f32>,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> ocean: ExtensionOcean;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var deplacement_fft: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var echantillonneur: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var derivees_fft: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var carte_rivage: texture_2d<f32>;

const TAU: f32 = 6.28318530718;

// ---------------------------------------------------------------------------------------------------------------
// Vagues de bord (copie de `rivage.rs`).

struct Rivage {
    fond: f32,
    distance: f32,          // distance signée au rivage (m), < 0 en mer
    normale: vec2<f32>,     // vers la terre
    poids: f32,             // 0 hors de la grille, fondu sur les bords de la grille
};

fn lire_rivage(xz: vec2<f32>) -> Rivage {
    var r: Rivage;
    r.fond = -1e4;
    r.distance = -1e4;
    r.normale = vec2<f32>(0.0);
    r.poids = 0.0;
    if (ocean.rivage.w <= 0.0) {
        return r;
    }
    let n = vec2<i32>(textureDimensions(carte_rivage));
    let nf = vec2<f32>(n);
    let u = (xz - ocean.rivage.xy) / ocean.rivage.z;
    if (any(u < vec2<f32>(0.0)) || any(u > vec2<f32>(1.0))) {
        return r;
    }
    let q = clamp(u * nf - 0.5, vec2<f32>(0.0), nf - 1.0);
    let i0 = vec2<i32>(floor(q));
    let i1 = min(i0 + 1, n - 1);
    let t = q - floor(q);
    let a = textureLoad(carte_rivage, i0, 0);
    let b = textureLoad(carte_rivage, vec2<i32>(i1.x, i0.y), 0);
    let c = textureLoad(carte_rivage, vec2<i32>(i0.x, i1.y), 0);
    let d = textureLoad(carte_rivage, i1, 0);
    let v = mix(mix(a, b, t.x), mix(c, d, t.x), t.y);
    r.fond = v.x;
    r.distance = v.y;
    r.normale = select(vec2<f32>(0.0), normalize(v.zw), dot(v.zw, v.zw) > 1e-8);
    // Fondu sur les 8 % du bord de la grille : pas de couture quand la grille se déplace.
    let bord = min(min(u.x, 1.0 - u.x), min(u.y, 1.0 - u.y));
    r.poids = smoothstep(0.0, 0.08, bord);
    return r;
}

fn attenuation_houle(profondeur: f32) -> f32 {
    let rampe = 0.12 + 0.88 * saturate(profondeur / 6.0);
    let limite = (0.35 * max(profondeur, 0.0) + 0.03) / max(ocean.amplitude_houle, 1e-3);
    return saturate(min(rampe, limite));
}

fn phase_bord(xz: vec2<f32>, distance: f32) -> f32 {
    let k = TAU / max(ocean.bord.y, 1.0);
    let ondulation = perlin3(vec3<f32>(xz.x * 0.015, 1.3, xz.y * 0.015), 0) * 4.0;
    return k * distance + ondulation - ocean.bord.w * ocean.temps;
}

struct Bord {
    deplacement: vec3<f32>,
    pente: vec2<f32>,
    ecume: f32,
};

fn vagues_bord(xz: vec2<f32>, r: Rivage, profondeur: f32) -> Bord {
    var o: Bord;
    o.deplacement = vec3<f32>(0.0);
    o.pente = vec2<f32>(0.0);
    o.ecume = 0.0;
    let d = r.distance;
    let enveloppe = smoothstep(-170.0, -70.0, d) * (1.0 - smoothstep(15.0, 35.0, d));
    if (enveloppe <= 0.0 || ocean.bord.x <= 0.0 || dot(r.normale, r.normale) == 0.0) {
        return o;
    }
    let n = r.normale;
    let k = TAU / max(ocean.bord.y, 1.0);
    let th = phase_bord(xz, d);
    let h = max(profondeur, 0.05);
    var a = ocean.bord.x * clamp(pow(4.0 / h, 0.25), 0.6, 1.7);     // levée de la houle (Green)
    let a_max = 0.39 * h + 0.03;                                     // déferlement (McCowan)
    let deferlement = saturate((a / a_max - 0.7) * 3.0);
    a = min(a, a_max) * enveloppe;
    let q = mix(0.35, 0.95, saturate(1.0 - h / 3.0));                // crête penchée vers la plage
    let s = sin(th);
    let c = cos(th);
    let horizontal = -n * (q * a * s);
    o.deplacement = vec3<f32>(horizontal.x, a * c, horizontal.y);
    o.pente = n * (-a * k * s);
    // Jet de rive.
    let ressac = pow(saturate(0.5 + 0.5 * cos(phase_bord(xz, 0.0))), 1.5);
    o.deplacement.y += ocean.bord.z * ressac * smoothstep(-70.0, -20.0, d);
    // Écume de déferlement et traînée.
    let crete = pow(saturate(c), 6.0);
    let derriere = fract(-th / TAU);
    o.ecume = deferlement * max(crete, exp(-derriere * 5.0) * 0.85) * enveloppe;
    o.deplacement *= r.poids;
    o.pente *= r.poids;
    o.ecume *= r.poids;
    return o;
}

// Motif « dentelle » de l'écume (réseau de bulles), à un instant donné.
fn dentelle_a(xz: vec2<f32>, t: f32) -> f32 {
    let a = 1.0 - worley3(vec3<f32>(xz * 0.55, t * 0.12), 0);
    let b = 1.0 - worley3(vec3<f32>(xz * 1.7 + 13.0, t * 0.2), 0);
    return saturate(smoothstep(0.25, 0.6, a) * 0.7 + smoothstep(0.3, 0.7, b) * 0.5);
}

// Fondu enchaîné entre t et t + 200 s : continu quand le temps de l'océan reboucle sur 200 s.
fn dentelle(xz: vec2<f32>) -> f32 {
    let f = ocean.temps / 200.0;
    return mix(dentelle_a(xz, ocean.temps), dentelle_a(xz, ocean.temps + 200.0), 1.0 - f);
}

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
    var depl = g.deplacement + fft;
    let riv = lire_rivage(xz);
    if (riv.poids > 0.0) {
        let profondeur = ocean.niveau - riv.fond;
        depl = depl * mix(1.0, attenuation_houle(profondeur), riv.poids)
            + vagues_bord(xz, riv, profondeur).deplacement;
    }
    let monde = vec3<f32>(xz.x, ocean.niveau, xz.y) + depl;
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

// Ronds des impacts de pluie sur l'eau : anneaux qui s'élargissent (jusqu'à ~35 cm), une goutte par cellule et par
// période. Renvoie la pente ajoutée. Le temps de l'océan boucle sur 200 s : 240 périodes entières, pas de saut.
fn ronds_pluie(xz: vec2<f32>, temps: f32) -> vec2<f32> {
    var grad = vec2<f32>(0.0);
    for (var couche = 0; couche < 2; couche++) {
        let p = xz * (2.5 + f32(couche) * 1.3) + f32(couche) * 17.0;
        let cellule = vec2<i32>(floor(p));
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let c = cellule + vec2<i32>(x, y);
                let h = hachage(u32(c.x) ^ hachage(u32(c.y) ^ hachage(u32(couche) + 77u)));
                let hx = f32(h & 0xFFFFu) / 65535.0;
                let hy = f32((h >> 16u) & 0xFFFFu) / 65535.0;
                let hz = f32(hachage(h) & 0xFFFFu) / 65535.0;
                let t = fract(temps * 1.2 + hz);
                let dv = p - (vec2<f32>(c) + vec2<f32>(hx, hy));
                let r = length(dv);
                let anneau = r - t * 0.9;
                let onde = sin(anneau * 40.0) * exp(-anneau * anneau * 400.0) * (1.0 - t) * (1.0 - t);
                grad += dv / max(r, 1e-3) * onde;
            }
        }
    }
    return grad;
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
    // Rivage : houle atténuée en eau peu profonde, pente et écume des vagues de bord.
    let riv = lire_rivage(repos);
    var ecume_bord = 0.0;
    var eau = 1e4;
    if (riv.poids > 0.0) {
        let profondeur = ocean.niveau - riv.fond;
        let b = vagues_bord(repos, riv, profondeur);
        pente = pente * mix(1.0, attenuation_houle(profondeur), riv.poids) + b.pente;
        // Hauteur d'eau au-dessus du fond, sous ce pixel.
        eau = max(monde.y - lire_rivage(monde.xz).fond, 0.0);
        let lace = dentelle(monde.xz);
        let surf = 1.0 - smoothstep(0.0, 1.3, eau);
        // Pulsation quantifiée (29 cycles sur 200 s) pour boucler avec le temps de l'océan.
        let respiration = 0.55 + 0.45 * sin(ocean.temps * (29.0 * TAU / 200.0)
            + perlin3(vec3<f32>(monde.xz * 0.15, 0.7), 0) * 4.5);
        let lisere = surf * respiration * mix(0.5, 1.0, lace);
        let nappe = b.ecume * mix(0.25, 1.15, lace);
        let pellicule = (1.0 - smoothstep(0.0, 0.6, eau)) * 0.35 * lace;
        ecume_bord = max(lisere, max(nappe, pellicule)) * riv.poids;
    }
    // Pluie : ronds d'impact près de la caméra (au-delà, ils sont plus petits qu'un pixel : la rugosité prend
    // le relais, plus bas).
    let pres_pluie = ocean.pluie * (1.0 - smoothstep(18.0, 40.0, distance));
    if (pres_pluie > 0.01) {
        pente += ronds_pluie(monde.xz, ocean.temps) * pres_pluie * 0.9;
    }
    var n = normalize(vec3<f32>(-pente.x, 1.0, -pente.y));
    // Lissage lointain : la normale moyenne d'un pixel lointain tend vers le haut.
    n = normalize(mix(n, vec3<f32>(0.0, 1.0, 0.0), saturate(distance / 12000.0)));
    pbr_input.N = n;
    pbr_input.world_normal = n;

    // Rugosité : un peu plus forte au loin et par vent fort.
    // Sous la pluie, des milliers d'impacts dépolissent la surface : la mer devient plus mate.
    pbr_input.material.perceptual_roughness = mix(0.04, 0.16, saturate(distance / 3000.0)) + ocean.vent * 0.003
        + ocean.pluie * 0.07;

    // Eau peu profonde : on voit le fond à travers une couche absorbante (le rouge est absorbé d'abord).
    if (eau < 1e3) {
        let fond = ocean.couleur_fond.rgb * exp(-eau * vec3<f32>(0.6, 0.14, 0.1));
        pbr_input.material.base_color = vec4<f32>(
            mix(pbr_input.material.base_color.rgb, fond, exp(-eau * 0.28)),
            1.0,
        );
    }

    // Écume : crêtes qui se replient (jacobien FFT, et compression de la houle), en motif de dentelle.
    let motif = bruit_valeur(monde.xz * 0.8 + ocean.temps * 0.05) * 0.6 + bruit_valeur(monde.xz * 3.1) * 0.4;
    let ecume_houle = saturate((g.crete - 0.45) * 2.5) * smoothstep(4.0, 12.0, ocean.vent);
    let ecume = saturate(max(max(ecume_fft * mix(0.5, 1.2, motif), ecume_houle * smoothstep(0.35, 0.75, motif)), ecume_bord) * ocean.ecume);
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
