//! The asset library: reusable materials, each a little document of layers, kept in files of their
//! own and shared by every artwork. Libraries hold folders; folders hold assets. An asset is an
//! artwork folder (artwork.json + layers + thumbnail) in a shared items pool the folders name by id,
//! so the same asset can sit in several folders without being copied. A small index keeps the
//! libraries, their folders and each asset's card; the pixels live in the items pool. Writes are
//! whole-or-not-at-all, so cut short anywhere the library is still there.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use procraft_engine::document::{BlendMode, Document, Layer, LayerId, LayerKind};
use procraft_engine::zip::{ZipReader, ZipWriter};

use crate::{CanvasSession, State, ThumbnailImage};

/// The index of the libraries, in the assets' folder.
const INDEX_FILE: &str = "libraries.json";
/// The shared pool of asset documents.
const ITEMS: &str = "items";
/// The first library's id.
const FIRST: &str = "main";
/// The first folder's id in a new library.
const GENERAL: &str = "general";
const STOCK_NAME: &str = "Assets";
const NEW_LIBRARY: &str = "Untitled library";
const NEW_FOLDER: &str = "New folder";
const NEW_ASSET: &str = "Untitled asset";
/// The manifest a shared asset file carries.
pub(crate) const SHARE_MANIFEST: &str = "assets.json";
/// What a shared asset file's manifest says it is.
const SHARE_FORMAT: &str = "procraft-assets";

/// A library as the panel shows it: its name, whether it is the one in use, and its folders.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AssetLibraryInfo {
    pub name: String,
    pub current: bool,
    pub folders: Vec<AssetFolderInfo>,
}

/// A folder as the panel shows it: its name and the assets in it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AssetFolderInfo {
    pub name: String,
    pub assets: Vec<AssetInfo>,
}

/// An asset as its card shows it.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct AssetInfo {
    pub id: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
}

/// An asset's card in the index: enough to show it without reading its document.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct AssetCard {
    id: String,
    name: String,
    width: u32,
    height: u32,
}

impl AssetCard {
    fn info(&self) -> AssetInfo {
        AssetInfo { id: self.id.clone(), name: self.name.clone(), width: self.width, height: self.height }
    }
}

/// A folder in the index: its id, name and the cards of the assets in it, in order.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Folder {
    id: String,
    name: String,
    #[serde(default)]
    assets: Vec<AssetCard>,
}

/// A library in the index: its id, name and folders.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Library {
    id: String,
    name: String,
    #[serde(default)]
    folders: Vec<Folder>,
}

impl Library {
    fn first() -> Library {
        Library {
            id: FIRST.to_owned(),
            name: STOCK_NAME.to_owned(),
            folders: vec![Folder { id: GENERAL.to_owned(), name: "General".to_owned(), assets: Vec::new() }],
        }
    }
}

/// The index as kept.
#[derive(serde::Serialize)]
struct IndexFile<'a> {
    libraries: &'a [Library],
    current: &'a str,
}

/// The libraries: in order, the one in use (by id), and the folder they are kept in.
#[derive(Clone, Debug)]
pub(crate) struct Assets {
    list: Vec<Library>,
    current: String,
    dir: Option<PathBuf>,
}

impl Default for Assets {
    fn default() -> Assets {
        Assets { list: vec![Library::first()], current: FIRST.to_owned(), dir: None }
    }
}

impl Assets {
    fn place(&self, id: &str) -> Option<usize> {
        self.list.iter().position(|l| l.id == id)
    }
}

/// A name no other in `taken` has: numbered after it ("Name 2", "Name 3"...) while one has it.
fn unique_name(taken: &[String], name: &str) -> String {
    if !taken.iter().any(|n| n == name) {
        return name.to_owned();
    }
    let (base, from) = match name.rsplit_once(' ').map(|(b, n)| (b, n.parse::<u32>())) {
        Some((base, Ok(n))) if n >= 2 => (base, n),
        _ => (name, 1),
    };
    (from + 1..)
        .map(|n| format!("{base} {n}"))
        .find(|n| !taken.iter().any(|t| t == n))
        .unwrap_or_else(|| name.to_owned())
}

/// An id none has: 128 random bits written as a UUID (as the gallery's folder names are).
fn fresh_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let half = |k: u8| {
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(now);
        h.write_u8(k);
        h.finish()
    };
    let (a, b) = (half(0), half(1));
    let a = (a & !0xF000) | 0x4000;
    let b = (b & !(0xC << 60)) | (0x8 << 60);
    format!(
        "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
        a >> 32,
        (a >> 16) & 0xFFFF,
        a & 0xFFFF,
        b >> 48,
        b & 0xFFFF_FFFF_FFFF
    )
}

