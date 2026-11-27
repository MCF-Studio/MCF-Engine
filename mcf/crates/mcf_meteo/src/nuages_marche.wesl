// Ray marching volumétrique des nuages en demi résolution, porté de `cloud_march.hlsl` de SkyForge.
//
//  * Transmittance (Beer-Lambert) : T(t) = exp(-∫ σt ds).
//  * Diffusion intégrée par segment de façon conservative : (S - S e^{-σ dt}) / σ (Hillaire 2015).
//  * Phase de Henyey-Greenstein double lobe : mix(HG(g1), HG(g2), 0,3).
//  * Effet « powder » (Schneider 2015) : assombrit les bords fins tournés vers le soleil.
//  * Diffusion multiple (Wrenninge 2013) : 3 octaves a^n · phase(c^n g) · e^{-b^n τ}, a = b = c = 0,5.
//  * Pas adaptatif : fin près de la caméra (on peut voler dedans), grossier au loin ; dans le vide on avance
//    par grands pas avec la densité grossière, et dès qu'on touche un nuage on recule d'un pas et on affine.
//
// Différence avec SkyForge : unités physiques. L'éclairement du soleil (lux) multiplié par la phase (1/sr)
// donne une luminance (cd/m²), multipliée ensuite par l'exposition de la vue. Au loin, les nuages deviennent
// transparents au lieu de se fondre dans un ciel analytique : le ciel de l'atmosphère de Bevy prend le relais.

import bevy_render::view::View;
import package::nuages_commun::{ParamsNuages, PI, remapper};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> p: ParamsNuages;
@group(0) @binding(2) var profondeur: texture_depth_2d;
@group(0) @binding(3) var forme: texture_3d<f32>;
@group(0) @binding(4) var detail: texture_3d<f32>;
@group(0) @binding(5) var carte: texture_2d<f32>;
@group(0) @binding(6) var echantillonneur: sampler;
@group(0) @binding(7) var sortie: texture_storage_2d<rgba16float, write>; // rgb = lumière, a = transmittance
@group(0) @binding(8) var<uniform> r: ParamsReprojection;
@group(0) @binding(9) var sortie_distance: texture_storage_2d<r32float, write>; // distance moyenne des nuages (m)

// Reprojection temporelle : chaque image ne marche qu'un pixel par bloc `pas` × `pas` de la cible (pixel
// `bloc × pas + decalage`) ; la passe de reconstruction complète les autres depuis l'image précédente.
struct ParamsReprojection {
    precedente: mat4x4<f32>,  // clip_from_world de l'image précédente
    taille_cible: vec2<u32>,  // texture complète (demi résolution de l'écran)
    decalage: vec2<u32>,      // pixel marché dans chaque bloc à cette image
    pas: u32,                 // 2 avec reprojection, 1 sans
    valide: u32,              // 0 : pas d'historique utilisable
    r0: u32,
    r1: u32,
};

fn ecrire(bloc: vec2<u32>, couleur: vec4<f32>, distance: f32) {
    textureStore(sortie, bloc, couleur);
    textureStore(sortie_distance, bloc, vec4(distance, 0.0, 0.0, 0.0));
}

fn hg(c: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(max(1.0 + g2 - 2.0 * g * c, 1e-4), 1.5));
}

fn gradient_hauteur(h: f32, type_nuage: f32) -> f32 {
    // Stratus : couche basse et fine ; cumulonimbus : toute la hauteur.
    let sommet = mix(0.3, 1.0, type_nuage);
    let bas = saturate(h / 0.07);
    let haut = saturate((sommet - h) / max(sommet * 0.45, 1e-3));
    return bas * haut;
}

