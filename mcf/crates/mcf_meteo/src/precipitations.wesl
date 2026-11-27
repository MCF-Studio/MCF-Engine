// Pluie et neige, portées de `rain_draw.hlsl` de SkyForge.
//
// Particules sans état : la position de chaque goutte est une fonction de son indice, du temps et du déplacement
// cumulé par le vent. Les gouttes vivent dans une boîte toroïdale centrée sur la caméra (elles « bouclent » au lieu
// de réapparaître). Les vitesses de chute et le flottement des flocons sont quantifiés pour que tout boucle
// exactement sur `periode` secondes : aucun saut visible, aucune perte de précision sur des heures de jeu.
//
// Maillage : un quad par particule. Attribut position = (coin.x, coin.y, indice de la particule).

import bevy_pbr::render::mesh_view_bindings::view;

struct Uniformes {
    decalage_vent: vec3<f32>,   // déplacement cumulé par le vent (m), bouclé sur la boîte
    temps: f32,                 // temps bouclé sur `periode` (s)
    boite: vec3<f32>,           // dimensions de la boîte (m)
    actives: f32,               // nombre de particules affichées
    vent: vec3<f32>,            // vent local (m/s), pour l'inclinaison des traînées
    facteur_neige: f32,         // part des particules qui sont des flocons
    couleur: vec3<f32>,         // éclairage des gouttes (linéaire)
    trainee: f32,               // durée d'exposition simulée (s) : longueur des traînées
    luminosite: f32,
    quantum_vitesse: f32,       // pas des vitesses de chute : hauteur de boîte / période (m/s)
    periode: f32,               // période de bouclage (s)
    _remplissage: f32,
    abris: vec4<f32>,           // carte des abris : origine x, origine z, côté d'une case, cases par côté
};
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> u: Uniformes;
// Hauteur du plus haut toit au-dessus de chaque case (R32Float).
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var carte_abris: texture_2d<f32>;

// Vrai si le point est sous un abri (toit, pont, véhicule).
fn sous_abri(p: vec3<f32>) -> bool {
    let n = i32(u.abris.w);
    let c = vec2<i32>(floor((p.xz - u.abris.xy) / u.abris.z));
    if (any(c < vec2<i32>(0)) || any(c >= vec2<i32>(n))) {
        return false;
    }
    return p.y < textureLoad(carte_abris, c, 0).r;
}

const PI: f32 = 3.14159265359;

fn hachage(x_in: u32) -> u32 {
    var x = x_in;
    x ^= x >> 16u; x *= 0x7feb352du;
    x ^= x >> 15u; x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}
fn hachage_f(x: u32) -> f32 { return f32(hachage(x) & 0x00FFFFFFu) / 16777216.0; }

struct Particule {
    position: vec3<f32>,
    vitesse: vec3<f32>,
    neige: bool,
    alpha: f32,
};

fn particule(i: u32) -> Particule {
    var p: Particule;
    let h = hachage(i * 0x9E3779B9u + 0x632BE5ABu);
    let r = vec3<f32>(hachage_f(h), hachage_f(h ^ 0x85EBCA6Bu), hachage_f(h ^ 0xC2B2AE35u));
    p.neige = hachage_f(h ^ 0x27D4EB2Fu) < u.facteur_neige;

    // Vitesse de chute, arrondie à un multiple de `quantum_vitesse` : en `periode` secondes, chaque particule
    // parcourt un nombre entier de hauteurs de boîte.
    let variation = 0.85 + 0.3 * hachage_f(h ^ 0x165667B1u);
    let chute_visee = select(9.0, 1.1, p.neige) * variation;
    let chute = max(round(chute_visee / u.quantum_vitesse), 1.0) * u.quantum_vitesse;

    var rel = r * u.boite + u.decalage_vent - vec3<f32>(0.0, chute * u.temps, 0.0);
    p.vitesse = vec3<f32>(u.vent.x, -chute, u.vent.z);

    if (p.neige) {
        // Flottement : fréquence entière sur la période, donc périodique lui aussi.
        let tours = round(u.periode * (1.5 + hachage_f(h ^ 0xD3A2646Cu)) / (2.0 * PI));
        let w = 2.0 * PI * tours / u.periode;
        let phase = w * u.temps + hachage_f(h ^ 0xFD7046C5u) * 2.0 * PI;
        let amplitude = 0.6 / max(w, 0.1);
        rel += vec3<f32>(sin(phase), 0.0, cos(phase * 1.0)) * amplitude;
        p.vitesse += vec3<f32>(cos(phase), 0.0, -sin(phase)) * 0.6;
    }

    // Boîte toroïdale centrée sur la caméra.
    let camera = view.world_position;
    p.position = camera + (fract((rel - camera) / u.boite + 0.5) - 0.5) * u.boite;

    let distance = length(camera - p.position);
    let demi = 0.5 * min(u.boite.x, u.boite.z);
    p.alpha = saturate(1.0 - distance / demi) * saturate(distance / 0.5) * u.luminosite;
    if (f32(i) >= u.actives || sous_abri(p.position)) {
        p.alpha = 0.0;
    }
    return p;
}

struct Sommet {
    @location(0) position: vec3<f32>,
};

struct VersFragment {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
    @location(2) @interpolate(flat) neige: f32,
};

@vertex
fn vertex(s: Sommet) -> VersFragment {
    var o: VersFragment;
    let coin = s.position.xy;          // x dans {-1, 1}, y dans {0, 1}
    let p = particule(u32(s.position.z));
    let vers_camera = normalize(view.world_position - p.position);

    var monde: vec3<f32>;
    if (p.neige) {
        // Flocon : petit carré face caméra (rond dans le fragment), légère traînée.
        let h = hachage(u32(s.position.z) ^ 0xB5297A4Du);
        let taille = mix(0.008, 0.02, hachage_f(h));
        let droite = normalize(cross(vec3<f32>(0.0, 1.0, 0.0), vers_camera));
        let haut = cross(vers_camera, droite);
        let q = vec2<f32>(coin.x, coin.y * 2.0 - 1.0);
        monde = p.position + (droite * q.x + haut * q.y) * taille - p.vitesse * u.trainee * 0.15 * coin.y;
    } else {
        // Goutte : trait étiré selon la vitesse (flou de mouvement simulé), tourné vers la caméra.
        let axe = p.vitesse * u.trainee;
        let cote = normalize(cross(normalize(axe + 1e-5), vers_camera)) * 0.006;
        monde = p.position + cote * coin.x - axe * coin.y;
    }

    o.clip = view.clip_from_world * vec4<f32>(monde, 1.0);
    if (p.alpha <= 0.0) {
        // Sommet dégénéré : le triangle est éliminé.
        o.clip = vec4<f32>(0.0, 0.0, -1.0, 1.0);
    }
    o.uv = coin;
    o.alpha = p.alpha;
    o.neige = select(0.0, 1.0, p.neige);
    return o;
}

@fragment
fn fragment(i: VersFragment) -> @location(0) vec4<f32> {
    let lumiere = u.couleur * view.exposure;
    if (i.neige > 0.5) {
        let r = length(vec2<f32>(i.uv.x, i.uv.y * 2.0 - 1.0));
        let flocon = 1.0 - smoothstep(0.4, 1.0, r);
        return vec4<f32>(lumiere * 1.6, i.alpha * flocon * 0.9);
    }
    let bord = 1.0 - abs(i.uv.x);
    return vec4<f32>(lumiere, i.alpha * bord * 0.35);
}