/// Whether `id` is one a library's files can have.
fn is_library_id(id: &str) -> bool {
    id == FIRST || id.strip_prefix("lib-").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// A library id none of `list` has and no folder in `dir` is kept by.
fn new_library_id(list: &[Library], dir: &Path) -> String {
    (1u32..)
        .map(|n| format!("lib-{n}"))
        .find(|id| list.iter().all(|l| l.id != *id) && !dir.join(id).exists())
        .unwrap_or_default()
}

/// The item folder of asset `id` in `dir`.
fn item_dir(dir: &Path, id: &str) -> PathBuf {
    dir.join(ITEMS).join(id)
}

/// `text` kept in the file at `path`, replaced whole; false if it couldn't be.
fn write(path: &Path, text: &str) -> bool {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let written = crate::project::write_atomically(path, |p| {
        std::fs::write(p, text.as_bytes()).map_err(|e| crate::project::file_error("keeping the assets", e))
    });
    if let Err(e) = &written {
        log::error!("{e}");
    }
    written.is_ok()
}

/// The index kept in `dir`, as much of it as can be read; none if there is none or it isn't one.
fn read_index(dir: &Path) -> Option<(Vec<Library>, String)> {
    let text = std::fs::read_to_string(dir.join(INDEX_FILE)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let entries = value.get("libraries")?.as_array()?;
    let mut list: Vec<Library> = Vec::new();
    for entry in entries {
        let Ok(mut read) = serde_json::from_value::<Library>(entry.clone()) else { continue };
        if !is_library_id(&read.id) || list.iter().any(|l| l.id == read.id) {
            continue;
        }
        read.name = read.name.trim().chars().take(256).collect();
        if read.name.is_empty() {
            read.name = NEW_LIBRARY.to_owned();
        }
        if read.folders.is_empty() {
            read.folders.push(Folder { id: GENERAL.to_owned(), name: "General".to_owned(), assets: Vec::new() });
        }
        list.push(read);
    }
    if list.is_empty() {
        return None;
    }
    let current = value.get("current").and_then(serde_json::Value::as_str).unwrap_or(FIRST).to_owned();
    let current = if list.iter().any(|l| l.id == current) {
        current
    } else {
        list.first().map_or_else(|| FIRST.to_owned(), |l| l.id.clone())
    };
    Some((list, current))
}

/// The libraries kept in `dir`: as the index has them, else a first library.
fn libraries_in(dir: &Path) -> (Vec<Library>, String) {
    read_index(dir).unwrap_or_else(|| (vec![Library::first()], FIRST.to_owned()))
}

/// Writes serialised, so two threads' changes land in the order they read them.
static ASSET_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn asset_writes() -> std::sync::MutexGuard<'static, ()> {
    ASSET_WRITES.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[uniffi::export]
impl CanvasSession {
    /// Opens the asset library kept in `dir`, for every artwork to share. True if it could be read.
    pub fn open_assets(&self, dir: String) -> bool {
        let dir = PathBuf::from(dir);
        let (list, current) = libraries_in(&dir);
        let mut st = self.shared.lock();
        st.assets = Assets { list, current, dir: Some(dir) };
        true
    }

    /// Every asset library in order, the one in use among them.
    pub fn asset_libraries(&self) -> Vec<AssetLibraryInfo> {
        let st = self.shared.lock();
        st.assets
            .list
            .iter()
            .map(|l| AssetLibraryInfo {
                name: l.name.clone(),
                current: l.id == st.assets.current,
                folders: l
                    .folders
                    .iter()
                    .map(|f| AssetFolderInfo {
                        name: f.name.clone(),
                        assets: f.assets.iter().map(AssetCard::info).collect(),
                    })
                    .collect(),
            })
            .collect()
    }

    /// Brings the library at `index` into use. False if there is none there.
    pub fn open_asset_library(&self, index: u32) -> bool {
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        let Some(id) = st.assets.list.get(index as usize).map(|l| l.id.clone()) else { return false };
        st.assets.current = id.clone();
        let dir = st.assets.dir.clone();
        let list = st.assets.list.clone();
        drop(st);
        dir.is_none_or(|dir| write(&dir.join(INDEX_FILE), &index_json(&list, &id)))
    }

    /// A new, empty library after the others; its place, or none if it couldn't be kept.
    pub fn new_asset_library(&self) -> Option<u32> {
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        let dir = st.assets.dir.clone()?;
        let id = new_library_id(&st.assets.list, &dir);
        let names: Vec<String> = st.assets.list.iter().map(|l| l.name.clone()).collect();
        let library = Library {
            id: id.clone(),
            name: unique_name(&names, NEW_LIBRARY),
            folders: vec![Folder { id: fresh_id(), name: "General".to_owned(), assets: Vec::new() }],
        };
        st.assets.list.push(library);
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        if !write(&dir.join(INDEX_FILE), &index_json(&list, &current)) {
            return None;
        }
        self.shared.lock().assets.place(&id).map(|p| p as u32)
    }

    /// Renames the library at `index`; false if there is none there or the name is blank.
    pub fn rename_asset_library(&self, index: u32, name: String) -> bool {
        let name: String = name.trim().chars().take(256).collect();
        if name.is_empty() {
            return false;
        }
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        let Some(library) = st.assets.list.get_mut(index as usize) else { return false };
        library.name = name;
        let dir = st.assets.dir.clone();
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        dir.is_none_or(|dir| write(&dir.join(INDEX_FILE), &index_json(&list, &current)))
    }

    /// Deletes the library at `index` (the one in use: the next, else the one before, comes into
    /// use). False if it is the only one, there is none there, or it couldn't be kept. Its assets'
    /// documents go once no other library names them.
    pub fn delete_asset_library(&self, index: u32) -> bool {
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        let index = index as usize;
        if st.assets.list.len() < 2 {
            return false;
        }
        let Some(gone) = st.assets.list.get(index).map(|l| l.id.clone()) else { return false };
        let next = if index + 1 < st.assets.list.len() { index + 1 } else { index.saturating_sub(1) };
        let current = if st.assets.current == gone {
            st.assets.list.get(next).map(|l| l.id.clone()).unwrap_or_else(|| FIRST.to_owned())
        } else {
            st.assets.current.clone()
        };
        st.assets.list.remove(index);
        st.assets.current = current.clone();
        let dir = st.assets.dir.clone();
        let list = st.assets.list.clone();
        drop(st);
        let Some(dir) = dir else { return false };
        if !write(&dir.join(INDEX_FILE), &index_json(&list, &current)) {
            return false;
        }
        forget_unnamed(&dir, &list);
        true
    }

    /// A new folder in the library at `index`; its place in that library, or none.
    pub fn new_asset_folder(&self, index: u32) -> Option<u32> {
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        let dir = st.assets.dir.clone()?;
        let library = st.assets.list.get_mut(index as usize)?;
        let names: Vec<String> = library.folders.iter().map(|f| f.name.clone()).collect();
        let folder = Folder { id: fresh_id(), name: unique_name(&names, NEW_FOLDER), assets: Vec::new() };
        library.folders.push(folder);
        let at = (library.folders.len() - 1) as u32;
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        write(&dir.join(INDEX_FILE), &index_json(&list, &current)).then_some(at)
    }

    /// Renames the folder at `folder` in the library at `index`; false if there is none or the name
    /// is blank.
    pub fn rename_asset_folder(&self, index: u32, folder: u32, name: String) -> bool {
        let name: String = name.trim().chars().take(256).collect();
        if name.is_empty() {
            return false;
        }
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        let Some(f) = st.assets.list.get_mut(index as usize).and_then(|l| l.folders.get_mut(folder as usize)) else {
            return false;
        };
        f.name = name;
        let dir = st.assets.dir.clone();
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        dir.is_none_or(|dir| write(&dir.join(INDEX_FILE), &index_json(&list, &current)))
    }

    /// Deletes the folder at `folder` in the library at `index`. False if it is the library's only
    /// folder or there is none. Its assets' documents go once no other folder names them.
    pub fn delete_asset_folder(&self, index: u32, folder: u32) -> bool {
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        let Some(library) = st.assets.list.get_mut(index as usize) else { return false };
        if library.folders.len() < 2 {
            return false;
        }
        if folder as usize >= library.folders.len() {
            return false;
        }
        library.folders.remove(folder as usize);
        let dir = st.assets.dir.clone();
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        let Some(dir) = dir else { return false };
        if !write(&dir.join(INDEX_FILE), &index_json(&list, &current)) {
            return false;
        }
        forget_unnamed(&dir, &list);
        true
    }
}

impl CanvasSession {
    /// The folder the asset library is kept in, if it has one.
    pub(crate) fn assets_dir(&self) -> Option<PathBuf> {
        self.shared.lock().assets.dir.clone()
    }
}

/// The index as JSON.
fn index_json(list: &[Library], current: &str) -> String {
    serde_json::to_string(&IndexFile { libraries: list, current }).unwrap_or_default()
}

/// Removes the item documents no library's folder names any more.
fn forget_unnamed(dir: &Path, list: &[Library]) {
    let named: std::collections::HashSet<&str> =
        list.iter().flat_map(|l| l.folders.iter()).flat_map(|f| f.assets.iter()).map(|a| a.id.as_str()).collect();
    let Ok(items) = std::fs::read_dir(dir.join(ITEMS)) else { return };
    for entry in items.flatten() {
        let name = entry.file_name();
        let Some(id) = name.to_str() else { continue };
        if !named.contains(id) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// The card of the asset `id` in the library in use's folders, and where it sits (library, folder,
/// place), for the operations that change one asset.
pub(crate) fn find_asset(assets: &Assets, id: &str) -> Option<(usize, usize, usize)> {
    for (li, library) in assets.list.iter().enumerate() {
        for (fi, folder) in library.folders.iter().enumerate() {
            if let Some(ai) = folder.assets.iter().position(|a| a.id == id) {
                return Some((li, fi, ai));
            }
        }
    }
    None
}

/// A thumbnail read from an asset's kept picture, for the panel.
pub(crate) fn asset_thumbnail(dir: &Path, id: &str, max_side: u32) -> Option<ThumbnailImage> {
    let path = item_dir(dir, id).join("thumbnail.png");
    let bytes = std::fs::read(path).ok()?;
    let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let (width, height) = image.dimensions();
    let rgba = image.into_raw();
    let (width, height, rgba) = crate::project::thumbnail(&rgba, width, height, max_side);
    Some(ThumbnailImage { width, height, rgba })
}

/// An asset's whole content: its document and its layers' pixels.
pub(crate) type Content = (Document, Vec<(LayerId, Vec<u8>)>);

/// What of the open artwork a capture takes.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetScope {
    CurrentLayer,
    Group,
    Picked,
    Canvas,
    Selection,
}

/// A group layer with nothing of its own set.
fn group_layer(id: LayerId, name: &str, parent: Option<LayerId>) -> Layer {
    Layer {
        id,
        name: name.to_owned(),
        visible: true,
        opacity: 1.0,
        blend: BlendMode::Normal,
        alpha_lock: false,
        clipped: false,
        locked: false,
        reference: false,
        kind: LayerKind::Group,
        parent,
        collapsed: false,
        text: None,
        private: false,
        assisted: false,
        plugin: None,
    }
}

/// A one-layer document holding `rgba` (premultiplied), the canvas's size.
fn flat_document(width: u32, height: u32, rgba: Vec<u8>) -> Content {
    let doc = Document::new(width, height);
    let id = doc.current();
    (doc, vec![(id, rgba)])
}

/// The roots a structured capture takes: the current layer, its group, or the picked layers.
fn capture_roots(st: &State, scope: AssetScope) -> Vec<LayerId> {
    let current = st.doc.current();
    match scope {
        AssetScope::Picked => st.doc.chosen(),
        AssetScope::Group => {
            if st.doc.layer(current).is_some_and(|l| l.kind == LayerKind::Group) {
                vec![current]
            } else if let Some(g) = st.doc.group_of(current) {
                vec![g]
            } else {
                vec![current]
            }
        }
        _ => vec![current],
    }
}

impl CanvasSession {
    /// The document and pixels a capture of `scope` takes, without changing the artwork.
    fn capture_document(&self, scope: AssetScope) -> Option<Content> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (w, h) = (st.doc.width(), st.doc.height());
        if scope == AssetScope::Canvas {
            return Some(flat_document(w, h, st.composite_with_background()?));
        }
        if scope == AssetScope::Selection {
            return Some(flat_document(w, h, st.selected_pixels()?));
        }
        let roots = capture_roots(st, scope);
        let mut keep: HashSet<LayerId> = HashSet::new();
        for &r in &roots {
            keep.insert(r);
            if st.doc.layer(r).is_some_and(|l| l.kind == LayerKind::Group) {
                keep.extend(st.doc.contents(r));
            }
        }
        for id in keep.clone() {
            if let Some(m) = st.doc.mask_of(id) {
                keep.insert(m);
            }
        }
        if keep.is_empty() {
            return None;
        }
        // the kept layers as they are, their ids kept (a subset of the document's, so still unique);
        // a layer whose group isn't kept comes to the top level
        let layers: Vec<Layer> = st
            .doc
            .layers()
            .iter()
            .filter(|l| keep.contains(&l.id))
            .map(|l| {
                let mut l = l.clone();
                l.parent = l.parent.filter(|p| keep.contains(p));
                l
            })
            .collect();
        let current = layers.iter().rev().find(|l| l.kind != LayerKind::Mask).map(|l| l.id)?;
        let mut doc = Document::from_layers(w, h, layers, current);
        doc.keep_ids_from(&st.doc);
        let mut pixels = Vec::new();
        for l in doc.layers() {
            if l.kind == LayerKind::Group {
                continue;
            }
            pixels.push((l.id, st.layer_pixels(l.id)?));
        }
        Some((doc, pixels))
    }

    /// Keeps `doc` and its pixels as a new asset in the folder at `library`/`folder`, named `name`;
    /// its id, or none if it couldn't be kept.
    fn keep_asset(
        &self,
        doc: Document,
        pixels: Vec<(LayerId, Vec<u8>)>,
        name: String,
        library: u32,
        folder: u32,
    ) -> Option<String> {
        let dir = self.assets_dir()?;
        let (width, height) = (doc.width(), doc.height());
        let id = fresh_id();
        let item = item_dir(&dir, &id);
        let session = CanvasSession::with_document(doc, name.clone(), pixels);
        if session.save(item.to_string_lossy().into_owned()).is_err() {
            return None;
        }
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        let f = st.assets.list.get_mut(library as usize)?.folders.get_mut(folder as usize)?;
        let names: Vec<String> = f.assets.iter().map(|a| a.name.clone()).collect();
        let name = unique_name(&names, &name);
        f.assets.push(AssetCard { id: id.clone(), name, width, height });
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        write(&dir.join(INDEX_FILE), &index_json(&list, &current)).then_some(id)
    }

    /// Places the asset `id` above the current layer: one layer if it holds a single image, a group
    /// if it holds several. Returns the new layer's or group's id, or none if there is no room or it
    /// can't be read. The app enters transform on it afterwards.
    fn insert_asset(&self, doc: Document, pixels: Vec<(LayerId, Vec<u8>)>, name: &str) -> Option<u64> {
        let (aw, ah) = (doc.width(), doc.height());
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let (tw, th) = (st.doc.width(), st.doc.height());
        if !st.doc.has_room(pixels.len()) {
            return None;
        }
        let current = st.doc.current();
        let context = st.doc.group_of(current);
        let offset = st.doc.layers().iter().map(|l| l.id.0).max().unwrap_or(0) + 1;
        let multi = pixels.len() > 1 || doc.layers().iter().any(|l| l.kind == LayerKind::Group);
        let mut layers: Vec<Layer> = doc
            .layers()
            .iter()
            .map(|l| {
                let mut l = l.clone();
                l.id = LayerId(l.id.0 + offset);
                l.parent = l.parent.map(|p| LayerId(p.0 + offset));
                l
            })
            .collect();
        let next = offset + doc.layers().iter().map(|l| l.id.0).max().unwrap_or(0) + 1;
        let top = if multi {
            let g = LayerId(next);
            for l in layers.iter_mut() {
                if l.parent.is_none() {
                    l.parent = Some(g);
                }
            }
            layers.push(group_layer(g, name, context));
            g
        } else {
            let id = layers.iter().find(|l| l.kind == LayerKind::Paint)?.id;
            for l in layers.iter_mut() {
                if l.parent.is_none() {
                    l.parent = context;
                }
            }
            id
        };
        // each layer's pixels centred and fitted onto this canvas, all by the asset's own size so a
        // group's layers stay aligned
        let mut placed_pixels: Vec<(LayerId, Vec<u8>)> = Vec::new();
        for (id, rgba) in &pixels {
            let image = ThumbnailImage { width: aw, height: ah, rgba: rgba.clone() };
            placed_pixels.push((LayerId(id.0 + offset), crate::clipboard::placed(image, tw, th, true)?));
        }
        let at = st.doc.layers().iter().position(|l| l.id == current)?;
        let after_mask = at + 1 + usize::from(st.doc.mask_of(current).is_some());
        let before = st.doc.clone();
        st.doc.insert_layers(after_mask, layers);
        // once the place's transform is confirmed, go back to the layer we were on
        st.place_return = Some((current, top));
        st.doc.set_current(top);
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("place asset");
            canvas.record_document(before);
            if let Some(gpu) = st.gpu.as_ref() {
                for (id, rgba) in &placed_pixels {
                    let _ = canvas.upload_layer(gpu, *id, rgba);
                }
            }
        }
        st.revision += 1;
        self.shared.frame(st);
        Some(top.0)
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Captures `scope` of the open artwork as a new asset named `name` in the folder at
    /// `library`/`folder`; its id, or none if it couldn't be taken or kept.
    pub fn capture_asset(&self, name: String, scope: AssetScope, library: u32, folder: u32) -> Option<String> {
        let (doc, pixels) = self.capture_document(scope)?;
        let name = if name.trim().is_empty() { NEW_ASSET.to_owned() } else { name.trim().chars().take(256).collect() };
        self.keep_asset(doc, pixels, name, library, folder)
    }

    /// Places the asset `id` into the artwork (see [`CanvasSession::insert_asset`]); its new layer's
    /// or group's id, or none.
    pub fn place_asset(&self, id: String) -> Option<u64> {
        let dir = self.assets_dir()?;
        let (doc, pixels) = crate::project::read_artwork(&item_dir(&dir, &id))?;
        // the group it lands in takes the asset's own name
        let name = {
            let st = self.shared.lock();
            st.assets
                .list
                .iter()
                .flat_map(|l| l.folders.iter())
                .flat_map(|f| f.assets.iter())
                .find(|a| a.id == id)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| "Asset".to_owned())
        };
        self.insert_asset(doc, pixels, &name)
    }

    /// The asset `id`'s picture, scaled to fit `max_side`, for the panel.
    pub fn asset_preview(&self, id: String, max_side: u32) -> Option<ThumbnailImage> {
        let dir = self.assets_dir()?;
        asset_thumbnail(&dir, &id, max_side)
    }

    /// Gives the asset `id` a thumbnail of the user's choosing (a PNG or JPEG), in place of the one
    /// made from what was added. False if it couldn't be read or kept.
    pub fn set_asset_thumbnail(&self, id: String, bytes: Vec<u8>) -> bool {
        let Some(dir) = self.assets_dir() else { return false };
        let Ok(img) = image::load_from_memory(&bytes) else { return false };
        let img = img.to_rgba8();
        let (w, h) = img.dimensions();
        let prem = crate::clipboard::premultiplied(ThumbnailImage { width: w, height: h, rgba: img.into_raw() });
        let (tw, th, small) = crate::project::thumbnail(&prem.rgba, w, h, crate::project::THUMBNAIL_SIDE);
        let _writing = asset_writes();
        keep_thumbnail(&dir, &id, tw, th, &small)
    }

    /// Gives the asset `id` the open canvas as its thumbnail, as it is seen now. False if there is
    /// no canvas to read or it couldn't be kept.
    pub fn set_asset_thumbnail_from_canvas(&self, id: String) -> bool {
        let Some(dir) = self.assets_dir() else { return false };
        let got = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            let (w, h) = (st.doc.width(), st.doc.height());
            st.composite_with_background().map(|rgba| (w, h, rgba))
        };
        let Some((w, h, rgba)) = got else { return false };
        let (tw, th, small) = crate::project::thumbnail(&rgba, w, h, crate::project::THUMBNAIL_SIDE);
        let _writing = asset_writes();
        keep_thumbnail(&dir, &id, tw, th, &small)
    }

    /// Gives the asset `id` the current layer as its thumbnail. False if it couldn't be read or kept.
    pub fn set_asset_thumbnail_from_layer(&self, id: String) -> bool {
        let Some(dir) = self.assets_dir() else { return false };
        let current = self.shared.lock().doc.current();
        let Some(img) = self.copy_layer_contents(current.0) else { return false };
        let (tw, th, small) =
            crate::project::thumbnail(&img.rgba, img.width, img.height, crate::project::THUMBNAIL_SIDE);
        let _writing = asset_writes();
        keep_thumbnail(&dir, &id, tw, th, &small)
    }

    /// Renames the asset `id`; false if there is none or the name is blank.
    pub fn rename_asset(&self, id: String, name: String) -> bool {
        let name: String = name.trim().chars().take(256).collect();
        if name.is_empty() {
            return false;
        }
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        let Some((li, fi, ai)) = find_asset(&st.assets, &id) else { return false };
        let Some(card) =
            st.assets.list.get_mut(li).and_then(|l| l.folders.get_mut(fi)).and_then(|f| f.assets.get_mut(ai))
        else {
            return false;
        };
        card.name = name;
        let dir = st.assets.dir.clone();
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        dir.is_none_or(|dir| write(&dir.join(INDEX_FILE), &index_json(&list, &current)))
    }

    /// Deletes the asset `id` from every folder; its document goes once no folder names it. False
    /// if there is none.
    pub fn delete_asset(&self, id: String) -> bool {
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        if find_asset(&st.assets, &id).is_none() {
            return false;
        }
        for library in st.assets.list.iter_mut() {
            for folder in library.folders.iter_mut() {
                folder.assets.retain(|a| a.id != id);
            }
        }
        let dir = st.assets.dir.clone();
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        let Some(dir) = dir else { return false };
        if !write(&dir.join(INDEX_FILE), &index_json(&list, &current)) {
            return false;
        }
        forget_unnamed(&dir, &list);
        true
    }

    /// A copy of the asset `id` just after it in its folder; the copy's id, or none.
    pub fn duplicate_asset(&self, id: String) -> Option<String> {
        let _writing = asset_writes();
        let dir = self.assets_dir()?;
        let mut st = self.shared.lock();
        let (li, fi, ai) = find_asset(&st.assets, &id)?;
        let folder = st.assets.list.get_mut(li).and_then(|l| l.folders.get_mut(fi))?;
        let card = folder.assets.get(ai)?.clone();
        let copy = fresh_id();
        copy_dir_all(&item_dir(&dir, &id), &item_dir(&dir, &copy)).ok()?;
        let names: Vec<String> = folder.assets.iter().map(|a| a.name.clone()).collect();
        let copy_name = unique_name(&names, &card.name);
        let mut copy_card = card;
        copy_card.id = copy.clone();
        copy_card.name = copy_name;
        folder.assets.insert(ai + 1, copy_card);
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        write(&dir.join(INDEX_FILE), &index_json(&list, &current)).then_some(copy)
    }

    /// Moves the asset `id` to the end of the folder at `library`/`folder`; false if there is no
    /// such asset or folder.
    pub fn move_asset(&self, id: String, library: u32, folder: u32) -> bool {
        let _writing = asset_writes();
        let mut st = self.shared.lock();
        let Some((li, fi, ai)) = find_asset(&st.assets, &id) else { return false };
        let Some(from) = st.assets.list.get_mut(li).and_then(|l| l.folders.get_mut(fi)) else { return false };
        if ai >= from.assets.len() {
            return false;
        }
        let card = from.assets.remove(ai);
        let Some(to) = st.assets.list.get_mut(library as usize).and_then(|l| l.folders.get_mut(folder as usize)) else {
            return false;
        };
        to.assets.push(card);
        let dir = st.assets.dir.clone();
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        dir.is_none_or(|dir| write(&dir.join(INDEX_FILE), &index_json(&list, &current)))
    }
}

