// Surfaces mouillées, flaques et neige, par-dessus le StandardMaterial de Bevy. Porté de `WetSurface.hlsli` de
// SkyForge (d'après Lagarde, « Water drop », 2013) :
//  1. L'eau remplit les pores : l'albédo s'assombrit, d'autant plus que le matériau est poreux.
//  2. Un film d'eau lisse la surface : la rugosité baisse.
//  3. Dans les flaques (surfaces planes, creux du bruit), l'eau domine : rugosité ~0, normale plate avec les
//     ondulations des impacts de pluie.
//  4. La neige recouvre les faces tournées vers le ciel.
// Les zones abritées (carte des abris) restent sèches et sans neige.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    mesh_view_bindings::{globals, view},
};

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

struct ExtensionMeteo {
    mouillage: f32,
    flaques: f32,
    neige: f32,
    pluie: f32,          // intensité de la pluie (ondulations dans les flaques)
    porosite: f32,       // 0 = métal / verre, 1 = très poreux (terre, béton brut)
    reception_flaques: f32, // 0 = jamais de flaques (murs, toits en pente), 1 = sol
    niveau_mer: f32,     // niveau de la mer (m)
    hauteur_mouillee: f32, // hauteur au-dessus de la mer jusqu'où les vagues mouillent le sable (m)
    abris: vec4<f32>,    // carte des abris : origine x, origine z, côté d'une case, cases par côté
    rivage: vec4<f32>,   // grille du rivage : origine x, origine z, côté (m), 1 si active
};

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> meteo: ExtensionMeteo;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var carte_abris: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var carte_rivage: texture_2d<f32>;

fn hachage(x_in: u32) -> u32 {
    var x = x_in;
    x ^= x >> 16u; x *= 0x7feb352du;
    x ^= x >> 15u; x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}
fn hachage_f(x: u32) -> f32 { return f32(hachage(x) & 0x00FFFFFFu) / 16777216.0; }
fn hachage33(c: vec3<i32>) -> vec3<f32> {
    let h = hachage(u32(c.x) ^ hachage(u32(c.y) ^ hachage(u32(c.z))));
    return vec3<f32>(hachage_f(h), hachage_f(h ^ 0x9E3779B9u), hachage_f(h ^ 0x85EBCA6Bu));
}