fn densite(pos: vec3<f32>, grossiere: bool, detail_proche: f32) -> f32 {
    let h = (pos.y - p.bas) / (p.haut - p.bas);
    if h <= 0.0 || h >= 1.0 {
        return 0.0;
    }
    let mc = textureSampleLevel(carte, echantillonneur, (pos.xz + p.decalage) / 30000.0, 0.0);
    // La pluie épaissit et abaisse les nuages.
    let couverture = saturate(p.couverture + (mc.r - 0.5) * 0.9 + p.precipitations * 0.15);
    let type_nuage = saturate(p.type_nuages + (mc.g - 0.5) * 0.4);

    let decalage3 = vec3(p.decalage.x, 0.0, p.decalage.y);
    let s = textureSampleLevel(forme, echantillonneur, (pos + decalage3) * p.echelle_forme, 0.0);
    let fbm = s.g * 0.625 + s.b * 0.25 + s.a * 0.125;
    var base = remapper(s.r, -(1.0 - fbm), 1.0, 0.0, 1.0);
    // Écart assumé avec SkyForge : mesurée sur 1 500 points, cette forme ne varie qu'entre 0,77 et 0,89
    // (5e et 95e centiles), si bien qu'une couverture de 0,6 donnait déjà un ciel entièrement couvert. On l'étale
    // sur [0, 1] pour que la couverture soit à peu près la fraction du ciel couverte (sens météorologique).
    base = saturate(remapper(base, 0.77, 0.89, 0.0, 1.0));
    base *= gradient_hauteur(h, type_nuage);
    base = saturate(remapper(base, 1.0 - couverture, 1.0, 0.0, 1.0)) * couverture;
    if grossiere || base <= 0.0 {
        return base;
    }

    // Érosion par le détail (vaporeux en bas, bouillonnant en haut), entraînée par le vent ; distorsion par les
    // canaux de la forme pour que les détails « tourbillonnent ».
    let tourbillon = (s.gba - 0.5) * 0.35;
    let dp = (pos + vec3(p.decalage.x, -p.temps * 2.0, p.decalage.y) * 1.3) * p.echelle_detail + tourbillon;
    let d = textureSampleLevel(detail, echantillonneur, dp, 0.0);
    var dfbm = d.r * 0.625 + d.g * 0.25 + d.b * 0.125;
    if detail_proche > 0.0 {
        // Octave très fine, seulement près de la caméra (volutes visibles en vol).
        let d2 = textureSampleLevel(detail, echantillonneur, dp * 4.7 + tourbillon * 2.0, 0.0);
        dfbm = mix(dfbm, dfbm * 0.75 + d2.r * 0.25, detail_proche);
    }
    let modulation = mix(dfbm, 1.0 - dfbm, saturate(h * 6.0));
    return saturate(remapper(base, modulation * p.force_detail, 1.0, 0.0, 1.0));
}

fn profondeur_optique_soleil(pos: vec3<f32>) -> f32 {
    var od = 0.0;
    var pas = (p.haut - p.bas) * 0.35 / f32(p.pas_lumiere);
    var t = pas * 0.5;
    for (var i = 0u; i < p.pas_lumiere; i++) {
        let q = pos + p.soleil_direction * t;
        od += densite(q, i + 1u == p.pas_lumiere, 0.0) * pas;
        t += pas;
        pas *= 1.6; // cône qui s'élargit
    }
    return od * p.extinction * p.densite;
}

fn monde_depuis_ndc(ndc: vec3<f32>) -> vec3<f32> {
    let w = view.world_from_clip * vec4(ndc, 1.0);
    return w.xyz / w.w;
}