/// How a layered PSD is brought in: one grouped asset, or each layer its own.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PsdImportMode {
    Group,
    Explode,
}

#[uniffi::export]
impl CanvasSession {
    /// Imports a picture (PNG or JPEG) as a single-image asset named `name`; its id, or none.
    pub fn import_asset_image(&self, bytes: Vec<u8>, name: String, library: u32, folder: u32) -> Option<String> {
        let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
        let (w, h) = image.dimensions();
        let (w, h, rgba) = crate::project::thumbnail(&image.into_raw(), w, h, 4096);
        let prem = crate::clipboard::premultiplied(ThumbnailImage { width: w, height: h, rgba });
        let (doc, pixels) = flat_document(w, h, prem.rgba);
        let name = asset_name(&name);
        self.keep_asset(doc, pixels, name, library, folder)
    }

    /// Imports a layered PSD (or PSB): one grouped asset, or each paint layer its own. The number of
    /// assets made, or none if it couldn't be read.
    pub fn import_asset_psd(
        &self,
        bytes: Vec<u8>,
        name: String,
        mode: PsdImportMode,
        library: u32,
        folder: u32,
    ) -> Option<u32> {
        let imported = procraft_engine::psd::read(&bytes).ok()?;
        let name = if name.trim().is_empty() { imported.name.clone() } else { name.trim().chars().take(256).collect() };
        match mode {
            PsdImportMode::Group => {
                self.keep_asset(imported.document, imported.pixels, name, library, folder).map(|_| 1)
            }
            PsdImportMode::Explode => {
                let (w, h) = (imported.document.width(), imported.document.height());
                let mut count = 0u32;
                for l in imported.document.layers() {
                    if l.kind != LayerKind::Paint {
                        continue;
                    }
                    let Some(px) = imported.pixels.iter().find(|(id, _)| *id == l.id).map(|(_, p)| p.clone()) else {
                        continue;
                    };
                    let doc = Document::from_layers(w, h, vec![l.clone()], l.id);
                    let lname = if l.name.trim().is_empty() { format!("{name} {}", count + 1) } else { l.name.clone() };
                    if self.keep_asset(doc, vec![(l.id, px)], lname, library, folder).is_some() {
                        count += 1;
                    }
                }
                (count > 0).then_some(count)
            }
        }
    }

