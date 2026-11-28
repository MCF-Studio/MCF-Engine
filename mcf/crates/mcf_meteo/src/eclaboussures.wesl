// Éclaboussures des gouttes de pluie : une petite couronne et quelques gouttelettes à chaque point d'impact
// calculé par `eclaboussures_calcul.wesl`. Maillage : un quad par éclaboussure, position = (coin.x, coin.y, indice).

import bevy_pbr::render::mesh_view_bindings::view;

struct Uniformes {
    couleur: vec3<f32>,   // éclairage des gouttes (linéaire, cd/m²)
    luminosite: f32,
    taille: f32,          // demi-largeur de la couronne à pleine ouverture (m)
    demi_largeur: f32,    // demi-côté de la zone autour de la caméra (m)
    actives: f32,         // nombre d'éclaboussures vivantes
    _remplissage: f32,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> u: Uniformes;
// xyz = point d'impact, w = âge normalisé (négatif : pas d'impact).
@group(constants::MATERIAL_BIND_GROUP) @binding(1) var impacts: texture_2d<f32>;

const PI: f32 = 3.14159265359;

fn hachage(x_in: u32) -> u32 {
    var x = x_in;
    x ^= x >> 16u; x *= 0x7feb352du;
    x ^= x >> 15u; x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}
fn hachage_f(x: u32) -> f32 { return f32(hachage(x) & 0x00FFFFFFu) / 16777216.0; }

struct Sommet {
    @location(0) position: vec3<f32>,
};

struct VersFragment {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) age: f32,
    @location(2) @interpolate(flat) alpha: f32,
    @location(3) @interpolate(flat) graine: u32,
    @location(4) @interpolate(flat) elevation: f32,
};

@vertex
fn vertex(s: Sommet) -> VersFragment {
    var o: VersFragment;
    let i = u32(s.position.z);
    let cote = u32(textureDimensions(impacts).x);
    let e = textureLoad(impacts, vec2<u32>(i % cote, i / cote), 0);
    o.clip = vec4<f32>(0.0, 0.0, -1.0, 1.0);
    o.uv = s.position.xy;
    o.age = e.w;
    o.alpha = 0.0;
    o.graine = hachage(i ^ 0xB5297A4Du);
    o.elevation = 0.0;
    if (e.w < 0.0 || f32(i) >= u.actives) {
        // Sommet dégénéré : le triangle est éliminé.
        return o;
    }
    let vers = view.world_position - e.xyz;
    let distance = length(vers);
    let vers_camera = vers / max(distance, 1e-4);
    var droite = cross(vec3<f32>(0.0, 1.0, 0.0), vers_camera);
    droite = select(vec3<f32>(1.0, 0.0, 0.0), normalize(droite), dot(droite, droite) > 1e-6);
    let haut = cross(vers_camera, droite);
    let l = u.taille * (0.7 + 0.6 * hachage_f(o.graine));
    // Légèrement vers la caméra, pour ne pas se battre avec la surface touchée.
    // Le quad descend de 0,2 sous le point d'impact : la moitié avant du rond à la surface y tient.
    let monde = e.xyz + (droite * s.position.x + haut * (s.position.y * 1.2 - 0.2)) * l + vers_camera * 0.02;
    o.elevation = abs(vers_camera.y);
    o.clip = view.clip_from_world * vec4<f32>(monde, 1.0);
    o.alpha = saturate(1.0 - distance / u.demi_largeur) * saturate(distance / 0.3) * u.luminosite;
    return o;
}

@fragment
fn fragment(i: VersFragment) -> @location(0) vec4<f32> {
    let t = i.age;
    let x = i.uv.x;                 // −1 → 1
    let h = i.uv.y * 1.2 - 0.2;     // hauteur au-dessus de la surface (le quad descend un peu dessous)
    let graine = hachage_f(i.graine) * 6.2831853;

    // Rond à la surface : une ellipse qui s'élargit, d'autant plus aplatie qu'on la regarde de côté.
    let aplati = max(i.elevation, 0.12);
    let rayon = 0.15 + 0.75 * t;
    let d = length(vec2<f32>(x, h / aplati));
    let rond = exp(-pow((d - rayon) / 0.07, 2.0)) * (1.0 - t);

    // Couronne : un voile d'eau qui s'évase, monte vite puis retombe (60 % de la vie). Vue de côté, ses bords
    // sont plus lumineux que sa face, et son sommet est dentelé.
    let tc = saturate(t / 0.6);
    let rc = 0.1 + 0.3 * tc;
    let hc = 0.6 * sin(PI * min(tc * 1.15, 1.0));
    var couronne = 0.0;
    if (hc > 0.01 && h > 0.0) {
        let r_y = rc * (0.7 + 0.5 * saturate(h / hc));
        let cote = abs(x) / r_y;
        let angle = asin(clamp(x / r_y, -1.0, 1.0));
        let sommet = hc * (0.72 + 0.28 * cos(angle * 5.0 + graine));
        let dedans = (1.0 - smoothstep(0.85, 1.0, cote)) * (1.0 - smoothstep(sommet * 0.75, sommet, h));
        couronne = dedans * (0.22 + 0.78 * pow(cote, 4.0)) * (1.0 - tc);
    }

    // Gouttelettes projetées depuis le bord de la couronne (trajectoires balistiques).
    var gouttes = 0.0;
    for (var k = 0u; k < 6u; k++) {
        let hk = hachage(i.graine + k * 0x9E3779B9u);
        let vx = (hachage_f(hk) * 2.0 - 1.0) * 1.2;
        let vy = 1.9 + 1.4 * hachage_f(hk ^ 0x85EBCA6Bu);
        let centre = vec2<f32>(vx * (0.1 + t), vy * t - 3.0 * t * t);
        let e = vec2<f32>(x, h) - centre;
        gouttes += exp(-dot(e, e) / 0.0016) * step(0.0, centre.y);
    }

    let eau = saturate(rond * 0.55 + couronne * 0.8 + gouttes * (1.0 - t * t));
    let lumiere = u.couleur * view.exposure * 1.3;
    return vec4<f32>(lumiere, eau * i.alpha);
}