// Bruit de valeur 2D lissé, pour placer les flaques.
fn bruit_valeur(p: vec2<f32>) -> f32 {
    let i = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hachage33(vec3<i32>(i, 7)).x;
    let b = hachage33(vec3<i32>(i + vec2<i32>(1, 0), 7)).x;
    let c = hachage33(vec3<i32>(i + vec2<i32>(0, 1), 7)).x;
    let d = hachage33(vec3<i32>(i + vec2<i32>(1, 1), 7)).x;
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn sous_abri(p: vec3<f32>) -> bool {
    let n = i32(meteo.abris.w);
    let c = vec2<i32>(floor((p.xz - meteo.abris.xy) / meteo.abris.z));
    if (n <= 0 || any(c < vec2<i32>(0)) || any(c >= vec2<i32>(n))) {
        return false;
    }
    // Petite marge : une surface juste sous le toit est abritée, le toit lui-même ne l'est pas.
    return p.y < textureLoad(carte_abris, c, 0).r - 0.05;
}

// Sable mouillé par les vagues de bord : sous la hauteur atteinte par l'eau (jet de rive), et seulement près du
// rivage (distance signée de la grille du rivage, lue au plus proche : les cases font 2 m).
fn rive_mouillee(p: vec3<f32>) -> f32 {
    if (meteo.rivage.w <= 0.0) {
        return 0.0;
    }
    let n = vec2<i32>(textureDimensions(carte_rivage));
    let c = vec2<i32>(floor((p.xz - meteo.rivage.xy) / meteo.rivage.z * vec2<f32>(n)));
    if (any(c < vec2<i32>(0)) || any(c >= n)) {
        return 0.0;
    }
    let distance = textureLoad(carte_rivage, c, 0).g;
    let pres = 1.0 - smoothstep(40.0, 60.0, distance);
    // Limite légèrement ondulée (motif de ~7 m, ±4 cm de hauteur : sur une plage en pente douce, une plus grande
    // variation donnait des taches sur des dizaines de mètres), fondu sur 25 cm (sable humide puis sec).
    let bord = (bruit_valeur(p.xz * 0.15) - 0.5) * 0.08;
    let h = p.y - meteo.niveau_mer;
    return pres * (1.0 - smoothstep(meteo.hauteur_mouillee - 0.25, meteo.hauteur_mouillee, h + bord));
}

// Ondulations des impacts de pluie : anneaux qui s'élargissent, une goutte par cellule et par période.
fn ondulations(xz: vec2<f32>, temps: f32, intensite: f32) -> vec3<f32> {
    var grad = vec2<f32>(0.0);
    for (var couche = 0; couche < 2; couche++) {
        let p = xz * (2.5 + f32(couche) * 1.3) + f32(couche) * 17.0;
        let cellule = vec2<i32>(floor(p));
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let c = cellule + vec2<i32>(x, y);
                let h = hachage33(vec3<i32>(c, couche));
                let centre = vec2<f32>(c) + h.xy;
                let t = fract(temps * 1.2 + h.z);
                let dv = p - centre;
                let r = length(dv);
                let anneau = r - t * 0.9;
                let onde = sin(anneau * 40.0) * exp(-anneau * anneau * 400.0) * (1.0 - t) * (1.0 - t);
                grad += dv / max(r, 1e-3) * onde;
            }
        }
    }
    grad *= intensite * 0.25;
    return normalize(vec3<f32>(-grad.x, 1.0, -grad.y));
}

// Ruissellement sur les faces raides (murs, carrosseries, poteaux) : des filets d'eau verticaux, légèrement
// sinueux, parcourus par des gouttes qui descendent. Renvoie (intensité, écart au centre du filet).
fn ruissellement(p: vec3<f32>, tangente: vec3<f32>, temps: f32) -> vec2<f32> {
    let u = dot(p, tangente) * 22.0;   // filets tous les ~4,5 cm
    let colonne = i32(floor(u));
    let h = hachage33(vec3<i32>(colonne, 91, 13));
    if (h.x > 0.4) {
        return vec2<f32>(0.0);
    }
    let sinuosite = (bruit_valeur(vec2<f32>(f32(colonne) * 7.3, p.y * 2.5)) - 0.5) * 0.5;
    let ecart = fract(u) - 0.5 - sinuosite;
    let filet = 1.0 - smoothstep(0.1, 0.28, abs(ecart));
    // Goutte qui descend le filet : tête nette en bas, traînée qui s'efface au-dessus.
    let s = fract(-p.y * 0.7 - temps * (0.5 + 0.8 * h.y) + h.z);
    return vec2<f32>(filet * (0.2 + 0.8 * smoothstep(0.55, 1.0, s)), ecart);
}

