//! Puppet Warp: Transform's Puppet. The app hands over what is painted; this lays a mesh of
//! triangles over it following its outline, and bends that mesh by the pins put on it, as rigidly
//! as it can. The app keeps the mesh it is given, finds where touches and pins fall on it, and
//! warps the pixels through it on the GPU. `tools/pack_plugin.py` makes a `.procraft-plugin` file
//! of it.
//!
//! The interface is written down at the top of `crates/core/src/plugin_warps.rs`.

mod geom;
mod mesh;
mod puppet;

use std::sync::Mutex;

use kit::layer::{Record, fields};

use crate::geom::Vec2;
use crate::puppet::{MeshSettings, Painted, Pin, PuppetMesh, Solver};

/// Where records come in and go out.
static IN: kit::Place = kit::Place::new();
static OUT: kit::Place = kit::Place::new();

/// The mesh last laid, and what was made ready to bend it for the pins last asked about.
static LAID: Mutex<Option<(PuppetMesh, Option<Solver>)>> = Mutex::new(None);

/// The warps, by their place in the manifest.
const PUPPET: i32 = 0;

/// What is painted, as the app hands it over.
mod outline {
    pub const COLS: u16 = 1;
    pub const ROWS: u16 = 2;
    pub const SCALE: u16 = 3;
    pub const ORIGIN: u16 = 4;
    pub const CELLS: u16 = 5;
    pub const EXPANSION: u16 = 6;
    pub const DENSITY: u16 = 7;
}

/// The mesh handed back, and the mesh bent.
mod mesh_out {
    pub const POINTS: u16 = 1;
    pub const TRIANGLES: u16 = 2;
    pub const SPACING: u16 = 3;
    pub const ANGLES: u16 = 2;
}

/// A pin, as the app hands it over.
const PIN: u16 = 1;

fn u32_of(bytes: &[u8]) -> u32 {
    bytes.get(..4).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes)
}

fn f32_of(bytes: &[u8]) -> f32 {
    bytes.get(..4).and_then(|b| b.try_into().ok()).map_or(f32::NAN, f32::from_le_bytes)
}

fn f32s(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The mesh over the outline `bytes` tell, as a record: none when nothing is painted.
fn lay(bytes: &[u8]) -> Option<(PuppetMesh, Vec<u8>)> {
    let (mut cols, mut rows, mut scale, mut origin) = (0, 0, 1.0, Vec2::ZERO);
    let mut cells: &[u8] = &[];
    let mut settings = MeshSettings::default();
    for (id, value) in fields(bytes) {
        match id {
            outline::COLS => cols = u32_of(value) as usize,
            outline::ROWS => rows = u32_of(value) as usize,
            outline::SCALE => scale = f32_of(value),
            outline::ORIGIN => origin = Vec2::new(f32_of(value), f32_of(value.get(4..).unwrap_or_default())),
            outline::CELLS => cells = value,
            outline::EXPANSION => settings.expansion = f32_of(value),
            outline::DENSITY => settings.density = u32_of(value),
            _ => {}
        }
    }
    let mesh = PuppetMesh::build(Painted { cells, cols, rows, scale, origin }, settings)?;
    let mut out = Record::default();
    out.field(mesh_out::POINTS, &f32s(mesh.rest().iter().flat_map(|v| [v.x, v.y])))
        .field(
            mesh_out::TRIANGLES,
            &mesh.triangles().iter().flatten().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>(),
        )
        .field(mesh_out::SPACING, &mesh.spacing().to_le_bytes());
    Some((mesh, out.bytes()))
}

/// The pins `bytes` tell, each on `mesh`; none if one isn't.
fn pins(mesh: &PuppetMesh, bytes: &[u8]) -> Option<Vec<Pin>> {
    fields(bytes)
        .filter(|(id, _)| *id == PIN)
        .map(|(_, v)| {
            let n: Vec<f32> = v.chunks_exact(4).skip(1).map(f32_of).collect();
            let [w0, w1, w2, x, y, angle] = n[..] else { return None };
            let tri = u32_of(v);
            let pin =
                Pin { tri, weights: [w0, w1, w2], at: Vec2::new(x, y), angle: angle.is_finite().then_some(angle) };
            (mesh.has_triangle(tri) && pin.at.is_finite() && pin.weights.iter().all(|w| w.is_finite())).then_some(pin)
        })
        .collect()
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_warp_in(len: i32) -> i32 {
    IN.make(len)
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_warp_out() -> i32 {
    OUT.with(|bytes| bytes.as_mut_ptr() as usize as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_warp_mesh(warp: i32, len: i32) -> i32 {
    let bytes = IN.take();
    let made = match warp {
        PUPPET => lay(bytes.get(..len.max(0) as usize).unwrap_or_default()),
        _ => None,
    };
    let mut laid = lock(&LAID);
    let Some((mesh, out)) = made else {
        *laid = None;
        return -1;
    };
    *laid = Some((mesh, None));
    let len = out.len() as i32;
    OUT.put(out);
    len
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_warp_bend(len: i32) -> i32 {
    let bytes = IN.take();
    let mut laid = lock(&LAID);
    let Some((mesh, solver)) = laid.as_mut() else { return -1 };
    let Some(pins) = pins(mesh, bytes.get(..len.max(0) as usize).unwrap_or_default()) else { return -1 };
    // made ready again only when the pins are put elsewhere on the mesh, or hold angles otherwise
    if !solver.as_ref().is_some_and(|s| s.fits(&pins)) {
        *solver = Some(Solver::new(mesh, &pins));
    }
    let Some(solver) = solver.as_ref() else { return -1 };
    let bent = solver.solve(mesh, &pins);
    let mut out = Record::default();
    out.field(mesh_out::POINTS, &f32s(bent.verts.iter().flat_map(|v| [v.x, v.y])))
        .field(mesh_out::ANGLES, &f32s(bent.angles.iter().copied()));
    let out = out.bytes();
    let len = out.len() as i32;
    OUT.put(out);
    len
}
