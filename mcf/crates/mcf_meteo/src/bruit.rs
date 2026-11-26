//! Hachage et bruit de gradient 3D (Perlin), identiques à `Common.hlsli` de SkyForge pour que le CPU et le GPU
//! (version WGSL à venir) donnent les mêmes valeurs.

use bevy::math::{IVec3, Vec3};

/// Hachage entier 32 bits (« lowbias32 »).
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// Hachage d'une cellule 3D.
pub fn hash3(v: [u32; 3]) -> u32 {
    hash(v[0] ^ hash(v[1] ^ hash(v[2])))
}

/// Nombre pseudo-aléatoire dans `[0, 1)` (24 bits de mantisse).
pub fn hash_f(x: u32) -> f32 {
    (hash(x) & 0x00FF_FFFF) as f32 / 16_777_216.0
}

/// Trois nombres pseudo-aléatoires dans `[0, 1)` pour une cellule.
pub fn hash33(p: IVec3) -> Vec3 {
    // Même réinterprétation que `uint3(int3)` en HLSL / WGSL.
    let h = hash3([p.x as u32, p.y as u32, p.z as u32]);
    Vec3::new(hash_f(h), hash_f(h ^ 0x9E37_79B9), hash_f(h ^ 0x85EB_CA6B))
}

fn fondu(t: Vec3) -> Vec3 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn produit_gradient(cellule: IVec3, periode: i32, f: Vec3) -> f32 {
    let c = if periode > 0 {
        cellule.rem_euclid(IVec3::splat(periode))
    } else {
        cellule
    };
    let g = (hash33(c) * 2.0 - 1.0 + 1e-4).normalize();
    g.dot(f)
}

/// Bruit de Perlin 3D, environ dans `[-1, 1]`. `periode > 0` rend le bruit périodique (en cellules) sur les trois
/// axes ; `0` = non périodique.
pub fn perlin3(p: Vec3, periode: i32) -> f32 {
    let base = p.floor();
    let i = base.as_ivec3();
    let f = p - base;
    let u = fondu(f);
    let n = |d: IVec3| produit_gradient(i + d, periode, f - d.as_vec3());
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = lerp(n(IVec3::new(0, 0, 0)), n(IVec3::new(1, 0, 0)), u.x);
    let x10 = lerp(n(IVec3::new(0, 1, 0)), n(IVec3::new(1, 1, 0)), u.x);
    let x01 = lerp(n(IVec3::new(0, 0, 1)), n(IVec3::new(1, 0, 1)), u.x);
    let x11 = lerp(n(IVec3::new(0, 1, 1)), n(IVec3::new(1, 1, 1)), u.x);
    lerp(lerp(x00, x10, u.y), lerp(x01, x11, u.y), u.z)
}
