//! Mesh warps (interface 24): Transform's Puppet, the plugin's. The app hands the plugin what is
//! painted of what is being transformed; the plugin lays a mesh of triangles over it and hands it
//! back. The app keeps that mesh: it finds where touches and pins fall on it, draws it, and warps
//! the pixels through it on the GPU (and a vector layer's points through its triangles). As pins
//! are put, moved and turned, the app hands the plugin where each is on the mesh and where it has
//! been taken, and the plugin answers where every point of the mesh goes, and the turn about each
//! pin. Bending comes with every frame of a drag, so the plugin keeps what it can from one call to
//! the next (the mesh it laid last, and what it made ready for the pins as they are put).
//!
//! The plugin's code is a copy of its own, started when Transform asks for the warp and kept while
//! the app has it. It never sees pixels: what is painted comes as cells, each of one or more
//! pixels, one byte a cell, not 0 where any pixel in it is painted (at most [`MOST_CELLS`] of
//! them, bigger cells for a bigger drawing).
//!
//! Records here are those `plugin_layers.rs` writes: fields one after another, each a 16-bit id, a
//! 32-bit length and its bytes, little-endian. Numbers are little-endian too. Points are in source
//! pixels: canvas pixels where the content was when the transform began.
//!
//! What is painted (`procraft_warp_mesh(warp, len)`, `warp` by its place in the manifest's
//! `warps`): [`outline`]. The mesh it lays (its answer's length, at `procraft_warp_out()`; below 0
//! for none): [`mesh`]. The pins (`procraft_warp_bend(len)`): a [`pin`] field each. The mesh bent
//! (its answer's length; below 0 for none, and the mesh shows at rest): [`bent`].

use std::sync::{Arc, Mutex};

use procraft_engine::geom::{IRect, Vec2};
use procraft_engine::puppet::{Bends, Bent, Pin, PuppetMesh};

use crate::plugin_layers::{Record, fields};
use crate::plugin_run::Running;
use crate::plugins::Plugins;
use crate::{CanvasSession, State};

/// The fields of what is painted.
pub mod outline {
    /// How many cells across and down (u32 each).
    pub const COLS: u16 = 1;
    pub const ROWS: u16 = 2;
    /// How many canvas pixels a cell is on a side (f32).
    pub const SCALE: u16 = 3;
    /// Where the first cell's top left is, x and y (f32 each).
    pub const ORIGIN: u16 = 4;
    /// The cells, a byte each, row by row from the top: not 0 where anything is painted.
    pub const CELLS: u16 = 5;
    /// How far past what is painted the mesh reaches (canvas pixels, f32), and about how many
    /// triangles across its longer side (u32): Transform's settings for it.
    pub const EXPANSION: u16 = 6;
    pub const DENSITY: u16 = 7;
}

/// The fields of a mesh laid.
pub mod mesh {
    /// Its points at rest, x and y by turns (f32 each).
    pub const POINTS: u16 = 1;
    /// Its triangles, three points each by their places among them (u32 each).
    pub const TRIANGLES: u16 = 2;
    /// About how wide its triangles are (canvas pixels, f32).
    pub const SPACING: u16 = 3;
}

/// The field of a pin: the triangle it is on (u32) and the weights of its corners that make it
/// there, then where it has been taken, x and y, and the turn it holds the mesh about it at
/// (radians, clockwise on the screen; not a number for none: the mesh turns there as it bends),
/// f32 each.
pub const PIN: u16 = 1;

/// The fields of the mesh bent.
pub mod bent {
    /// Where each of its points goes, x and y by turns (f32 each).
    pub const POINTS: u16 = 1;
    /// The turn about each pin, as the pins came (f32 each).
    pub const ANGLES: u16 = 2;
}

/// The most cells what is painted is handed over in.
pub const MOST_CELLS: usize = 1 << 18;

fn f32s(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}