    /// Imports another artwork's layers as one asset (a group if it has several); its id, or none.
    pub fn import_asset_from_project(&self, dir: String, name: String, library: u32, folder: u32) -> Option<String> {
        let (doc, pixels) = crate::project::read_artwork(Path::new(&dir))?;
        let name = asset_name(&name);
        self.keep_asset(doc, pixels, name, library, folder)
    }

    /// Imports a paper triplet - its colour `m`, screen `s` and burn `c` pictures - as one grouped
    /// asset: the colour Multiply at full, the screen Screen at half, the burn Colour Burn at two
    /// fifths, the paper's own construction. Its id, or none if a picture couldn't be read.
    pub fn import_asset_paper(
        &self,
        m: Vec<u8>,
        s: Vec<u8>,
        c: Vec<u8>,
        name: String,
        library: u32,
        folder: u32,
    ) -> Option<String> {
        let m_img = image::load_from_memory(&m).ok()?.to_rgba8();
        let (mw, mh) = m_img.dimensions();
        let (w, h, m_rgba) = crate::project::thumbnail(&m_img.into_raw(), mw, mh, 4096);
        let m_px = crate::clipboard::premultiplied(ThumbnailImage { width: w, height: h, rgba: m_rgba }).rgba;
        let s_px = paper_layer_pixels(&s, w, h)?;
        let c_px = paper_layer_pixels(&c, w, h)?;
        let layers = vec![
            paper_layer(LayerId(1), "Colour", BlendMode::Multiply, 1.0),
            paper_layer(LayerId(2), "Screen", BlendMode::Screen, 0.5),
            paper_layer(LayerId(3), "Burn", BlendMode::ColorBurn, 0.4),
        ];
        // the paper shows as its colour layer, not the blended stack
        let (tw, th, m_thumb) = crate::project::thumbnail(&m_px, w, h, crate::project::THUMBNAIL_SIDE);
        let doc = Document::from_layers(w, h, layers, LayerId(3));
        let pixels = vec![(LayerId(1), m_px.clone()), (LayerId(2), s_px.clone()), (LayerId(3), c_px.clone())];
        let id = self.keep_asset(doc, pixels, asset_name(&name), library, folder)?;
        if let Some(dir) = self.assets_dir() {
            let _writing = asset_writes();
            keep_thumbnail(&dir, &id, tw, th, &m_thumb);
            // the layers are opaque photographs: keep them as JPEG so a shared library stays small
            let item = item_dir(&dir, &id);
            for (lid, px) in [(LayerId(1), &m_px), (LayerId(2), &s_px), (LayerId(3), &c_px)] {
                crate::project::store_layer_jpeg(&item, lid, w, h, px);
            }
        }
        Some(id)
    }