@compute @workgroup_size(8, 8, 1)
fn marcher(@builtin(global_invocation_id) id: vec3<u32>) {
    if any(id.xy >= textureDimensions(sortie)) {
        return;
    }
    let taille = r.taille_cible;
    let pixel = min(id.xy * r.pas + r.decalage, taille - 1u);
    let uv = (vec2<f32>(pixel) + 0.5) / vec2<f32>(taille);
    let ndc = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let camera = view.world_position;
    let dir = normalize(monde_depuis_ndc(vec3(ndc, 1.0)) - camera);

    // Géométrie opaque : profondeur la plus proche de l'empreinte du pixel (Z inversé : plus grand = plus
    // proche), pour que les nuages ne débordent pas sur les silhouettes au premier plan.
    let taille_prof = vec2<i32>(textureDimensions(profondeur));
    let p0 = vec2<i32>(floor(uv * vec2<f32>(taille_prof) - 0.5));
    var prof = 0.0;
    for (var j = 0; j < 2; j++) {
        for (var i = 0; i < 2; i++) {
            let c = clamp(p0 + vec2(i, j), vec2(0), taille_prof - 1);
            prof = max(prof, textureLoad(profondeur, c, 0));
        }
    }
    var distance_scene = 1e9;
    if prof > 0.0 {
        distance_scene = length(monde_depuis_ndc(vec3(ndc, prof)) - camera);
    }

    // Intersection avec la couche (Terre plate, suffisant à cette échelle).
    var t_entree = 0.0;
    var t_sortie = 0.0;
    if abs(dir.y) < 1e-4 {
        let dedans = camera.y > p.bas && camera.y < p.haut;
        t_sortie = select(0.0, p.distance_max, dedans);
    } else {
        let tb = (p.bas - camera.y) / dir.y;
        let th = (p.haut - camera.y) / dir.y;
        t_entree = max(min(tb, th), 0.0);
        t_sortie = max(tb, th);
    }
    t_sortie = min(min(t_sortie, p.distance_max), distance_scene);
    if t_sortie <= t_entree || p.couverture <= 0.0 {
        ecrire(id.xy, vec4(0.0, 0.0, 0.0, 1.0), select(p.distance_max, t_entree, t_entree > 0.0));
        return;
    }

    // Pas qui grandit avec la distance. Décalage stable par pixel de la cible.
    let longueur = t_sortie - t_entree;
    let pas_loin = clamp(longueur / f32(p.pas), 40.0, 600.0);
    let gigue = fract(52.9829189 * fract(dot(vec2<f32>(pixel), vec2(0.06711056, 0.00583715))));
    var t = t_entree + gigue * min(p.pas_proche + t_entree * 0.02, pas_loin);

    let cos_theta = dot(dir, p.soleil_direction);
    let poudre_force = p.poudre * saturate(0.6 - 0.6 * cos_theta);
    // Soleil : teinte chaude quand il est bas, éteint sous l'horizon (comme SkyForge).
    let h_soleil = saturate(p.soleil_direction.y);
    let visibilite = saturate(p.soleil_direction.y * 4.0 + 0.15);
    let chaud = mix(vec3(1.0, 0.45, 0.2), vec3(1.0, 0.96, 0.9), saturate(h_soleil * 3.0));
    let soleil = p.soleil_couleur * chaud * p.soleil_lux * visibilite;
    // Ambiance : ciel + lumière diffusée plusieurs fois dans la masse (au-delà des 3 octaves), environ E / 12
    // en luminance pour un soleil haut, désaturée par la couverture ; bases plus sombres, encore plus sous la
    // pluie. Calée à l'œil sur le ciel de l'atmosphère de Bevy (pas de mesure de référence).
    let ciel = mix(vec3(0.6, 0.72, 1.0), vec3(0.85), saturate(p.couverture * 1.25 - 0.25));
    let amb_haut = ciel * p.soleil_lux * (0.15 + 0.85 * saturate(p.soleil_direction.y * 2.0 + 0.15))
        * 0.12 * p.ambiance;
    let amb_bas = amb_haut * mix(vec3(0.45, 0.48, 0.55), vec3(0.22, 0.23, 0.26), p.precipitations);

    var diffusion = vec3(0.0);
    var tr = 1.0;
    var somme_dist = 0.0;
    var poids_dist = 0.0;
    var fin = false;
    var vides = 0u;

    for (var i = 0u; i < p.pas * 3u && t < t_sortie; i++) {
        let pas = min(p.pas_proche + t * 0.02, pas_loin);
        let pos = camera + dir * t;
        if !fin {
            if densite(pos, true, 0.0) > 0.0 {
                fin = true;
                vides = 0u;
                t = max(t - pas, t_entree);
            } else {
                t += pas * 2.0;
            }
            continue;
        }
        let detail_proche = 1.0 - smoothstep(150.0, 600.0, t);
        let d = densite(pos, false, detail_proche);
        if d > 0.002 {
            vides = 0u;
            let sigma = d * p.extinction * p.densite;
            let tau = profondeur_optique_soleil(pos);
            let poudre = mix(1.0, saturate(2.0 * (1.0 - exp(-2.0 * tau))), poudre_force);
            var s = 0.0;
            var an = 1.0;
            var bn = 1.0;
            var cn = 1.0;
            for (var o = 0; o < 3; o++) {
                let phase = mix(hg(cos_theta, p.g_avant * cn), hg(cos_theta, p.g_arriere * cn), 0.3);
                s += an * phase * exp(-tau * bn);
                an *= 0.5;
                bn *= 0.5;
                cn *= 0.5;
            }
            let hn = saturate((pos.y - p.bas) / (p.haut - p.bas));
            let ls = soleil * s * poudre + mix(amb_bas, amb_haut, hn);
            let ti = exp(-sigma * pas);
            diffusion += tr * (ls - ls * ti);
            somme_dist += t * tr * (1.0 - ti);
            poids_dist += tr * (1.0 - ti);
            tr *= ti;
            if tr < 0.01 {
                tr = 0.0;
                break;
            }
        } else {
            vides += 1u;
            if vides > 6u {
                fin = false;
            }
        }
        t += pas;
    }

    // Au loin, les nuages s'effacent (le ciel de l'atmosphère reprend la main).
    let loin = select(t_entree, somme_dist / max(poids_dist, 1e-6), poids_dist > 0.0);
    let brume = saturate(loin / p.distance_fondu);
    let f = brume * brume;
    diffusion *= 1.0 - f;
    tr = mix(tr, 1.0, f);

    ecrire(id.xy, vec4(diffusion * view.exposure, tr), loin);
}