@fragment
fn fragment(
    in: VertexOutput,
    @builtin(front_facing) is_front: bool,
) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let p = pbr_input.world_position.xyz;
    let n = pbr_input.N;
    let expose = select(1.0, 0.0, sous_abri(p));
    let vers_ciel = saturate(n.y);

    // 1-2. Surface mouillée (les faces verticales reçoivent moins de pluie).
    // Les vagues mouillent aussi sous un abri (un ponton), et toutes les faces.
    let mouillage = max(meteo.mouillage * expose * mix(0.6, 1.0, vers_ciel), rive_mouillee(p));
    let porosite = meteo.porosite;
    pbr_input.material.base_color = vec4<f32>(
        pbr_input.material.base_color.rgb * mix(1.0, mix(0.85, 0.3, porosite), mouillage),
        pbr_input.material.base_color.a,
    );
    pbr_input.material.perceptual_roughness = mix(
        pbr_input.material.perceptual_roughness,
        0.07,
        mouillage * mix(1.0, 0.55, porosite),
    );

    // 3. Flaques : seulement sur les surfaces quasi horizontales, dans les creux d'un bruit basse fréquence.
    let plat = smoothstep(0.92, 0.98, n.y) * meteo.reception_flaques;
    let creux = bruit_valeur(p.xz * 0.35) * 0.7 + bruit_valeur(p.xz * 1.3) * 0.3;
    let seuil = 1.0 - meteo.flaques * 0.75;
    let flaque = smoothstep(seuil, seuil + 0.06, creux) * plat * expose * step(0.001, meteo.flaques);
    if (flaque > 0.0) {
        pbr_input.material.base_color = vec4<f32>(
            pbr_input.material.base_color.rgb * mix(1.0, 0.55, flaque),
            pbr_input.material.base_color.a,
        );
        pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.02, flaque);
        // L'eau d'une flaque est plane, quel que soit le relief dessous.
        pbr_input.N = normalize(mix(pbr_input.N, vec3<f32>(0.0, 1.0, 0.0), flaque));
    }
    // Ronds d'impact : pleinement dans les flaques, plus discrets sur le film d'eau de tout sol mouillé exposé
    // à la pluie. Seulement près de la caméra (plus loin, ils sont plus petits qu'un pixel).
    let distance = length(view.world_position - p);
    let pres = 1.0 - smoothstep(18.0, 35.0, distance);
    let film = mouillage * smoothstep(0.85, 0.97, n.y) * expose * 0.4;
    let ronds = max(flaque, film) * step(0.01, meteo.pluie) * pres;
    if (ronds > 0.01) {
        let n_ronds = ondulations(p.xz, globals.time, meteo.pluie);
        pbr_input.N = normalize(mix(pbr_input.N, n_ronds, ronds));
    }

    // Ruissellement sur les faces raides exposées, tant qu'il pleut ou que la surface est encore mouillée.
    // Moins visible sur un matériau poreux, qui boit l'eau.
    let raide = (1.0 - smoothstep(0.55, 0.8, abs(n.y))) * expose * pres;
    let force = raide * max(meteo.pluie, meteo.mouillage * 0.3) * (1.0 - 0.6 * porosite);
    if (force > 0.01) {
        let tangente = normalize(cross(n, vec3<f32>(0.0, 1.0, 0.0)));
        let r = ruissellement(p, tangente, globals.time);
        let eau = r.x * force;
        pbr_input.material.base_color = vec4<f32>(
            pbr_input.material.base_color.rgb * mix(1.0, 0.72, eau),
            pbr_input.material.base_color.a,
        );
        pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.04, eau);
        // Filet bombé : la normale penche de part et d'autre de son centre.
        pbr_input.N = normalize(pbr_input.N + tangente * r.y * 2.0 * eau);
    }

    // 4. Neige : recouvre les faces tournées vers le ciel, bord irrégulier.
    let irregularite = bruit_valeur(p.xz * 2.1) * 0.25;
    let neige = smoothstep(0.0, 0.25, meteo.neige * expose * smoothstep(0.35, 0.8, vers_ciel) - irregularite * (1.0 - meteo.neige));
    if (neige > 0.0) {
        pbr_input.material.base_color = vec4<f32>(
            mix(pbr_input.material.base_color.rgb, vec3<f32>(0.9, 0.92, 0.95), neige),
            pbr_input.material.base_color.a,
        );
        pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.75, neige);
        pbr_input.material.metallic = mix(pbr_input.material.metallic, 0.0, neige);
        pbr_input.N = normalize(mix(pbr_input.N, vec3<f32>(0.0, 1.0, 0.0), neige * 0.5));
    }

    pbr_input.material.base_color = alpha_discard(
        pbr_input.material.flags,
        pbr_input.material.alpha_cutoff,
        pbr_input.material.base_color,
    );

    var out: FragmentOutput;
@if(PREPASS_PIPELINE) {
    out = deferred_output(in, pbr_input);
} @else {
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
}
    return out;
}