    /// Imports every paper triplet in `dir` (`<Name>_M.jpg` with its `_S.jpg` and `_C.jpg`) as a
    /// grouped asset each, named after the paper. The number of papers brought in, or none.
    pub fn import_asset_paper_pack(&self, dir: String, library: u32, folder: u32) -> Option<u32> {
        let dir = Path::new(&dir);
        let mut bases: Vec<String> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix("_M.jpg")).map(str::to_owned))
            .collect();
        bases.sort();
        bases.dedup();
        let mut count = 0u32;
        for base in bases {
            let (Ok(m), Ok(s), Ok(c)) = (
                std::fs::read(dir.join(format!("{base}_M.jpg"))),
                std::fs::read(dir.join(format!("{base}_S.jpg"))),
                std::fs::read(dir.join(format!("{base}_C.jpg"))),
            ) else {
                continue;
            };
            let name = base.replace('-', " ");
            if self.import_asset_paper(m, s, c, name, library, folder).is_some() {
                count += 1;
            }
        }
        (count > 0).then_some(count)
    }
}

/// What a shared file holds: one asset, a folder of them, or a whole library.
#[derive(Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
enum ShareKind {
    Asset,
    Set,
    Library,
}

/// A shared file's manifest: the names and structure; each asset's document goes in the zip beside
/// it (its layers' blends and opacities live in there).
#[derive(serde::Serialize, serde::Deserialize)]
struct ShareManifest {
    format: String,
    version: u32,
    kind: ShareKind,
    name: String,
    libraries: Vec<ShareLibrary>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ShareLibrary {
    name: String,
    folders: Vec<ShareFolder>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ShareFolder {
    name: String,
    assets: Vec<AssetCard>,
}

impl CanvasSession {
    /// Writes `libs` as a shared file of `kind` named `name` at `path`: the manifest, then every
    /// asset's document files. False if any of it couldn't be read or kept.
    fn write_share(&self, libs: &[Library], kind: ShareKind, name: &str, path: &str) -> bool {
        let Some(dir) = self.assets_dir() else { return false };
        let manifest = ShareManifest {
            format: SHARE_FORMAT.to_owned(),
            version: 1,
            kind,
            name: name.to_owned(),
            libraries: libs
                .iter()
                .map(|l| ShareLibrary {
                    name: l.name.clone(),
                    folders: l
                        .folders
                        .iter()
                        .map(|f| ShareFolder { name: f.name.clone(), assets: f.assets.clone() })
                        .collect(),
                })
                .collect(),
        };
        let Ok(text) = serde_json::to_string(&manifest) else { return false };
        let mut zip = ZipWriter::new();
        zip.add(SHARE_MANIFEST, text.as_bytes());
        // an asset can sit in several folders; its files go in once
        let ids: HashSet<&str> =
            libs.iter().flat_map(|l| &l.folders).flat_map(|f| &f.assets).map(|a| a.id.as_str()).collect();
        for id in ids {
            if zip_tree(&mut zip, &dir, &item_dir(&dir, id)).is_err() {
                return false;
            }
        }
        std::fs::write(path, zip.finish()).is_ok()
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The asset `id` as a file at `path` (a `.asset`: its document and a manifest). False if it
    /// couldn't be written.
    pub fn export_asset(&self, id: String, path: String) -> bool {
        let st = self.shared.lock();
        let Some((li, fi, ai)) = find_asset(&st.assets, &id) else { return false };
        let Some(library) = st.assets.list.get(li) else { return false };
        let Some(folder) = library.folders.get(fi) else { return false };
        let Some(card) = folder.assets.get(ai) else { return false };
        let scope = Library {
            id: library.id.clone(),
            name: library.name.clone(),
            folders: vec![Folder { id: folder.id.clone(), name: folder.name.clone(), assets: vec![card.clone()] }],
        };
        let name = card.name.clone();
        drop(st);
        self.write_share(&[scope], ShareKind::Asset, &name, &path)
    }

    /// The folder at `library`/`folder` as a file at `path` (a `.assetset`). False if there is no
    /// such folder or it couldn't be written.
    pub fn export_asset_folder(&self, library: u32, folder: u32, path: String) -> bool {
        let st = self.shared.lock();
        let Some(l) = st.assets.list.get(library as usize) else { return false };
        let Some(f) = l.folders.get(folder as usize) else { return false };
        let scope = Library { id: l.id.clone(), name: l.name.clone(), folders: vec![f.clone()] };
        let name = f.name.clone();
        drop(st);
        self.write_share(&[scope], ShareKind::Set, &name, &path)
    }

    /// The library at `index` as a file at `path` (a `.assetlibrary`), its folders and assets. False
    /// if there is none there or it couldn't be written.
    pub fn export_asset_library(&self, library: u32, path: String) -> bool {
        let st = self.shared.lock();
        let Some(l) = st.assets.list.get(library as usize) else { return false };
        let (scope, name) = (l.clone(), l.name.clone());
        drop(st);
        self.write_share(&[scope], ShareKind::Library, &name, &path)
    }

    /// Brings a shared asset file in: a library file becomes libraries of its own, a set a new
    /// folder in `library`, a single asset an addition to `library`/`folder`. Every asset gets a
    /// fresh id and its files copied into the pool. The number brought in, or none if it isn't a
    /// readable asset file.
    pub fn import_asset_file(&self, bytes: Vec<u8>, library: u32, folder: u32) -> Option<u32> {
        let dir = self.assets_dir()?;
        let zip = ZipReader::new(&bytes)?;
        // only plain files whose names stay inside the pool
        if zip.facts().any(|f| !f.plain || f.encrypted || !safe_entry(f.name)) {
            return None;
        }
        let manifest: ShareManifest = serde_json::from_slice(&zip.read(SHARE_MANIFEST)?).ok()?;
        if manifest.format != SHARE_FORMAT || manifest.version != 1 {
            return None;
        }
        let _writing = asset_writes();
        // the place a set or a single asset goes must exist; a library file makes its own
        if manifest.kind != ShareKind::Library {
            let st = self.shared.lock();
            let l = st.assets.list.get(library as usize)?;
            if manifest.kind == ShareKind::Asset && l.folders.get(folder as usize).is_none() {
                return None;
            }
        }
        // each asset's files into the pool under a fresh id, the manifest's structure kept
        let mut libs: Vec<Library> = Vec::new();
        let mut count = 0u32;
        for ml in &manifest.libraries {
            let mut folders = Vec::new();
            for mf in &ml.folders {
                let mut assets: Vec<AssetCard> = Vec::new();
                for card in &mf.assets {
                    let Some(mut new) = take_item(&zip, &dir, card) else { continue };
                    let names: Vec<String> = assets.iter().map(|a| a.name.clone()).collect();
                    new.name = unique_name(&names, &asset_name(&new.name));
                    assets.push(new);
                    count += 1;
                }
                folders.push(Folder { id: fresh_id(), name: mf.name.clone(), assets });
            }
            libs.push(Library { id: String::new(), name: ml.name.clone(), folders });
        }
        if count == 0 {
            return None;
        }
        let mut st = self.shared.lock();
        match manifest.kind {
            ShareKind::Library => {
                for mut l in libs {
                    let names: Vec<String> = st.assets.list.iter().map(|x| x.name.clone()).collect();
                    l.id = new_library_id(&st.assets.list, &dir);
                    l.name = unique_name(&names, &share_name(&l.name, NEW_LIBRARY));
                    st.assets.list.push(l);
                }
            }
            ShareKind::Set => {
                let l = st.assets.list.get_mut(library as usize)?;
                let names: Vec<String> = l.folders.iter().map(|f| f.name.clone()).collect();
                let assets = libs.into_iter().flat_map(|l| l.folders).flat_map(|f| f.assets).collect();
                let name = unique_name(&names, &share_name(&manifest.name, NEW_FOLDER));
                l.folders.push(Folder { id: fresh_id(), name, assets });
            }
            ShareKind::Asset => {
                let f = st.assets.list.get_mut(library as usize).and_then(|l| l.folders.get_mut(folder as usize))?;
                for mut card in libs.into_iter().flat_map(|l| l.folders).flat_map(|f| f.assets) {
                    let names: Vec<String> = f.assets.iter().map(|a| a.name.clone()).collect();
                    card.name = unique_name(&names, &card.name);
                    f.assets.push(card);
                }
            }
        }
        let list = st.assets.list.clone();
        let current = st.assets.current.clone();
        drop(st);
        write(&dir.join(INDEX_FILE), &index_json(&list, &current)).then_some(count)
    }
}

/// A paint layer of a paper triplet, its blend and opacity the construction contract.
fn paper_layer(id: LayerId, name: &str, blend: BlendMode, opacity: f32) -> Layer {
    Layer {
        id,
        name: name.to_owned(),
        visible: true,
        opacity,
        blend,
        alpha_lock: false,
        clipped: false,
        locked: false,
        reference: false,
        kind: LayerKind::Paint,
        parent: None,
        collapsed: false,
        text: None,
        private: false,
        assisted: false,
        plugin: None,
    }
}

/// A paper layer's picture decoded to `w` × `h` (scaled if it differs), premultiplied.
fn paper_layer_pixels(bytes: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (iw, ih) = img.dimensions();
    let rgba = if (iw, ih) == (w, h) {
        img.into_raw()
    } else {
        image::imageops::resize(&img, w, h, image::imageops::FilterType::Lanczos3).into_raw()
    };
    Some(crate::clipboard::premultiplied(ThumbnailImage { width: w, height: h, rgba }).rgba)
}

/// Writes a prepared thumbnail picture over the asset's own.
fn keep_thumbnail(dir: &Path, id: &str, tw: u32, th: u32, rgba: &[u8]) -> bool {
    crate::project::write_atomically(&item_dir(dir, id).join("thumbnail.png"), |p| {
        crate::project::write_png(p, tw, th, rgba, &[])
    })
    .is_ok()
}

impl State {
    /// A confirmed place hands the selection back to the layer it interrupted and folds its group.
    pub(crate) fn settle_place_return(&mut self) {
        if let Some((prev, top)) = self.place_return.take() {
            if self.doc.layer(top).is_some_and(|l| l.kind == LayerKind::Group) {
                self.doc.set_collapsed(top, true);
            }
            if self.doc.layer(prev).is_some() {
                self.doc.set_current(prev);
            }
        }
    }
}

/// A trimmed name, or the default when blank.
fn asset_name(name: &str) -> String {
    share_name(name, NEW_ASSET)
}

/// A trimmed name, or `fallback` when blank (a shared library's or folder's).
fn share_name(name: &str, fallback: &str) -> String {
    let name: String = name.trim().chars().take(256).collect();
    if name.is_empty() { fallback.to_owned() } else { name }
}

/// Copies a folder and everything in it (an asset's document being duplicated).
fn copy_dir_all(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let dest = to.join(entry.file_name());
        if kind.is_dir() {
            copy_dir_all(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// Every file under `dir` into the zip, named by its path from `root`.
fn zip_tree(zip: &mut ZipWriter, root: &Path, dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            zip_tree(zip, root, &path)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            let name: Vec<String> = rel.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            zip.add(&name.join("/"), &std::fs::read(&path)?);
        }
    }
    Ok(())
}

/// Whether a zip entry's name keeps to its own folder: no absolute path, no climbing out.
fn safe_entry(name: &str) -> bool {
    !name.contains('\\') && !name.contains(':') && name.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

/// Copies the item `card` names out of the zip into the pool under a fresh id; its new card, or
/// none if its files couldn't be read. Nothing lands outside the pool.
fn take_item(zip: &ZipReader<'_>, dir: &Path, card: &AssetCard) -> Option<AssetCard> {
    let id = fresh_id();
    let to = item_dir(dir, &id);
    let from = format!("{ITEMS}/{}/", card.id);
    let copy = || -> Option<()> {
        let mut kept = false;
        for name in zip.names() {
            let Some(rest) = name.strip_prefix(from.as_str()) else { continue };
            let path = to.join(rest);
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p).ok()?;
            }
            std::fs::write(&path, zip.read(name)?).ok()?;
            kept = true;
        }
        kept.then_some(())
    };
    // a half-copied or unreadable document is no asset: leave no trace of it
    if copy().is_none() || crate::project::read_artwork(&to).is_none() {
        let _ = std::fs::remove_dir_all(&to);
        return None;
    }
    Some(AssetCard { id, name: card.name.clone(), width: card.width, height: card.height })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-assets-{tag}-{}", fresh_id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn libraries_and_folders_round_trip() {
        let dir = temp_dir("crud");
        let s = CanvasSession::new(64, 64);
        assert!(s.open_assets(dir.to_string_lossy().into_owned()));
        // a first library with a General folder
        let libs = s.asset_libraries();
        assert_eq!(libs.len(), 1);
        assert_eq!(libs[0].folders.len(), 1);
        // another library, a folder in it, renamed
        let li = s.new_asset_library().unwrap();
        assert!(s.open_asset_library(li));
        let fi = s.new_asset_folder(li).unwrap();
        assert!(s.rename_asset_library(li, "Mine".into()));
        assert!(s.rename_asset_folder(li, fi, "Textures".into()));
        // a fresh session reads it back, the one in use kept
        let t = CanvasSession::new(64, 64);
        assert!(t.open_assets(dir.to_string_lossy().into_owned()));
        let libs = t.asset_libraries();
        assert_eq!(libs.len(), 2);
        let mine = libs.iter().find(|l| l.name == "Mine").unwrap();
        assert!(mine.current);
        assert!(mine.folders.iter().any(|f| f.name == "Textures"));
        // the folder and the library go
        assert!(s.delete_asset_folder(li, fi));
        assert!(s.delete_asset_library(li));
        assert_eq!(s.asset_libraries().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn capture_and_place_a_layer() {
        let Ok(gpu) = procraft_gpu::Gpu::headless() else {
            eprintln!("skipped: no GPU");
            return;
        };
        let dir = temp_dir("place");
        let s = CanvasSession::new(64, 64);
        {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            st.window_size = (64, 64);
            st.view = Some(procraft_engine::view::View::fit((64, 64), (64, 64)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
            // paint the current layer: a red square in the middle
            let id = st.doc.current();
            let mut rgba = vec![0u8; 64 * 64 * 4];
            for y in 16..48 {
                for x in 16..48 {
                    let at = (y * 64 + x) * 4;
                    rgba[at] = 255;
                    rgba[at + 3] = 255;
                }
            }
            let canvas = st.canvas.as_mut().unwrap();
            let gpu = st.gpu.as_ref().unwrap();
            canvas.upload_layer(gpu, id, &rgba).unwrap();
        }
        assert!(s.open_assets(dir.to_string_lossy().into_owned()));
        let before = s.layers().len();
        let id = s.capture_asset("Square".into(), AssetScope::CurrentLayer, 0, 0).expect("captured");
        assert!(s.asset_libraries()[0].folders[0].assets.iter().any(|a| a.id == id));
        // placing it adds a layer holding the picture
        let placed = s.place_asset(id).expect("placed");
        assert_ne!(placed, 0);
        assert_eq!(s.layers().len(), before + 1);
        let pic = s.copy_layer_contents(placed).expect("pixels");
        assert!(pic.rgba.as_chunks::<4>().0.iter().any(|p| p[0] > 0 && p[3] > 0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_paper_triplet_keeps_its_blends_and_opacities() {
        let dir = temp_dir("paper");
        let s = CanvasSession::new(64, 64);
        assert!(s.open_assets(dir.to_string_lossy().into_owned()));
        // three tiny pictures: the colour, the screen, the burn
        let pic = |r: u8, g: u8, b: u8| -> Vec<u8> {
            let img = image::RgbaImage::from_fn(8, 8, |_, _| image::Rgba([r, g, b, 255]));
            let mut bytes = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
            bytes
        };
        let id = s
            .import_asset_paper(pic(200, 190, 170), pic(42, 42, 42), pic(196, 196, 196), "Bond Paper".into(), 0, 0)
            .expect("imported");
        let (doc, pixels) = crate::project::read_artwork(&dir.join(ITEMS).join(&id)).expect("read back");
        assert_eq!(doc.layers().len(), 3);
        assert_eq!(pixels.len(), 3);
        // bottom to top: colour Multiply full, screen Screen half, burn Colour Burn two fifths
        let kept: Vec<(u32, f32)> = doc.layers().iter().map(|l| (l.blend.id(), l.opacity)).collect();
        assert_eq!(kept, vec![(1, 1.0), (7, 0.5), (3, 0.4)]);
        // the thumbnail is the colour layer, not the blended stack
        let thumb = s.asset_preview(id.clone(), 64).expect("thumbnail");
        let px = thumb.rgba.as_chunks::<4>().0;
        let red = px.iter().map(|p| p[0] as u32).sum::<u32>() / px.len() as u32;
        assert!(red > 180, "the thumbnail should be the light colour layer, got {red}");
        // and one of the user's choosing replaces it
        assert!(s.set_asset_thumbnail(id.clone(), pic(255, 0, 0)));
        let thumb = s.asset_preview(id, 64).expect("thumbnail");
        assert!(thumb.rgba.as_chunks::<4>().0.iter().all(|p| p[0] > 200 && p[1] < 60));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn shared_files_round_trip() {
        let dir = temp_dir("share");
        let s = CanvasSession::new(64, 64);
        assert!(s.open_assets(dir.to_string_lossy().into_owned()));
        let pic = |r: u8, g: u8, b: u8| -> Vec<u8> {
            let img = image::RgbaImage::from_fn(8, 8, |_, _| image::Rgba([r, g, b, 255]));
            let mut bytes = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
            bytes
        };
        let paper = |name: &str, lib: u32, fol: u32| {
            s.import_asset_paper(pic(200, 190, 170), pic(42, 42, 42), pic(196, 196, 196), name.into(), lib, fol)
                .expect("imported")
        };
        // a library with two folders: General holds one paper, Textures two
        assert!(s.rename_asset_library(0, "Paper Stock".into()));
        let bond = paper("Bond Paper", 0, 0);
        let fi = s.new_asset_folder(0).unwrap();
        assert!(s.rename_asset_folder(0, fi, "Textures".into()));
        paper("Vellum", 0, fi);
        paper("Grain", 0, fi);
        // the blends a paper keeps, read back from an item
        let blends = |d: &Path, id: &str| -> Vec<(u32, f32)> {
            let (doc, _) = crate::project::read_artwork(&d.join(ITEMS).join(id)).unwrap();
            doc.layers().iter().map(|l| (l.blend.id(), l.opacity)).collect()
        };
        let want = vec![(1, 1.0), (7, 0.5), (3, 0.4)];

        // a whole library comes back as a library of its own, structure intact
        let libfile = dir.join("stock.assetlibrary");
        assert!(s.export_asset_library(0, libfile.to_string_lossy().into_owned()));
        let d1 = temp_dir("share-lib");
        let t1 = CanvasSession::new(64, 64);
        assert!(t1.open_assets(d1.to_string_lossy().into_owned()));
        assert_eq!(t1.import_asset_file(std::fs::read(&libfile).unwrap(), 0, 0), Some(3));
        let libs = t1.asset_libraries();
        assert_eq!(libs.len(), 2);
        let stock = libs.iter().find(|l| l.name == "Paper Stock").unwrap();
        assert_eq!(stock.folders.len(), 2);
        let general = stock.folders.iter().find(|f| f.name == "General").unwrap();
        let textures = stock.folders.iter().find(|f| f.name == "Textures").unwrap();
        assert_eq!(general.assets.len(), 1);
        assert_eq!(textures.assets.len(), 2);
        assert_eq!(general.assets[0].name, "Bond Paper");
        assert_eq!(blends(&d1, &general.assets[0].id), want);

        // a folder comes back as a new folder in the library it's sent to
        let setfile = dir.join("textures.assetset");
        assert!(s.export_asset_folder(0, fi, setfile.to_string_lossy().into_owned()));
        let d2 = temp_dir("share-set");
        let t2 = CanvasSession::new(64, 64);
        assert!(t2.open_assets(d2.to_string_lossy().into_owned()));
        assert_eq!(t2.import_asset_file(std::fs::read(&setfile).unwrap(), 0, 0), Some(2));
        let libs = t2.asset_libraries();
        let folders = &libs[0].folders;
        assert_eq!(folders.len(), 2);
        let made = folders.iter().find(|f| f.name == "Textures").unwrap();
        assert_eq!(made.assets.len(), 2);
        assert!(made.assets.iter().any(|a| a.name == "Vellum"));
        assert!(made.assets.iter().any(|a| a.name == "Grain"));
        assert_eq!(blends(&d2, &made.assets[0].id), want);

        // a single asset joins the folder it's sent to
        let onefile = dir.join("bond.asset");
        assert!(s.export_asset(bond.clone(), onefile.to_string_lossy().into_owned()));
        let d3 = temp_dir("share-one");
        let t3 = CanvasSession::new(64, 64);
        assert!(t3.open_assets(d3.to_string_lossy().into_owned()));
        assert_eq!(t3.import_asset_file(std::fs::read(&onefile).unwrap(), 0, 0), Some(1));
        let libs = t3.asset_libraries();
        let assets = &libs[0].folders[0].assets;
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].name, "Bond Paper");
        assert_eq!(blends(&d3, &assets[0].id), want);

        for d in [&dir, &d1, &d2, &d3] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn import_refuses_foreign_and_unsafe_zips() {
        let dir = temp_dir("share-bad");
        let s = CanvasSession::new(64, 64);
        assert!(s.open_assets(dir.to_string_lossy().into_owned()));
        // a zip with no manifest is not an asset file
        let mut w = ZipWriter::new();
        w.add("notes.txt", b"just notes");
        assert_eq!(s.import_asset_file(w.finish(), 0, 0), None);
        // not a zip at all
        assert_eq!(s.import_asset_file(b"PK nothing here".to_vec(), 0, 0), None);
        // a manifest, but an entry that climbs out of the pool
        let manifest = serde_json::json!({
            "format": "procraft-assets", "version": 1, "kind": "asset", "name": "X",
            "libraries": [{ "name": "L", "folders": [{ "name": "F",
                "assets": [{ "id": "it", "name": "X", "width": 8, "height": 8 }] }] }]
        });
        let mut w = ZipWriter::new();
        w.add("assets.json", manifest.to_string().as_bytes());
        w.add("items/it/artwork.json", b"{}");
        w.add("../evil", b"x");
        assert_eq!(s.import_asset_file(w.finish(), 0, 0), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn placing_returns_selection_and_folds_the_group() {
        let dir = temp_dir("place-return");
        let s = CanvasSession::new(64, 64);
        assert!(s.open_assets(dir.to_string_lossy().into_owned()));
        // a layer of our own to come back to
        let base = s.add_layer();
        let pic = |r: u8, g: u8, b: u8| -> Vec<u8> {
            let img = image::RgbaImage::from_fn(8, 8, |_, _| image::Rgba([r, g, b, 255]));
            let mut bytes = Vec::new();
            img.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).unwrap();
            bytes
        };
        let id = s
            .import_asset_paper(pic(200, 190, 170), pic(42, 42, 42), pic(196, 196, 196), "Paper".into(), 0, 0)
            .unwrap();
        let top = s.place_asset(id).unwrap();
        assert!(s.layers().iter().any(|l| l.id == top && l.current));
        // what a confirmed transform does (the GPU box itself isn't needed to check it)
        s.shared.lock().settle_place_return();
        let ls = s.layers();
        assert!(ls.iter().any(|l| l.id == base && l.current), "selection returns to the layer we were on");
        assert!(ls.iter().any(|l| l.id == top && l.collapsed), "the placed group folds closed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