fn f32s_of(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

/// What is painted in `alpha` (one byte a pixel over `area`), as cells: their record, without
/// Transform's settings; none if nothing is painted.
fn cells_of(area: IRect, alpha: &[u8]) -> Option<Record> {
    let width = area.width().max(1) as usize;
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (y, row) in alpha.chunks_exact(width).enumerate() {
        let Some(first) = row.iter().position(|&a| a != 0) else { continue };
        let last = row.iter().rposition(|&a| a != 0).unwrap_or(first);
        (x0, x1, y0, y1) = (x0.min(first), x1.max(last + 1), y0.min(y), y + 1);
    }
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let (w, h) = (x1 - x0, y1 - y0);
    let scale = ((w * h) as f64 / MOST_CELLS as f64).sqrt().ceil().max(1.0) as usize;
    let (cols, rows) = (w.div_ceil(scale), h.div_ceil(scale));
    let mut cells = vec![0_u8; cols * rows];
    for (y, row) in alpha.chunks_exact(width).enumerate().take(y1).skip(y0) {
        let line = (y - y0) / scale * cols;
        for (x, _) in row.iter().enumerate().take(x1).skip(x0).filter(|(_, a)| **a != 0) {
            if let Some(cell) = cells.get_mut(line + (x - x0) / scale) {
                *cell = 1;
            }
        }
    }
    let origin = [area.x0 as f32 + x0 as f32, area.y0 as f32 + y0 as f32];
    let mut record = Record::default();
    record
        .field(outline::COLS, &(cols as u32).to_le_bytes())
        .field(outline::ROWS, &(rows as u32).to_le_bytes())
        .field(outline::SCALE, &(scale as f32).to_le_bytes())
        .field(outline::ORIGIN, &f32s(origin))
        .field(outline::CELLS, &cells);
    Some(record)
}

/// The mesh a plugin handed back, if it holds up.
fn mesh_of(bytes: &[u8]) -> Option<PuppetMesh> {
    let (mut points, mut tris, mut spacing) = (Vec::new(), Vec::new(), 0.0);
    for (id, value) in fields(bytes) {
        match id {
            mesh::POINTS => {
                points = f32s_of(value).as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
            }
            mesh::TRIANGLES => {
                let corners = value.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b));
                tris = corners.collect::<Vec<u32>>().as_chunks::<3>().0.to_vec();
            }
            mesh::SPACING => spacing = f32s_of(value).first().copied().unwrap_or(0.0),
            _ => {}
        }
    }
    PuppetMesh::new(points, tris, spacing)
}

/// A plugin's mesh warp as Transform has it: a copy of its plugin's code, which keeps the mesh it
/// laid last, and its place among its plugin's warps. If its code stops, nothing more is asked of
/// it, and its plugin goes off.
pub(crate) struct MeshWarp {
    /// `<id>.<key>`, as [`Plugins::warps`] lists it.
    pub key: String,
    index: u32,
    plugin: String,
    plugins: Arc<Plugins>,
    running: Mutex<Option<Running>>,
}

impl MeshWarp {
    /// Whether its code hasn't stopped.
    fn running(&self) -> bool {
        self.running.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// The answer to `call` on its code, if it hasn't stopped; stopped now, it never runs again.
    fn ask(
        &self,
        call: impl FnOnce(&mut Running) -> Result<Option<Vec<u8>>, crate::plugin_run::Stop>,
    ) -> Option<Vec<u8>> {
        let mut running = self.running.lock().unwrap_or_else(|e| e.into_inner());
        match call(running.as_mut()?) {
            Ok(answer) => answer,
            Err(stop) => {
                *running = None;
                self.plugins.part_stopped(&self.plugin, stop);
                None
            }
        }
    }

    /// The mesh its plugin lays over what is painted in `alpha` (one byte a pixel over `area`),
    /// reaching `expansion` past it, about `density` triangles across; none if nothing is painted
    /// or it lays none.
    pub(crate) fn lay(&self, area: IRect, alpha: &[u8], expansion: f32, density: u32) -> Option<PuppetMesh> {
        let mut record = cells_of(area, alpha)?;
        record.field(outline::EXPANSION, &expansion.to_le_bytes()).field(outline::DENSITY, &density.to_le_bytes());
        let index = self.index;
        mesh_of(&self.ask(|running| running.warp_mesh(index, &record.bytes()))?)
    }
}

impl std::fmt::Debug for MeshWarp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MeshWarp").field("key", &self.key).finish_non_exhaustive()
    }
}

impl Bends for MeshWarp {
    fn bend(&self, mesh: &PuppetMesh, pins: &[Pin]) -> Option<Bent> {
        let mut record = Record::default();
        for pin in pins {
            let [w0, w1, w2] = pin.weights;
            let mut bytes = pin.tri.to_le_bytes().to_vec();
            bytes.extend(f32s([w0, w1, w2, pin.at.x, pin.at.y, pin.angle.unwrap_or(f32::NAN)]));
            record.field(PIN, &bytes);
        }
        let answer = self.ask(|running| running.warp_bend(&record.bytes()))?;
        let (mut verts, mut angles) = (Vec::new(), Vec::new());
        for (id, value) in fields(&answer) {
            match id {
                bent::POINTS => {
                    verts = f32s_of(value).as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
                }
                bent::ANGLES => angles = f32s_of(value),
                _ => {}
            }
        }
        // every point of the mesh, a number, and an angle for each pin
        let fits = verts.len() == mesh.rest().len()
            && angles.len() == pins.len()
            && verts.iter().all(|v| v.is_finite())
            && angles.iter().all(|a| a.is_finite());
        fits.then_some(Bent { verts, angles })
    }
}

