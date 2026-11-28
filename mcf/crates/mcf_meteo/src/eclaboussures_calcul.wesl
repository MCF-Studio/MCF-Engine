// Impacts des gouttes de pluie : pour chaque éclaboussure, cherche où sa goutte touche ce que la caméra voit
// (sol, objets, mer) et écrit le point d'impact dans une texture, lue ensuite par `eclaboussures.wesl`.
//
// Éclaboussures sans état : la position (x, z) de chacune est une fonction de son indice et de son cycle de vie
// (`vie` secondes), fixe dans le monde pendant le cycle. La goutte tombe à la verticale ; on descend le long de
// cette verticale et on compare chaque point à la profondeur de la vue : le premier passage de « devant » à
// « derrière » la géométrie est l'impact, affiné par dichotomie. Un passage derrière une silhouette (la verticale
// passe derrière un mur vu de la caméra) est écarté : l'écart de profondeur y est grand. Une verticale qui
// commence cachée (toit vu d'en dessous) ou qui finit sous un abri déclaré ne donne pas d'impact.
//
// Limites : seuls les impacts visibles à l'écran existent (c'est le but), caméra en perspective seulement.

import bevy_render::view::View;

struct Params {
    temps: f32,         // temps bouclé (s), multiple entier de `vie` à la boucle
    actives: f32,       // nombre d'éclaboussures vivantes
    demi_largeur: f32,  // demi-côté de la zone autour de la caméra (m)
    vie: f32,           // durée d'une éclaboussure (s)
    abris: vec4<f32>,   // carte des abris : origine x, origine z, côté d'une case, cases par côté
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<uniform> p: Params;
// Profondeur de la vue : multi-échantillonnée si la caméra a du MSAA (on lit l'échantillon 0).
@if(MULTISAMPLED)
@group(0) @binding(2) var profondeur: texture_depth_multisampled_2d;
@else
@group(0) @binding(2) var profondeur: texture_depth_2d;
// xyz = point d'impact (monde), w = âge normalisé (0 → 1), négatif s'il n'y a pas d'impact visible.
@group(0) @binding(3) var sortie: texture_storage_2d<rgba32float, write>;
// Hauteur du plus haut toit au-dessus de chaque case (R32Float) : pas d'impact dessous, même si le toit est hors
// de l'écran.
@group(0) @binding(4) var carte_abris: texture_2d<f32>;

fn sous_abri(pos: vec3<f32>) -> bool {
    let n = i32(p.abris.w);
    let c = vec2<i32>(floor((pos.xz - p.abris.xy) / p.abris.z));
    if (n <= 0 || any(c < vec2<i32>(0)) || any(c >= vec2<i32>(n))) {
        return false;
    }
    return pos.y < textureLoad(carte_abris, c, 0).r - 0.05;
}

const HORS_ECRAN: i32 = 0;
const DEVANT: i32 = 1;
const DERRIERE: i32 = 2;

// Hauteur explorée au-dessus et au-dessous de la caméra (m), et nombre de pas.
const AU_DESSUS: f32 = 6.0;
const AU_DESSOUS: f32 = 18.0;
const PAS: i32 = 40;
const DICHOTOMIE: i32 = 6;

fn hachage(x_in: u32) -> u32 {
    var x = x_in;
    x ^= x >> 16u; x *= 0x7feb352du;
    x ^= x >> 15u; x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}
fn hachage_f(x: u32) -> f32 { return f32(hachage(x) & 0x00FFFFFFu) / 16777216.0; }

struct Etat {
    code: i32,
    ecart: f32,     // distance de vue du point − distance de vue de la scène (m), si derrière
    distance: f32,  // distance de vue du point (m)
};

fn etat(pos: vec3<f32>) -> Etat {
    var e: Etat;
    e.code = HORS_ECRAN;
    let clip = view.clip_from_world * vec4<f32>(pos, 1.0);
    if (clip.w <= 0.05) {
        return e;
    }
    let ndc = clip.xyz / clip.w;
    if (any(abs(ndc.xy) >= vec2<f32>(1.0))) {
        return e;
    }
    let taille = vec2<f32>(textureDimensions(profondeur));
    let px = vec2<i32>(clamp((ndc.xy * vec2<f32>(0.5, -0.5) + 0.5) * taille, vec2<f32>(0.0), taille - 1.0));
    let d = textureLoad(profondeur, px, 0);
    e.distance = clip.w;
    // Z inversé : plus petit = plus loin. Perspective infinie : distance de vue = proche / profondeur.
    if (ndc.z < d) {
        e.code = DERRIERE;
        e.ecart = clip.w - view.clip_from_view[3][2] / d;
    } else {
        e.code = DEVANT;
    }
    return e;
}

@compute @workgroup_size(8, 8, 1)
fn calculer(@builtin(global_invocation_id) id: vec3<u32>) {
    let cote = textureDimensions(sortie).x;
    let i = id.y * cote + id.x;
    var resultat = vec4<f32>(0.0, 0.0, 0.0, -1.0);
    if (f32(i) < p.actives) {
        let h = hachage(i * 0x9E3779B9u + 0x7F4A7C15u);
        let cycles = p.temps / p.vie + hachage_f(h);
        let age = fract(cycles);
        let h2 = hachage(h ^ (u32(floor(cycles)) * 0x85EBCA6Bu));
        let r = vec2<f32>(hachage_f(h2), hachage_f(h2 ^ 0xC2B2AE35u));
        // Position fixe dans le monde, prise dans le carré centré sur la caméra.
        let boite = 2.0 * p.demi_largeur;
        let camera = view.world_position;
        let xz = camera.xz + (fract((r * boite - camera.xz) / boite + 0.5) - 0.5) * boite;

        let sommet = camera.y + AU_DESSUS;
        let pas = (AU_DESSUS + AU_DESSOUS) / f32(PAS);
        var precedent = HORS_ECRAN;
        var commence = false;
        for (var k = 0; k <= PAS; k++) {
            let y = sommet - f32(k) * pas;
            let e = etat(vec3<f32>(xz.x, y, xz.y));
            if (!commence && e.code != HORS_ECRAN) {
                commence = true;
                // Premier point à l'écran déjà caché : la verticale arrive de derrière quelque chose (un toit
                // vu d'en dessous, un mur) : pas d'impact pour elle.
                if (e.code == DERRIERE) {
                    break;
                }
            }
            if (e.code == DERRIERE && precedent == DEVANT) {
                var y_devant = y + pas;
                var y_derriere = y;
                for (var j = 0; j < DICHOTOMIE; j++) {
                    let m = 0.5 * (y_devant + y_derriere);
                    if (etat(vec3<f32>(xz.x, m, xz.y)).code == DERRIERE) {
                        y_derriere = m;
                    } else {
                        y_devant = m;
                    }
                }
                let fin = etat(vec3<f32>(xz.x, y_derriere, xz.y));
                // Vraie surface : l'écart tend vers 0. Silhouette : il reste grand.
                if (fin.code == DERRIERE && fin.ecart < 0.3 + 0.03 * fin.distance) {
                    if (!sous_abri(vec3<f32>(xz.x, y_devant, xz.y))) {
                        resultat = vec4<f32>(xz.x, y_devant, xz.y, age);
                    }
                    break;
                }
            }
            precedent = e.code;
        }
    }
    textureStore(sortie, vec2<i32>(id.xy), resultat);
}
