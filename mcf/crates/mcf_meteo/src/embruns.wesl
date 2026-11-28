// Embruns des vagues qui cassent : une bouffée de gouttelettes par quad, face à la caméra.
// Sommets : position = centre de la bouffée, uv = coin du quad (−1..1),
// couleur = (âge normalisé, demi-largeur à la naissance, graine, force).

import bevy_pbr::render::mesh_view_bindings::view;

struct Uniformes {
    couleur: vec3<f32>,   // luminance des gouttelettes (linéaire, cd/m²)
    opacite: f32,
};
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> u: Uniformes;

struct Sommet {
    @location(0) position: vec3<f32>,
    @location(1) coin: vec2<f32>,
    @location(2) donnees: vec4<f32>,
};

struct VersFragment {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) @interpolate(flat) donnees: vec4<f32>,
    @location(2) @interpolate(flat) proche: f32,
};

@vertex
fn vertex(s: Sommet) -> VersFragment {
    var o: VersFragment;
    o.uv = s.coin;
    o.donnees = s.donnees;
    o.proche = 0.0;
    o.clip = vec4<f32>(0.0, 0.0, -1.0, 1.0);
    if (s.donnees.y <= 0.0) {
        // Quad inutilisé : dégénéré.
        return o;
    }
    let vers = view.world_position - s.position;
    let distance = length(vers);
    let vers_camera = vers / max(distance, 1e-4);
    var droite = cross(vec3<f32>(0.0, 1.0, 0.0), vers_camera);
    droite = select(vec3<f32>(1.0, 0.0, 0.0), normalize(droite), dot(droite, droite) > 1e-6);
    let haut = cross(vers_camera, droite);
    // La bouffée s'étale en vieillissant.
    let l = s.donnees.y * (1.0 + 2.6 * s.donnees.x);
    let monde = s.position + (droite * s.coin.x + haut * s.coin.y) * l;
    o.clip = view.clip_from_world * vec4<f32>(monde, 1.0);
    // Fondu quand la bouffée arrive sur la caméra (sinon elle remplit l'écran d'un coup).
    o.proche = saturate((distance - l * 0.8) / (l * 2.0 + 0.5));
    return o;
}

fn hachage2(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2<f32>(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

fn bruit(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let w = f * f * (3.0 - 2.0 * f);
    return mix(
        mix(hachage2(i), hachage2(i + vec2<f32>(1.0, 0.0)), w.x),
        mix(hachage2(i + vec2<f32>(0.0, 1.0)), hachage2(i + vec2<f32>(1.0, 1.0)), w.x),
        w.y,
    );
}

@fragment
fn fragment(i: VersFragment) -> @location(0) vec4<f32> {
    let age = i.donnees.x;
    let graine = i.donnees.z * 57.0;
    // Coordonnées bornées : avec le MSAA elles débordent un peu du quad.
    let uv = clamp(i.uv, vec2<f32>(-1.0), vec2<f32>(1.0));
    let r = length(uv);
    // Nuage : dense au centre, fondu vers le bord, contour irrégulier par un bruit à deux échelles.
    let forme = bruit(uv * 2.3 + graine) * 0.65 + bruit(uv * 6.1 + graine * 1.7) * 0.35;
    let rr = r / (0.55 + 0.45 * forme);
    let bord = exp(-rr * rr * 2.2) * (1.0 - smoothstep(0.75, 1.0, r));
    // Grain de gouttelettes, plus marqué quand la bouffée se disperse.
    let grain = mix(1.0, 0.55 + 0.9 * bruit(uv * 19.0 + graine * 3.1), 0.35 + 0.5 * age);
    // Apparition rapide, disparition lente.
    let vie = smoothstep(0.0, 0.08, age) * pow(1.0 - saturate(age), 1.3);
    let a = saturate(bord * grain * vie * i.donnees.w * u.opacite) * i.proche;
    // Prémultiplié : les gouttelettes éclairent beaucoup et cachent un peu.
    return vec4<f32>(u.couleur * view.exposure * a, a * 0.7);
}