impl State {
    /// The mesh warp Transform's Puppet uses, if it has one.
    pub(crate) fn mesh_warp(&self) -> Option<Arc<MeshWarp>> {
        self.mesh_warp.clone()
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Transform's Puppet lays and bends its mesh by mesh warp `key` of `plugins` (as
    /// [`Plugins::warps`] lists it) from now on; none, or one that can't start, leaves Puppet with
    /// no mesh to lay. An open puppet laid by another is laid again. Whether it has one now.
    pub fn transform_mesh_warp(&self, plugins: Arc<Plugins>, key: Option<String>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        // the one it has, unless its code stopped
        if st.mesh_warp.as_ref().filter(|w| w.running()).map(|w| &w.key) == key.as_ref() && key.is_some() {
            return true;
        }
        st.mesh_warp = key.and_then(|key| {
            let code = plugins.warp_to_run(&key).ok()?;
            let running = Mutex::new(Some(code.running));
            Some(Arc::new(MeshWarp { key, index: code.index, plugin: code.plugin, plugins, running }))
        });
        if st.transform.as_ref().is_some_and(procraft_engine::transform::Transform::has_puppet) {
            st.lay_puppet();
            st.show_transform();
            self.shared.frame(st);
        }
        st.mesh_warp.is_some()
    }
}

/// The app's plugins in a folder of their own, plugin `id` on.
#[cfg(test)]
pub(crate) fn plugins_on(id: &str) -> Arc<Plugins> {
    use std::sync::atomic::{AtomicU32, Ordering};
    static MADE: AtomicU32 = AtomicU32::new(0);
    let n = MADE.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("procraft-plugins-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let plugins = Plugins::with_bundled(dir.to_string_lossy().into_owned());
    assert!(plugins.set_on(id.to_owned(), true), "{id}");
    plugins
}

/// The Puppet Warp plugin, and its warp.
#[cfg(test)]
pub(crate) const PUPPET: &str = "io.github.artcraftandroid.puppet-warp";

/// Transform's Puppet in `s` by the Puppet Warp plugin.
#[cfg(test)]
pub(crate) fn puppet_on(s: &CanvasSession) {
    assert!(s.transform_mesh_warp(plugins_on(PUPPET), Some(format!("{PUPPET}.puppet"))));
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// The Puppet Warp plugin's warp, started.
    fn puppet() -> MeshWarp {
        let plugins = plugins_on(PUPPET);
        let key = format!("{PUPPET}.puppet");
        assert_eq!(plugins.warps().iter().map(|w| w.label.as_str()).collect::<Vec<_>>(), ["Puppet"]);
        let code = plugins.warp_to_run(&key).unwrap();
        MeshWarp { key, index: code.index, plugin: code.plugin, plugins, running: Mutex::new(Some(code.running)) }
    }

    /// An alpha `w` × `h` with the rectangle `x0`..`x1`, `y0`..`y1` painted.
    fn bar(w: i32, h: i32, [x0, y0, x1, y1]: [i32; 4]) -> (IRect, Vec<u8>) {
        let mut alpha = vec![0_u8; (w * h) as usize];
        for y in y0..y1 {
            for x in x0..x1 {
                alpha[(y * w + x) as usize] = 255;
            }
        }
        (IRect::new(0, 0, w, h), alpha)
    }

    #[test]
    fn the_puppet_plugin_lays_a_mesh_over_what_is_painted_and_bends_it_by_its_pins() {
        let warp = puppet();
        let (area, alpha) = bar(240, 80, [20, 20, 220, 60]);
        let mesh = warp.lay(area, &alpha, 0.0, 40).unwrap();
        // over every painted pixel, and no further past them than simplifying the outline may take
        // it (a tenth of the spacing) and a pixel and a half
        for (x, y) in [(20.5, 20.5), (219.5, 59.5), (120.0, 40.0)] {
            assert!(mesh.locate(Vec2::new(x, y)).is_some(), "{x} {y}");
        }
        let reach = 0.1 * 5.0 + 1.5;
        let within = |v: f32, lo: f32, hi: f32| (lo - reach..=hi + reach).contains(&v);
        assert!(mesh.rest().iter().all(|v| within(v.x, 20.0, 220.0) && within(v.y, 20.0, 60.0)));
        assert!(warp.lay(area, &vec![0; alpha.len()], 0.0, 40).is_none());
        // two pins, the far one taken down: each holds where it is taken, the near end stays
        let mut pins: Vec<Pin> = [Vec2::new(30.0, 40.0), Vec2::new(210.0, 40.0)]
            .iter()
            .map(|&p| Pin::new(&mesh, p, None).unwrap())
            .collect();
        pins[1].at = pins[1].at + Vec2::new(-20.0, 60.0);
        let bent = warp.bend(&mesh, &pins).unwrap();
        for pin in &pins {
            assert!(mesh.at(&bent.verts, pin.tri, pin.weights).distance(pin.at) < 0.1);
        }
        let (corner, _) = mesh.locate(Vec2::new(20.5, 20.5)).unwrap();
        let near = mesh.triangles()[corner as usize][0] as usize;
        assert!(bent.verts[near].distance(mesh.rest()[near]) < 8.0, "{:?}", bent.verts[near]);
        assert!(bent.angles[1] > 0.2, "{:?}", bent.angles);
        // a pin on no triangle of the mesh bends nothing
        let off = Pin { tri: u32::MAX, ..pins[0] };
        assert!(warp.bend(&mesh, &[off]).is_none());
    }

    /// What laying a mesh and bending it costs under the interpreter: a drawing 2000 across, the
    /// mesh at its densest and at Transform's first setting, three pins, one dragged.
    #[test]
    #[ignore = "timing: cargo test --release -p procraft-core plugin_warps::tests::timing -- --ignored --nocapture"]
    fn timing() {
        let warp = puppet();
        let (area, alpha) = bar(2000, 2000, [100, 100, 1900, 1900]);
        for density in [40, 80] {
            let started = Instant::now();
            let mesh = warp.lay(area, &alpha, 4.0, density).unwrap();
            let laid = started.elapsed();
            let mut pins: Vec<Pin> = [Vec2::new(150.0, 150.0), Vec2::new(1850.0, 1850.0), Vec2::new(1000.0, 1000.0)]
                .iter()
                .map(|&p| Pin::new(&mesh, p, None).unwrap())
                .collect();
            let started = Instant::now();
            warp.bend(&mesh, &pins).unwrap();
            let readied = started.elapsed();
            let mut each = Vec::new();
            for k in 0..10 {
                pins[1].at = pins[1].at + Vec2::new(-20.0, 5.0 * k as f32);
                let started = Instant::now();
                warp.bend(&mesh, &pins).unwrap();
                each.push(started.elapsed());
            }
            each.sort();
            println!(
                "density {density}: {} points, {} triangles; laid in {laid:?}; pins put (made ready and bent) in {readied:?}; each drag frame {:?} (median of 10)",
                mesh.rest().len(),
                mesh.triangles().len(),
                each[each.len() / 2]
            );
        }
    }

    #[test]
    fn what_is_painted_goes_in_cells_no_more_than_the_most() {
        // a small drawing, a pixel a cell, from its painted corner
        let area = IRect::new(10, 20, 20, 30);
        let mut alpha = vec![0_u8; 100];
        alpha[3 * 10 + 4] = 255;
        alpha[5 * 10 + 7] = 1;
        let record = cells_of(area, &alpha).unwrap().bytes();
        let got: Vec<(u16, Vec<u8>)> = fields(&record).map(|(id, v)| (id, v.to_vec())).collect();
        let field = |id: u16| got.iter().find(|(i, _)| *i == id).map(|(_, v)| v.clone()).unwrap();
        assert_eq!(field(outline::COLS), 4_u32.to_le_bytes());
        assert_eq!(field(outline::ROWS), 3_u32.to_le_bytes());
        assert_eq!(field(outline::SCALE), 1.0_f32.to_le_bytes());
        assert_eq!(f32s_of(&field(outline::ORIGIN)), [14.0, 23.0]);
        assert_eq!(field(outline::CELLS), [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        assert!(cells_of(area, &[0; 100]).is_none());
        // a big one in bigger cells, every painted pixel in a painted cell
        let side = 3000;
        let area = IRect::new(0, 0, side, side);
        let mut alpha = vec![0_u8; (side * side) as usize];
        alpha[1] = 9;
        alpha[(side * side - 1) as usize] = 9;
        let record = cells_of(area, &alpha).unwrap().bytes();
        let cells = fields(&record).find(|(id, _)| *id == outline::CELLS).unwrap().1.to_vec();
        assert!(cells.len() <= MOST_CELLS);
        assert_eq!((cells[0], cells[cells.len() - 1]), (1, 1));
        assert_eq!(cells.iter().filter(|c| **c != 0).count(), 2);
    }
}
