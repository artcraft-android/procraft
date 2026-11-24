//! The store: the catalogues a person has added, the items they list, and what came from where.
//! A catalogue is one JSON file and a source is one catalogue plus its address; a file on this
//! machine is read here, while a web address waits for the app to fetch it. Nothing in this module
//! touches the network, so everything it does is tested on its own.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::project::{file_error, write_atomically};

/// The sources' list, beside the interface's other state.
const SOURCES_FILE: &str = "stores.json";
/// One source's copy of the catalogue it last read, under the store's own folder.
const CATALOGUE_FILE: &str = "catalogue.json";
/// What that read was: when, its digest, its name and the ids it listed.
const READ_FILE: &str = "read.json";
/// What has been installed from the store.
const INSTALLED_FILE: &str = "installed.json";

// The caps a catalogue and its items hold to (store-spec.md's cap table). Every one is a refusal
// or a drop, never a truncation.
const CATALOGUE_BYTES: usize = 1 << 20;
const ITEMS_MOST: usize = 1000;
const ID_CHARS: usize = 128;
const NAME_CHARS: usize = 64;
const VERSION_CHARS: usize = 32;
const LICENCE_CHARS: usize = 64;
const DESCRIPTION_CHARS: usize = 512;
const LINK_CHARS: usize = 2048;
const BODY_CHARS: usize = 4096;
const PREVIEWS_MOST: usize = 6;
const TAGS_MOST: usize = 12;
const TAG_CHARS: usize = 32;

/// The categories a catalogue may name, and the one a value outside them falls back to.
pub const CATEGORIES: [&str; 8] =
    ["Brushes", "Papers and Textures", "Colour", "Layers", "Interface", "Effects", "Canvas", "Other"];

/// What a catalogue says an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ItemKind {
    Plugin,
    BrushSet,
    AssetPack,
}

impl ItemKind {
    /// The words the interface shows for it.
    pub fn text(self) -> &'static str {
        match self {
            ItemKind::Plugin => "Plugin",
            ItemKind::BrushSet => "Brush set",
            ItemKind::AssetPack => "Asset pack",
        }
    }
}

/// One item a catalogue lists, as the store draws it. An empty string is a field the catalogue
/// left out.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StoreItem {
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub description: String,
    pub licence: String,
    /// What the file says it needs, in words: the interface number a catalogue's `compat` carries.
    pub requires: String,
    pub category: String,
    pub tags: Vec<String>,
    /// The card's picture, empty when the app draws the item's own.
    pub preview: String,
    /// The item page's pictures, in the order to show them.
    pub previews: Vec<String>,
    /// A longer description for the item's own page.
    pub body: String,
    /// Where the file is, empty only for an item that came with the app.
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    /// The source it came from, by id and by the name it showed then.
    pub source: String,
    pub source_name: String,
}

/// What a fetch brought back: the bytes when it worked, and why when it did not.
#[derive(Clone, Debug, uniffi::Record)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub failure: String,
}

/// What the store installed: what its card may say, and the digest an update is told by.
#[derive(Clone, Debug, uniffi::Record)]
pub struct Installed {
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    pub version: String,
    pub publisher: String,
    /// The source it came from, empty when it came with the app.
    pub source: String,
    /// The digest of the file that was taken, which a second copy under the same id changes.
    pub digest: String,
    /// Seconds since the epoch.
    pub at: u64,
}

/// A source as Settings lists it: one catalogue, where it came from, and how its last read went.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SourceInfo {
    pub id: String,
    pub name: String,
    /// The publisher the catalogue named, for the row's second line.
    pub by: String,
    /// The address as it was added: a web address, or a path on this machine.
    pub address: String,
    pub on: bool,
    /// When its catalogue was last read, in seconds since the epoch (0 for never).
    pub read: u64,
    /// Why the last read failed, empty when it did not.
    pub failure: String,
}

/// A source as `stores.json` keeps it: the switch and the address, and a name only where a person
/// gave one (the catalogue names itself otherwise).
#[derive(Clone, Debug, Default, PartialEq)]
struct Saved {
    id: String,
    name: String,
    address: String,
    on: bool,
}

/// One source held: what was saved, the catalogue last read from it, and how that read went.
#[derive(Clone, Debug)]
struct Held {
    saved: Saved,
    catalogue: Option<Catalogue>,
    read: u64,
    failure: String,
}

/// A catalogue that held up: what it calls itself and the items it lists, in its own order.
#[derive(Clone, Debug)]
struct Catalogue {
    name: String,
    publisher: String,
    items: Vec<StoreItem>,
}

/// Why a catalogue was not taken, in words a person reads after "Not added: ".
#[derive(Debug, PartialEq)]
enum Refusal {
    NotJson,
    Schema(u64),
    Shape,
    TooBig,
    TooMany(usize),
    DuplicateId(String),
}

impl Refusal {
    fn text(&self) -> String {
        match self {
            Refusal::NotJson => "it is not JSON".to_owned(),
            Refusal::Schema(schema) => format!("its schema version is {schema}, which this app does not read"),
            Refusal::Shape => "its shape is not a catalogue".to_owned(),
            Refusal::TooBig => "it is larger than the 1 MiB a catalogue may be".to_owned(),
            Refusal::TooMany(items) => {
                format!("it lists {items} items, more than the {ITEMS_MOST} a catalogue may hold")
            }
            Refusal::DuplicateId(id) => format!("two items share the id {id}"),
        }
    }
}

/// The store: every source a person has added, the catalogues read from them, and the items they
/// list together.
#[derive(uniffi::Object)]
pub struct Store {
    dir: Option<PathBuf>,
    held: Mutex<Vec<Held>>,
    /// What has been installed from the store, by catalogue id.
    installed: Mutex<Vec<Installed>>,
    /// How a web address is read. A store of files on this machine never uses it.
    transport: std::sync::Arc<dyn crate::fetch::Transport>,
}

#[uniffi::export]
impl Store {
    /// The store kept at `dir` (its sources and each source's copy), with every local catalogue
    /// read again as it opens.
    #[uniffi::constructor]
    pub fn open(dir: String) -> Arc<Store> {
        let root = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&root);
        Store::with_transport(&root, sources_at(&root), installed_at(&root), std::sync::Arc::new(crate::fetch::Web))
    }

    /// The sources, in the order they were added.
    pub fn sources(&self) -> Vec<SourceInfo> {
        self.held().iter().map(described).collect()
    }

    /// One source, by its id (None when it is gone).
    pub fn source(&self, id: String) -> Option<SourceInfo> {
        self.held().iter().find(|h| h.saved.id == id).map(described)
    }

    /// The categories a catalogue may name, in the order the filter row shows them.
    pub fn categories(&self) -> Vec<String> {
        CATEGORIES.iter().map(|c| (*c).to_owned()).collect()
    }

    /// What the store installed, newest first.
    pub fn installed(&self) -> Vec<Installed> {
        let mut list = self.records().clone();
        list.sort_by_key(|one| std::cmp::Reverse(one.at));
        list
    }

    /// One installed item, by its catalogue id.
    pub fn installed_one(&self, id: String) -> Option<Installed> {
        self.records().iter().find(|one| one.id == id).cloned()
    }

    /// Records that `id` was installed, with the digest of the file that was taken. False when no
    /// source lists it any more, so nothing can be recorded about an item that is gone.
    pub fn keep_installed(&self, id: String, digest: String) -> bool {
        let Some(item) = self.items().into_iter().find(|item| item.id == id) else { return false };
        let one = Installed {
            id: item.id,
            kind: item.kind,
            name: item.name,
            version: item.version,
            publisher: item.publisher,
            source: item.source,
            digest,
            at: now(),
        };
        {
            let mut list = self.records();
            list.retain(|held| held.id != one.id);
            list.push(one);
        }
        self.keep_installed_list();
        true
    }

    /// Forgets an installed item. False when it was not recorded.
    pub fn forget_installed(&self, id: String) -> bool {
        let removed = {
            let mut list = self.records();
            let before = list.len();
            list.retain(|one| one.id != id);
            list.len() != before
        };
        if removed {
            self.keep_installed_list();
        }
        removed
    }

    /// Every item in every source that is on: one item where two sources list the same bytes, two
    /// where they list different bytes under one id.
    pub fn items(&self) -> Vec<StoreItem> {
        let held = self.held();
        merged(&held)
    }

    /// The items whose name, publisher or tags hold `query` (the case does not matter), in the
    /// order the browse view draws them.
    pub fn search(&self, query: String) -> Vec<StoreItem> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let held = self.held();
        merged(&held).into_iter().filter(|i| holds(i, &query)).collect()
    }

    /// Adds the catalogue at `address`: a web address is kept and waits for the app to fetch it, a
    /// file on this machine is read now. None when it was added, or the reason it was not.
    pub fn add_source(&self, address: String) -> Option<String> {
        let address = address.trim().to_owned();
        if address.is_empty() {
            return Some("the address is empty".to_owned());
        }
        let mut held = self.held();
        if held.iter().any(|h| h.saved.address == address) {
            return Some("that address is already added".to_owned());
        }
        let saved = Saved { id: fresh_id(&address, &held), name: String::new(), address, on: true };
        let read = if web(&saved.address) { self.read_web(&saved) } else { read_local(&saved) };
        match read {
            Ok((catalogue, bytes)) => {
                self.keep(&saved, &bytes, &catalogue);
                held.push(Held { saved, catalogue: Some(catalogue), read: now(), failure: String::new() });
                self.keep_sources(&held);
                None
            }
            Err(reason) => Some(reason),
        }
    }

    /// Switches a source on or off: off hides its items and stops it being read.
    pub fn set_on(&self, id: String, on: bool) {
        let mut held = self.held();
        if let Some(h) = held.iter_mut().find(|h| h.saved.id == id) {
            h.saved.on = on;
            self.keep_sources(&held);
        }
    }

    /// Forgets a source: its catalogue, its copy on disk and its place in the list. Nothing it
    /// installed is touched.
    pub fn remove_source(&self, id: String) {
        // the shipped source never leaves: it can only be switched off
        if id == SHIPPED {
            return;
        }
        let mut held = self.held();
        held.retain(|h| h.saved.id != id);
        self.keep_sources(&held);
        if let Some(dir) = self.cache_dir(&id) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// Reads a source again: a file now, a web address once the app can fetch one. None when it
    /// was read, or why it was not; the copy it had stays either way.
    pub fn refresh(&self, id: String) -> Option<String> {
        // nothing to read: the shipped catalogue is already in the binary
        if id == SHIPPED {
            return None;
        }
        let mut held = self.held();
        let Some(h) = held.iter_mut().find(|h| h.saved.id == id) else {
            return Some("that source is gone".to_owned());
        };
        let saved = h.saved.clone();
        let read = if web(&saved.address) { self.read_web(&saved) } else { read_local(&saved) };
        match read {
            Ok((catalogue, bytes)) => {
                self.keep(&saved, &bytes, &catalogue);
                h.catalogue = Some(catalogue);
                h.read = now();
                h.failure = String::new();
                self.keep_sources(&held);
                None
            }
            Err(reason) => {
                h.failure = reason.clone();
                self.keep_sources(&held);
                Some(reason)
            }
        }
    }

    /// When a source's catalogue was last read, in seconds since the epoch.
    pub fn read_at(&self, id: String) -> u64 {
        self.held().iter().find(|h| h.saved.id == id).map_or(0, |h| h.read)
    }

    /// Fetches the file of the item `id`, up to the size the catalogue said it is, and says why
    /// when it does not come. Nothing is installed here: the bytes are handed back to be checked
    /// and taken by whichever app asked.
    pub fn fetch_item(&self, id: String) -> Fetched {
        let Some(item) = self.items().into_iter().find(|item| item.id == id) else {
            return Fetched { bytes: Vec::new(), failure: "no source lists that item".to_owned() };
        };
        if item.file.is_empty() {
            return Fetched {
                bytes: Vec::new(),
                failure: "it came with the app, so there is nothing to fetch".to_owned(),
            };
        }
        if let Err(why) = crate::fetch::web_address(&item.file) {
            return Fetched { bytes: Vec::new(), failure: why };
        }
        let cap = usize::try_from(item.bytes).unwrap_or(usize::MAX).max(1);
        match self.transport.get(&item.file, cap, crate::fetch::FETCH_TIME) {
            Ok(bytes) => Fetched { bytes, failure: String::new() },
            Err(why) => Fetched { bytes: Vec::new(), failure: why },
        }
    }

    /// Fetches a picture by its address: an item's preview, at the size a card or a page shows it.
    pub fn fetch_picture(&self, address: String) -> Fetched {
        if let Err(why) = crate::fetch::web_address(&address) {
            return Fetched { bytes: Vec::new(), failure: why };
        }
        match self.transport.get(&address, PICTURE_BYTES, crate::fetch::FETCH_TIME) {
            Ok(bytes) => Fetched { bytes, failure: String::new() },
            Err(why) => Fetched { bytes: Vec::new(), failure: why },
        }
    }
}

/// The largest a picture may be before it is refused.
const PICTURE_BYTES: usize = 4 * 1024 * 1024;

/// What a web source that has never been read says on its row.
const NOT_READ: &str = "it has not been read yet.";

/// The source that ships with the app: the plugins compiled into the binary, listed like any
/// others. It is never stored, cannot be removed, and can be switched off like the rest.
const SHIPPED: &str = "procraft";
const SHIPPED_NAME: &str = "ProCraft Store";

/// The shipped source: the bundled plugins as one catalogue.
fn shipped_held(on: bool) -> Held {
    Held {
        saved: Saved { id: SHIPPED.to_owned(), name: SHIPPED_NAME.to_owned(), address: String::new(), on },
        catalogue: Some(Catalogue {
            name: SHIPPED_NAME.to_owned(),
            publisher: "ProCraft".to_owned(),
            items: shipped_items().to_vec(),
        }),
        read: 0,
        failure: String::new(),
    }
}

/// The plugins compiled into the app, as the store lists them: their manifests read once for the
/// whole process, since the same files are parsed again by the plugin list at startup.
fn shipped_items() -> &'static [StoreItem] {
    static ITEMS: std::sync::OnceLock<Vec<StoreItem>> = std::sync::OnceLock::new();
    ITEMS.get_or_init(|| {
        crate::bundled::BUNDLED
            .iter()
            .filter_map(|(folder, bytes)| {
                let package = crate::plugin_file::read_package(bytes).ok()?;
                let manifest = package.manifest;
                Some(StoreItem {
                    id: manifest.id,
                    kind: ItemKind::Plugin,
                    name: manifest.name,
                    version: manifest.version,
                    publisher: manifest.publisher,
                    description: manifest.description,
                    licence: String::new(),
                    requires: format!("Interface {}", manifest.interface),
                    category: shipped_category(folder).to_owned(),
                    tags: Vec::new(),
                    preview: String::new(),
                    previews: Vec::new(),
                    body: String::new(),
                    // nothing to fetch: the file is already in the binary, at the size it carries
                    file: String::new(),
                    bytes: bytes.len() as u64,
                    sha256: String::new(),
                    source: SHIPPED.to_owned(),
                    source_name: SHIPPED_NAME.to_owned(),
                })
            })
            .collect()
    })
}

/// The shelf each bundled plugin sits on, as store-categories.md maps them.
fn shipped_category(folder: &str) -> &'static str {
    match folder {
        "brush-sounds" | "coloured-previews" | "krita-brushes" | "randomise-brush" => "Brushes",
        "paper" => "Papers and Textures",
        "colour-triangle" | "pop-up-palette" | "value-check" => "Colour",
        "colour-labels" | "layer-fills" | "layer-tools" | "vector-layers" => "Layers",
        "canvas-surround" | "floating-panels" | "interface" | "keyboard-shortcuts" | "reference-windows"
        | "slider-look" => "Interface",
        "effects" => "Effects",
        "colordrop-tools" | "finger-pressure" | "quickshape-curves" | "radial-segments" | "rotation-reset"
        | "touch-offset" => "Canvas",
        _ => "Other",
    }
}

impl Store {
    fn held(&self) -> std::sync::MutexGuard<'_, Vec<Held>> {
        self.held.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_transport(
        root: &Path,
        held: Vec<Held>,
        installed: Vec<Installed>,
        transport: std::sync::Arc<dyn crate::fetch::Transport>,
    ) -> Arc<Store> {
        Arc::new(Store {
            dir: Some(root.to_path_buf()),
            held: Mutex::new(held),
            installed: Mutex::new(installed),
            transport,
        })
    }

    /// Reads a source from its web address, or says why it could not be read.
    fn read_web(&self, saved: &Saved) -> Result<(Catalogue, Vec<u8>), String> {
        crate::fetch::web_address(&saved.address)?;
        let bytes = self.transport.get(&saved.address, CATALOGUE_BYTES, crate::fetch::FETCH_TIME)?;
        let catalogue = parse(&bytes, saved).map_err(|r| r.text())?;
        Ok((catalogue, bytes))
    }

    #[cfg(test)]
    fn open_with(dir: String, transport: std::sync::Arc<dyn crate::fetch::Transport>) -> Arc<Store> {
        let root = PathBuf::from(dir);
        let _ = std::fs::create_dir_all(&root);
        Store::with_transport(&root, sources_at(&root), installed_at(&root), transport)
    }

    fn records(&self) -> std::sync::MutexGuard<'_, Vec<Installed>> {
        self.installed.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes what has been installed out whole.
    fn keep_installed_list(&self) {
        let Some(dir) = &self.dir else { return };
        let text = write_installed(&self.records());
        let written = write_atomically(&dir.join(INSTALLED_FILE), |tmp| {
            std::fs::write(tmp, &text).map_err(|e| file_error("saving what was installed", e))
        });
        if let Err(e) = written {
            log::warn!("keeping what the store installed: {e}");
        }
    }

    fn cache_dir(&self, id: &str) -> Option<PathBuf> {
        self.dir.as_ref().map(|d| d.join(id))
    }

    /// Writes the sources' list out whole.
    fn keep_sources(&self, held: &[Held]) {
        let Some(dir) = &self.dir else { return };
        let text = write_sources(held);
        let written = write_atomically(&dir.join(SOURCES_FILE), |tmp| {
            std::fs::write(tmp, &text).map_err(|e| file_error("saving the sources", e))
        });
        if let Err(e) = written {
            log::warn!("keeping the store's sources: {e}");
        }
    }

    fn keep(&self, saved: &Saved, bytes: &[u8], catalogue: &Catalogue) {
        if let Some(dir) = &self.dir {
            keep_catalogue(dir, saved, bytes, catalogue);
        }
    }
}

/// Keeps a catalogue's bytes and what the read found, so the next open draws from the copy before
/// it reads anywhere.
fn keep_catalogue(root: &Path, saved: &Saved, bytes: &[u8], catalogue: &Catalogue) {
    let dir = root.join(&saved.id);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("keeping a catalogue's folder: {e}");
        return;
    }
    let write = |name: &str, text: &[u8]| {
        let written = write_atomically(&dir.join(name), |tmp| {
            std::fs::write(tmp, text).map_err(|e| file_error("saving a catalogue", e))
        });
        if let Err(e) = written {
            log::warn!("keeping a catalogue's {name}: {e}");
        }
    };
    write(CATALOGUE_FILE, bytes);
    write(READ_FILE, read_record(catalogue, bytes).as_bytes());
}

fn described(h: &Held) -> SourceInfo {
    let mut name = h.saved.name.clone();
    if name.is_empty() {
        name = match &h.catalogue {
            Some(catalogue) if !catalogue.name.is_empty() => catalogue.name.clone(),
            _ => h.saved.address.clone(),
        };
    }
    SourceInfo {
        id: h.saved.id.clone(),
        name,
        by: h.catalogue.as_ref().map(|c| c.publisher.clone()).unwrap_or_default(),
        address: h.saved.address.clone(),
        on: h.saved.on,
        read: h.read,
        failure: h.failure.clone(),
    }
}

/// The items of every source that is on, in the order Settings lists the sources and each
/// catalogue's own order within one, with the same bytes from two sources drawn once.
fn merged(held: &[Held]) -> Vec<StoreItem> {
    let mut out: Vec<StoreItem> = Vec::new();
    let mut seen: Vec<(String, String)> = Vec::new();
    for h in held.iter().filter(|h| h.saved.on) {
        let Some(catalogue) = &h.catalogue else { continue };
        for item in &catalogue.items {
            // one id with the same digest is one item; one id with a different digest is two
            let key = (item.id.clone(), item.sha256.clone());
            if seen.contains(&key) {
                continue;
            }
            seen.push(key);
            out.push(item.clone());
        }
    }
    out
}

fn holds(item: &StoreItem, query: &str) -> bool {
    item.name.to_lowercase().contains(query)
        || item.publisher.to_lowercase().contains(query)
        || item.tags.iter().any(|t| t.to_lowercase().contains(query))
}

fn web(address: &str) -> bool {
    let lower = address.to_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://")
}

/// The bytes a source at a path holds and the catalogue they parse to, or why not.
fn read_local(saved: &Saved) -> Result<(Catalogue, Vec<u8>), String> {
    let bytes = std::fs::read(Path::new(&saved.address)).map_err(|e| format!("it could not be read: {e}"))?;
    let catalogue = parse(&bytes, saved).map_err(|r| r.text())?;
    Ok((catalogue, bytes))
}

/// The sources a store's file lists, as they open: the one that ships with the app first.
fn sources_at(root: &Path) -> Vec<Held> {
    let (saved, shipped_on) = std::fs::read_to_string(root.join(SOURCES_FILE))
        .ok()
        .map_or_else(|| (Vec::new(), true), |text| read_sources(&text));
    let mut held: Vec<Held> = vec![shipped_held(shipped_on)];
    held.extend(saved.into_iter().map(|s| read_source(root, s)));
    held
}

/// What a store's file says has been installed.
fn installed_at(root: &Path) -> Vec<Installed> {
    std::fs::read_to_string(root.join(INSTALLED_FILE)).ok().map_or_else(Vec::new, |text| read_installed(&text))
}

/// A source as it opens: its cached copy, read again where it is a file on this machine.
fn read_source(root: &Path, saved: Saved) -> Held {
    let dir = root.join(&saved.id);
    let cached = std::fs::read(dir.join(CATALOGUE_FILE)).ok().and_then(|b| parse(&b, &saved).ok());
    let (read, failure) = read_record_of(&dir);
    if web(&saved.address) {
        let failure = if cached.is_some() && failure.is_empty() { String::new() } else { NOT_READ.to_owned() };
        return Held { saved, catalogue: cached, read, failure };
    }
    match read_local(&saved) {
        Ok((catalogue, bytes)) => {
            keep_catalogue(root, &saved, &bytes, &catalogue);
            Held { saved, catalogue: Some(catalogue), read: now(), failure: String::new() }
        }
        // a file that will not read now still shows the copy it had, with the reason it is stale
        Err(reason) => Held { saved, catalogue: cached, read, failure: reason },
    }
}

/// The time and the failure a source's read record kept.
fn read_record_of(dir: &Path) -> (u64, String) {
    let text = std::fs::read_to_string(dir.join(READ_FILE)).unwrap_or_default();
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { return (0, String::new()) };
    let read = value.get("read").and_then(|v| v.as_str()).and_then(from_iso).unwrap_or(0);
    let failure = value.get("failure").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
    (read, failure)
}

/// The ids and the switch, as `stores.json` keeps them. The shipped source is not stored there:
/// only whether it is on is, under `shipped`.
fn write_sources(held: &[Held]) -> String {
    let shipped = !held.iter().any(|h| h.saved.id == SHIPPED && !h.saved.on);
    let sources: Vec<serde_json::Value> = held
        .iter()
        .filter(|h| h.saved.id != SHIPPED)
        .map(|h| {
            let mut one = serde_json::Map::new();
            one.insert("id".to_owned(), serde_json::json!(h.saved.id.clone()));
            if !h.saved.name.is_empty() {
                one.insert("name".to_owned(), serde_json::json!(h.saved.name.clone()));
            }
            let key = if web(&h.saved.address) { "url" } else { "path" };
            one.insert(key.to_owned(), serde_json::json!(h.saved.address.clone()));
            one.insert("on".to_owned(), serde_json::json!(h.saved.on));
            serde_json::Value::Object(one)
        })
        .collect();
    serde_json::json!({ "schema": 1, "shipped": shipped, "sources": sources }).to_string()
}

/// The sources a store has added, and whether the one that ships with it is switched on.
fn read_sources(text: &str) -> (Vec<Saved>, bool) {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(text) else { return (Vec::new(), true) };
    let shipped = root.get("shipped").and_then(|v| v.as_bool()).unwrap_or(true);
    let Some(list) = root.get("sources").and_then(|v| v.as_array()) else { return (Vec::new(), shipped) };
    let saved = list
        .iter()
        .filter_map(|s| {
            let id = s.get("id")?.as_str()?.to_owned();
            if id == SHIPPED {
                return None;
            }
            let name = s.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
            let address =
                s.get("url").and_then(|v| v.as_str()).or_else(|| s.get("path").and_then(|v| v.as_str()))?.to_owned();
            let on = s.get("on").and_then(|v| v.as_bool()).unwrap_or(true);
            (!id.is_empty() && !address.is_empty()).then_some(Saved { id, name, address, on })
        })
        .collect();
    (saved, shipped)
}

/// What a read record holds: when it was, its digest, the name it read, and the ids it listed.
fn read_record(catalogue: &Catalogue, bytes: &[u8]) -> String {
    let mut ids: Vec<&str> = catalogue.items.iter().map(|i| i.id.as_str()).collect();
    ids.sort_unstable();
    serde_json::json!({
        "schema": 1,
        "read": iso(now()),
        "sha256": sha256_hex(bytes),
        "name": &catalogue.name,
        "items": ids,
        "key": serde_json::Value::Null,
        "failure": serde_json::Value::Null,
        "failed_at": serde_json::Value::Null,
    })
    .to_string()
}

/// The digest of a file, as a catalogue writes it: sha256, lowercase hex.
pub fn digest_of(bytes: &[u8]) -> String {
    sha256_hex(bytes)
}

/// The details file a store package carries: what a listing needs to know about the set, and
/// nothing else. The file itself stays the set's own; this is what the store reads beside it.
pub fn package_details(
    kind: &str,
    name: &str,
    author: &str,
    description: &str,
    category: &str,
    licence: &str,
    tags: &[String],
    pictures: &[String],
    exported: &str,
) -> String {
    serde_json::json!({
        "schema": 1,
        "kind": kind,
        "name": name,
        "author": author,
        "description": description,
        "category": category,
        "licence": licence,
        "tags": tags,
        "pictures": pictures,
        "exported": exported,
    })
    .to_string()
}

/// A short id for a source, from its address, so the same address keeps the same id.
fn fresh_id(address: &str, held: &[Held]) -> String {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in address.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    let mut bump = 0u32;
    loop {
        let candidate = format!("{:06x}", hash.wrapping_add(bump) & 0xFF_FFFF);
        if !held.iter().any(|h| h.saved.id == candidate) {
            return candidate;
        }
        bump = bump.wrapping_add(1);
    }
}

fn now() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => since.as_secs(),
        Err(_) => 0,
    }
}

/// `secs` since the epoch, as `YYYY-MM-DDTHH:MM:SSZ`.
fn iso(secs: u64) -> String {
    let (year, month, day) = civil((secs / 86_400) as i64);
    let rest = secs % 86_400;
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", rest / 3600, (rest % 3600) / 60, rest % 60)
}

/// The day `days` days after 1970, as a year, month and day (Howard Hinnant's civil date).
fn civil(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_part + 2) / 5 + 1) as u32;
    let month = if month_part < 10 { month_part + 3 } else { month_part - 9 } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The seconds `YYYY-MM-DDTHH:MM:SSZ` names, None when it will not read.
fn from_iso(text: &str) -> Option<u64> {
    let (date, time) = text.split_once('T')?;
    let mut days = date.split('-');
    let year: i64 = days.next()?.parse().ok()?;
    let month: u32 = days.next()?.parse().ok()?;
    let day: u32 = days.next()?.parse().ok()?;
    let mut clock = time.trim_end_matches('Z').split(':');
    let hour: u64 = clock.next()?.parse().ok()?;
    let minute: u64 = clock.next()?.parse().ok()?;
    let second: u64 = clock.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    // the day count the civil date came from, walked back out of it
    let month = i64::from(month);
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_part = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_part + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let epoch_days = era * 146_097 + day_of_era - 719_468;
    u64::try_from(epoch_days).ok().map(|d| d * 86_400 + hour * 3600 + minute * 60 + second)
}

/// The digest a read record keeps, 64 lowercase hex characters.
fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(64);
    for byte in sha256(bytes) {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// SHA-256 of `bytes` (FIPS 180-4), the digest a read record names its catalogue by. Written out
/// here because the crate has no digest dependency.
fn sha256(bytes: &[u8]) -> Vec<u8> {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
        0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
        0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
        0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
        0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
        0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let word = |part: &[u8]| part.iter().fold(0u32, |so_far, byte| (so_far << 8) | u32::from(*byte));
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] =
        [0x6a09e667u32, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut message = bytes.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(bytes.len() as u64).wrapping_mul(8).to_be_bytes());
    for block in message.chunks(64) {
        let mut w: Vec<u32> = block.chunks(4).map(word).collect();
        w.truncate(16);
        // each of the next 48 words is built from the sixteen before it, newest last
        let back =
            |words: &[u32], from_end: usize| words.iter().rev().nth(from_end.saturating_sub(1)).copied().unwrap_or(0);
        while w.len() < 64 {
            let s0 = back(&w, 15).rotate_right(7) ^ back(&w, 15).rotate_right(18) ^ (back(&w, 15) >> 3);
            let s1 = back(&w, 2).rotate_right(17) ^ back(&w, 2).rotate_right(19) ^ (back(&w, 2) >> 10);
            let next = back(&w, 16).wrapping_add(s0).wrapping_add(back(&w, 7)).wrapping_add(s1);
            w.push(next);
        }
        // eight working words, added back into the digest once the block's rounds are done
        let (mut wa, mut wb, mut wc, mut wd, mut we, mut wf, mut wg, mut wh) = (a, b, c, d, e, f, g, h);
        for (k, x) in K.iter().zip(w.iter()) {
            let s1 = we.rotate_right(6) ^ we.rotate_right(11) ^ we.rotate_right(25);
            let choice = (we & wf) ^ (!we & wg);
            let temp1 = wh.wrapping_add(s1).wrapping_add(choice).wrapping_add(*k).wrapping_add(*x);
            let s0 = wa.rotate_right(2) ^ wa.rotate_right(13) ^ wa.rotate_right(22);
            let majority = (wa & wb) ^ (wa & wc) ^ (wb & wc);
            let temp2 = s0.wrapping_add(majority);
            wh = wg;
            wg = wf;
            wf = we;
            we = wd.wrapping_add(temp1);
            wd = wc;
            wc = wb;
            wb = wa;
            wa = temp1.wrapping_add(temp2);
        }
        a = a.wrapping_add(wa);
        b = b.wrapping_add(wb);
        c = c.wrapping_add(wc);
        d = d.wrapping_add(wd);
        e = e.wrapping_add(we);
        f = f.wrapping_add(wf);
        g = g.wrapping_add(wg);
        h = h.wrapping_add(wh);
    }
    [a, b, c, d, e, f, g, h].iter().flat_map(|part| part.to_be_bytes()).collect()
}

/// A catalogue's bytes, checked against the caps and the rules, or why they were not taken.
fn parse(bytes: &[u8], saved: &Saved) -> Result<Catalogue, Refusal> {
    if bytes.len() > CATALOGUE_BYTES {
        return Err(Refusal::TooBig);
    }
    let json = std::str::from_utf8(bytes).map_err(|_| Refusal::NotJson)?;
    let root: serde_json::Value = serde_json::from_str(json).map_err(|_| Refusal::NotJson)?;
    let schema = root.get("schema").and_then(|v| v.as_u64()).ok_or(Refusal::Shape)?;
    if schema != 1 {
        return Err(Refusal::Schema(schema));
    }
    let named = root.get("store").ok_or(Refusal::Shape)?;
    let name = text(named.get("name"), NAME_CHARS).ok_or(Refusal::Shape)?;
    let publisher = text(named.get("publisher"), NAME_CHARS).unwrap_or_default();
    // required by the format, though nothing shows it: the row shows when it was read
    text(named.get("updated"), NAME_CHARS).ok_or(Refusal::Shape)?;
    let list = root.get("items").and_then(|v| v.as_array()).ok_or(Refusal::Shape)?;
    if list.len() > ITEMS_MOST {
        return Err(Refusal::TooMany(list.len()));
    }
    let mut items: Vec<StoreItem> = Vec::new();
    for value in list {
        let Some(item) = item_of(value, saved) else { continue };
        if items.iter().any(|i| i.id == item.id) {
            return Err(Refusal::DuplicateId(item.id));
        }
        items.push(item);
    }
    Ok(Catalogue { name, publisher, items })
}

/// One item, or None where it breaks a cap or a rule (the rest of the catalogue still opens).
fn item_of(value: &serde_json::Value, saved: &Saved) -> Option<StoreItem> {
    let id = text(value.get("id"), ID_CHARS)?;
    if !valid_id(&id) {
        return None;
    }
    let kind = kind_of(value.get("kind")?.as_str()?)?;
    let name = text(value.get("name"), NAME_CHARS)?;
    let version = text(value.get("version"), VERSION_CHARS)?;
    if !version_ok(&version) {
        return None;
    }
    let publisher = text(value.get("publisher"), NAME_CHARS)?;
    let description = text(value.get("description"), DESCRIPTION_CHARS)?;
    let licence = optional(value.get("licence"), LICENCE_CHARS)?.unwrap_or_default();
    let body = optional(value.get("body"), BODY_CHARS)?.unwrap_or_default();
    let requires = value
        .get("compat")
        .and_then(|c| c.get("procraft"))
        .and_then(|p| p.get("interface"))
        .and_then(|i| i.as_u64())
        .map(|number| format!("Interface {number}"))
        .unwrap_or_default();
    let named = value.get("category").and_then(|v| v.as_str()).unwrap_or_default();
    let category = CATEGORIES.iter().copied().find(|c| *c == named).map_or_else(|| "Other".to_owned(), str::to_owned);
    let tags = tags_of(value.get("tags"))?;
    let previews = links_of(value.get("previews"))?;
    let preview = value
        .get("preview")
        .and_then(|v| v.as_str())
        .map(plain)
        .filter(|link| https(link) && link.chars().count() <= LINK_CHARS)
        .unwrap_or_default();
    // every item a catalogue lists is one to fetch: the app's own catalogue is built in code
    // rather than parsed here, so an item without a file is a broken listing
    let file = text(value.get("file"), LINK_CHARS)?;
    if !https(&file) {
        return None;
    }
    let bytes = value.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
    let sha256 = value.get("sha256").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
    if bytes == 0 || !lower_hex(&sha256) {
        return None;
    }
    Some(StoreItem {
        id,
        kind,
        name,
        version,
        publisher,
        description,
        licence,
        requires,
        category,
        tags,
        preview,
        previews,
        body,
        file,
        bytes,
        sha256,
        source: saved.id.clone(),
        source_name: String::new(),
    })
}

fn kind_of(text: &str) -> Option<ItemKind> {
    match text {
        "plugin" => Some(ItemKind::Plugin),
        "brushset" => Some(ItemKind::BrushSet),
        "asset pack" => Some(ItemKind::AssetPack),
        _ => None,
    }
}

/// The catalogue's spelling of a kind, as an installed record keeps it.
fn kind_key(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Plugin => "plugin",
        ItemKind::BrushSet => "brushset",
        ItemKind::AssetPack => "asset pack",
    }
}

/// What has been installed, as it is written.
fn write_installed(list: &[Installed]) -> String {
    let items: Vec<serde_json::Value> = list
        .iter()
        .map(|one| {
            serde_json::json!({
                "id": one.id,
                "kind": kind_key(one.kind),
                "name": one.name,
                "version": one.version,
                "publisher": one.publisher,
                "source": one.source,
                "digest": one.digest,
                "at": one.at,
            })
        })
        .collect();
    serde_json::json!({ "schema": 1, "items": items }).to_string()
}

/// What has been installed, as it was written, with anything unreadable left out.
fn read_installed(text: &str) -> Vec<Installed> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { return Vec::new() };
    let Some(items) = value.get("items").and_then(|items| items.as_array()) else { return Vec::new() };
    items.iter().filter_map(installed_of).collect()
}

fn installed_of(value: &serde_json::Value) -> Option<Installed> {
    let text_of = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_owned).unwrap_or_default();
    let kind = kind_of(&text_of("kind"))?;
    Some(Installed {
        id: text_of("id"),
        kind,
        name: text_of("name"),
        version: text_of("version"),
        publisher: text_of("publisher"),
        source: text_of("source"),
        digest: text_of("digest"),
        at: value.get("at").and_then(|v| v.as_u64()).unwrap_or(0),
    })
}

/// The words a string shows as: control and bidi characters taken out, the ends trimmed.
fn plain(text: &str) -> String {
    text.chars().filter(|c| !banned(*c)).collect::<String>().trim().to_owned()
}

fn banned(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{1F}' | '\u{7F}'..='\u{9F}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}'
        | '\u{2066}'..='\u{2069}')
}

/// A required string of at most `most` characters, shown plainly.
fn text(value: Option<&serde_json::Value>, most: usize) -> Option<String> {
    let shown = plain(value?.as_str()?);
    (!shown.is_empty() && shown.chars().count() <= most).then_some(shown)
}

/// An optional string of at most `most` characters: None where it is absent, the whole item
/// dropped where it is there and too long.
fn optional(value: Option<&serde_json::Value>, most: usize) -> Option<Option<String>> {
    match value {
        None | Some(serde_json::Value::Null) => Some(None),
        Some(value) => {
            let shown = plain(value.as_str()?);
            if shown.chars().count() > most {
                return None;
            }
            Some((!shown.is_empty()).then_some(shown))
        }
    }
}

fn tags_of(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let Some(value) = value else { return Some(Vec::new()) };
    let list = value.as_array()?;
    if list.len() > TAGS_MOST {
        return None;
    }
    let mut tags = Vec::with_capacity(list.len());
    for one in list {
        let tag = plain(one.as_str()?);
        if tag.is_empty() || tag.chars().count() > TAG_CHARS {
            return None;
        }
        tags.push(tag);
    }
    Some(tags)
}

/// The pictures an item page shows: https links only, at most six, each within the link cap.
fn links_of(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let Some(value) = value else { return Some(Vec::new()) };
    let list = value.as_array()?;
    if list.len() > PREVIEWS_MOST {
        return None;
    }
    let mut links = Vec::with_capacity(list.len());
    for one in list {
        let link = plain(one.as_str()?);
        if link.chars().count() > LINK_CHARS {
            return None;
        }
        if https(&link) {
            links.push(link);
        }
    }
    Some(links)
}

fn https(link: &str) -> bool {
    link.to_lowercase().starts_with("https://")
}

fn lower_hex(sha256: &str) -> bool {
    sha256.len() == 64 && sha256.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// An id: two or more dot-separated parts of lowercase letters, digits and hyphens, each starting
/// and ending with a letter or a digit, and never in the app's own namespace.
fn valid_id(id: &str) -> bool {
    let chars = id.chars().count();
    if chars == 0 || chars > ID_CHARS || id.starts_with("procraft.") {
        return false;
    }
    let parts: Vec<&str> = id.split('.').collect();
    parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
                && part.chars().last().is_some_and(|c| c.is_ascii_alphanumeric())
                && part.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        })
}

/// Three numbers separated by dots; a `-` label after them is a name and is ignored, while build
/// metadata is not a version at all.
fn version_ok(version: &str) -> bool {
    if version.chars().count() > VERSION_CHARS || version.contains('+') {
        return false;
    }
    let numbers = version.split('-').next().unwrap_or_default();
    let parts: Vec<&str> = numbers.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()) && part.parse::<u32>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real thing: a catalogue whose file is a live https address. The bytes are fetched over
    /// the network, checked against the size and digest the catalogue gave, read as a brush set and
    /// kept as installed. Ignored by default because it needs a connection:
    /// `cargo test -p procraft-core --lib a_real_catalogue_fetches_and_reads_it -- --ignored`.
    #[test]
    #[ignore = "needs the network"]
    fn a_real_catalogue_fetches_and_reads_it() {
        let dir = dir("real-fetch");
        let catalogue = r#"{
  "schema": 1,
  "store": { "name": "Live Catalogue", "publisher": "R. Halloway", "updated": "2026-11-22" },
  "items": [{
    "id": "com.example.inking",
    "kind": "brushset",
    "name": "Inking",
    "publisher": "R. Halloway",
    "version": "1.0.0",
    "description": "A real brush set, served over https.",
    "category": "Brushes",
    "file": "https://raw.githubusercontent.com/artcraft-android/procraft/main/legacy-brushes/Inking.brushset",
    "bytes": 279872,
    "sha256": "74644b4c85847f54afa10939bde473a9fe423e7707c29be2cfbf068e41a544f2"
  }]
}"#;
        let at = dir.join("live.json");
        std::fs::write(&at, catalogue).expect("write the catalogue");
        let store = Store::open(dir.display().to_string());
        assert_eq!(store.add_source(at.display().to_string()), None);
        let fetched = store.fetch_item("com.example.inking".to_owned());
        assert!(fetched.failure.is_empty(), "the fetch said: {}", fetched.failure);
        assert_eq!(fetched.bytes.len(), 279872, "the catalogue's size");
        let digest = digest_of(&fetched.bytes);
        assert_eq!(digest, "74644b4c85847f54afa10939bde473a9fe423e7707c29be2cfbf068e41a544f2");
        let read = crate::brush_files::read_any_brushes(&fetched.bytes, Some("Inking.brushset".to_owned()));
        assert!(read.is_some(), "the fetched bytes read as a brush set");
        assert!(store.keep_installed("com.example.inking".to_owned(), digest));
        assert_eq!(
            store.installed_one("com.example.inking".to_owned()).map(|one| one.version),
            Some("1.0.0".to_owned())
        );
    }

    fn dir(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("procraft-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    fn saved(address: &str) -> Saved {
        Saved { id: "a1b2c3".into(), name: String::new(), address: address.into(), on: true }
    }

    const CATALOGUE: &str = r#"{
      "schema": 1,
      "store": { "name": "Ink and Grain", "publisher": "R. Halloway", "updated": "2026-03-08" },
      "items": [
        { "id": "org.example.dry-ink", "kind": "brushset", "name": "Dry Ink", "version": "2.1.0",
          "publisher": "R. Halloway", "description": "Six brushes that break up at the edge.",
          "file": "https://example.org/dry-ink.brushset", "bytes": 1844221,
          "sha256": "d7d08aaae59a5af97fd282f230408dce12ff0ee3b1d35fa443240cea248328e2",
          "licence": "CC-BY-4.0", "category": "Brushes", "tags": ["ink", "rough"] },
        { "id": "org.example.nudge", "kind": "plugin", "name": "Nudge", "version": "1.0.0",
          "publisher": "Example", "description": "Moves the selection a pixel at a time.",
          "file": "https://example.org/nudge.procraft-plugin", "bytes": 48213,
          "sha256": "b2120ab63b12230656f459714a385cf47fb71eb3d017667e56d4b9cac6260631",
          "compat": { "procraft": { "interface": 2 } },
          "licence": "CC0-1.0", "category": "Interface", "tags": ["selection", "keys"] }
      ]
    }"#;

    fn catalogue_file(dir: &Path, name: &str, body: &str) -> String {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path.to_string_lossy().into_owned()
    }

    /// A transport that answers from a table, so no test ever touches a socket.
    struct Fake(std::collections::HashMap<String, Result<Vec<u8>, String>>);

    impl crate::fetch::Transport for Fake {
        fn get(&self, address: &str, cap: usize, _timeout: std::time::Duration) -> Result<Vec<u8>, String> {
            match self.0.get(address) {
                Some(Ok(bytes)) if bytes.len() <= cap => Ok(bytes.clone()),
                Some(Ok(_)) => Err(format!("it is larger than the {cap} bytes it may be")),
                Some(Err(why)) => Err(why.clone()),
                None => Err("it could not be reached: no such address".to_owned()),
            }
        }
    }

    /// A store whose web addresses answer from `answers`: the address, then what it says.
    fn web_store(name: &str, answers: Vec<(&str, Result<&str, &str>)>) -> (Arc<Store>, PathBuf) {
        let store = dir(name);
        let table = answers
            .into_iter()
            .map(|(address, answer)| {
                (address.to_owned(), answer.map(|text| text.as_bytes().to_vec()).map_err(str::to_owned))
            })
            .collect();
        let opened = Store::open_with(store.to_string_lossy().into_owned(), Arc::new(Fake(table)));
        (opened, store)
    }

    #[test]
    fn a_catalogue_parses_its_items_and_names_itself() {
        let parsed = parse(CATALOGUE.as_bytes(), &saved("x")).unwrap();
        assert_eq!(parsed.name, "Ink and Grain");
        assert_eq!(parsed.items.len(), 2);
        let first = parsed.items.first().unwrap();
        assert_eq!(
            (first.id.as_str(), first.kind, first.version.as_str()),
            ("org.example.dry-ink", ItemKind::BrushSet, "2.1.0")
        );
        assert_eq!((first.category.as_str(), first.tags.len()), ("Brushes", 2));
        assert_eq!(parsed.items.get(1).unwrap().kind, ItemKind::Plugin);
    }

    #[test]
    fn a_catalogue_that_breaks_a_cap_is_refused_whole() {
        assert_eq!(parse(b"not json", &saved("x")).unwrap_err(), Refusal::NotJson);
        assert_eq!(
            parse(br#"{"schema": 2, "store": {"name": "x"}, "items": []}"#, &saved("x")).unwrap_err(),
            Refusal::Schema(2)
        );
        assert_eq!(parse(br#"{"schema": 1, "items": []}"#, &saved("x")).unwrap_err(), Refusal::Shape);
        let big = format!(r#"{{"schema": 1, "store": {{"name": "{}"}}, "items": []}}"#, "x".repeat(CATALOGUE_BYTES));
        assert_eq!(parse(big.as_bytes(), &saved("x")).unwrap_err(), Refusal::TooBig);
        let twice = CATALOGUE.replace("org.example.nudge", "org.example.dry-ink");
        assert_eq!(
            parse(twice.as_bytes(), &saved("x")).unwrap_err(),
            Refusal::DuplicateId("org.example.dry-ink".into())
        );
    }

    #[test]
    fn an_item_over_its_own_cap_is_dropped_and_the_rest_of_the_catalogue_opens() {
        let cases = [
            CATALOGUE.replace(r#""version": "2.1.0""#, r#""version": "2.1""#),
            CATALOGUE.replace("org.example.dry-ink", "Dry-ink"),
            CATALOGUE.replace(
                r#""tags": ["ink", "rough"]"#,
                r#""tags": ["ink", "rough", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k"]"#,
            ),
            CATALOGUE.replace(
                r#""sha256": "d7d08aaae59a5af97fd282f230408dce12ff0ee3b1d35fa443240cea248328e2""#,
                r#""sha256": "not-a-digest""#,
            ),
            CATALOGUE.replace(r#""file": "https://example.org/dry-ink.brushset""#, r#""file": "http://example.org/x""#),
        ];
        for (at, case) in cases.iter().enumerate() {
            let parsed = parse(case.as_bytes(), &saved("x")).unwrap();
            assert_eq!(parsed.items.len(), 1, "case {at} drops the broken item");
            assert_eq!(parsed.items.first().unwrap().name, "Nudge", "case {at} keeps the other");
        }
    }

    #[test]
    fn a_category_outside_the_set_falls_back_to_other() {
        let odd = CATALOGUE.replace(r#""category": "Brushes""#, r#""category": "Brushes and more""#);
        let parsed = parse(odd.as_bytes(), &saved("x")).unwrap();
        assert_eq!(parsed.items.first().unwrap().category, "Other");
    }

    /// The items the store lists from one source, whatever else it lists.
    fn from(store: &Store, source: &str) -> Vec<StoreItem> {
        store.items().into_iter().filter(|i| i.source == source).collect()
    }

    /// The source a test added, the one that ships with the app aside.
    fn added(store: &Store) -> Option<SourceInfo> {
        store.sources().into_iter().find(|s| s.id != SHIPPED)
    }

    #[test]
    fn a_source_is_added_from_a_file_and_drawn_with_the_name_it_gives() {
        let store = dir("add");
        let path = catalogue_file(&store, "catalogue.json", CATALOGUE);
        let s = Store::open(store.to_string_lossy().into_owned());
        assert_eq!(s.add_source(path), None);
        let one = added(&s).unwrap();
        assert_eq!((one.name.as_str(), one.by.as_str()), ("Ink and Grain", "R. Halloway"));
        assert_eq!(from(&s, &one.id).len(), 2);
        // and it is all still there when the store opens again, with the copy it kept
        let again = Store::open(store.to_string_lossy().into_owned());
        let one = added(&again).unwrap();
        assert_eq!(from(&again, &one.id).len(), 2);
        assert!(one.read > 0);
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_catalogue_that_will_not_read_is_refused_by_add_and_nothing_is_kept() {
        let store = dir("refuse");
        let path = catalogue_file(&store, "bad.json", "{}");
        let s = Store::open(store.to_string_lossy().into_owned());
        assert_eq!(s.add_source(path), Some("its shape is not a catalogue".to_owned()));
        assert!(added(&s).is_none());
        assert!(s.items().iter().all(|i| i.source == SHIPPED));
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_source_switched_off_takes_its_items_out_and_switching_it_back_brings_them_in() {
        let store = dir("off");
        let path = catalogue_file(&store, "catalogue.json", CATALOGUE);
        let s = Store::open(store.to_string_lossy().into_owned());
        assert_eq!(s.add_source(path), None);
        let id = added(&s).unwrap().id;
        s.set_on(id.clone(), false);
        assert!(from(&s, &id).is_empty());
        s.set_on(id.clone(), true);
        assert_eq!(from(&s, &id).len(), 2);
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn one_id_with_one_digest_is_one_item_and_two_digests_are_two() {
        let store = dir("merge");
        let one = catalogue_file(&store, "one.json", CATALOGUE);
        let two = catalogue_file(&store, "two.json", &CATALOGUE.replace("d7d08aaa", "a7d08aaa"));
        let s = Store::open(store.to_string_lossy().into_owned());
        assert_eq!(s.add_source(one), None);
        assert_eq!(s.add_source(two), None);
        // the same bytes from two sources draw once; the edited digest draws again
        let ids: Vec<String> = s.sources().into_iter().map(|x| x.id).filter(|id| id != SHIPPED).collect();
        assert_eq!(ids.len(), 2);
        let mine: Vec<StoreItem> = s.items().into_iter().filter(|i| ids.contains(&i.source)).collect();
        assert_eq!(mine.len(), 3);
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn search_reads_the_name_the_publisher_and_the_tags() {
        let store = dir("search");
        let path = catalogue_file(&store, "catalogue.json", CATALOGUE);
        let s = Store::open(store.to_string_lossy().into_owned());
        assert_eq!(s.add_source(path), None);
        let id = added(&s).unwrap().id;
        let found = |query: &str| s.search(query.to_owned()).into_iter().filter(|i| i.source == id).count();
        assert_eq!(found("dry"), 1);
        assert_eq!(found("example"), 1);
        assert_eq!(found("INK"), 1);
        assert_eq!(found("nothing here"), 0);
        assert!(s.search("  ".to_owned()).is_empty());
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn removing_a_source_forgets_it_and_its_copy() {
        let store = dir("remove");
        let path = catalogue_file(&store, "catalogue.json", CATALOGUE);
        let s = Store::open(store.to_string_lossy().into_owned());
        assert_eq!(s.add_source(path), None);
        let id = added(&s).unwrap().id;
        assert!(store.join(&id).exists());
        s.remove_source(id.clone());
        assert!(added(&s).is_none());
        assert!(from(&s, &id).is_empty());
        assert!(!store.join(&id).exists());
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_web_address_is_read_and_its_catalogue_lists() {
        let (s, store) = web_store("web", vec![("https://example.org/catalogue.json", Ok(CATALOGUE))]);
        assert_eq!(s.add_source("https://example.org/catalogue.json".into()), None);
        let source = added(&s).unwrap();
        assert_eq!(source.name, "Ink and Grain", "the catalogue names itself");
        assert_eq!(source.failure, "");
        assert!(source.read > 0, "it was read");
        assert_eq!(from(&s, &source.id).len(), 2, "both of its items list");
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_web_address_that_does_not_answer_is_not_added_and_says_why() {
        let (s, store) = web_store("web-fail", vec![("https://example.org/catalogue.json", Err("it answers 404"))]);
        assert_eq!(s.add_source("https://example.org/catalogue.json".into()), Some("it answers 404".to_owned()));
        assert!(added(&s).is_none(), "nothing that could not be read is added");
        // plain http never reaches the transport at all
        assert!(s.add_source("http://example.org/catalogue.json".into()).is_some());
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn reading_a_source_again_brings_the_new_listing_or_the_reason_it_did_not() {
        let (s, store) = web_store("web-again", vec![("https://example.org/catalogue.json", Ok(CATALOGUE))]);
        assert_eq!(s.add_source("https://example.org/catalogue.json".into()), None);
        assert_eq!(s.refresh(added(&s).unwrap().id), None, "the address still answers");
        assert_eq!(added(&s).unwrap().failure, "");
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn an_item_is_fetched_and_the_shipped_ones_have_nothing_to_fetch() {
        let (s, store) = web_store(
            "web-item",
            vec![
                ("https://example.org/catalogue.json", Ok(CATALOGUE)),
                ("https://example.org/nudge.procraft-plugin", Ok("plugin bytes")),
            ],
        );
        assert_eq!(s.add_source("https://example.org/catalogue.json".into()), None);
        let fetched = s.fetch_item("org.example.nudge".into());
        assert_eq!(fetched.failure, "");
        assert_eq!(fetched.bytes, b"plugin bytes".to_vec());
        assert!(!s.fetch_item("org.example.nowhere".into()).failure.is_empty(), "an unknown id is refused");
        let shipped = s.items().into_iter().find(|item| item.file.is_empty());
        assert!(shipped.is_some(), "the shipped source lists something");
        if let Some(item) = shipped {
            assert!(!s.fetch_item(item.id).failure.is_empty(), "a bundled item is not fetched");
        }
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_picture_is_fetched_and_a_plain_http_one_is_refused() {
        let (s, store) = web_store("web-picture", vec![("https://example.org/a.png", Ok("png"))]);
        assert_eq!(s.fetch_picture("https://example.org/a.png".into()).bytes, b"png".to_vec());
        assert!(!s.fetch_picture("http://example.org/a.png".into()).failure.is_empty());
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_source_that_goes_missing_keeps_its_copy_and_says_why_it_is_stale() {
        let store = dir("stale");
        let path = catalogue_file(&store, "catalogue.json", CATALOGUE);
        let s = Store::open(store.to_string_lossy().into_owned());
        assert_eq!(s.add_source(path.clone()), None);
        let id = added(&s).unwrap().id;
        std::fs::remove_file(&path).unwrap();
        let again = Store::open(store.to_string_lossy().into_owned());
        assert_eq!(from(&again, &id).len(), 2, "the copy it had is still drawn");
        let stale = again.sources().into_iter().find(|s| s.id == id).unwrap();
        assert!(stale.failure.starts_with("it could not be read"));
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn the_shipped_source_lists_the_plugins_that_come_with_the_app() {
        let store = dir("shipped");
        let s = Store::open(store.to_string_lossy().into_owned());
        let shipped = s.sources().into_iter().find(|x| x.id == SHIPPED).unwrap();
        assert_eq!((shipped.name.as_str(), shipped.by.as_str(), shipped.on), ("ProCraft Store", "ProCraft", true));
        let items = from(&s, SHIPPED);
        assert_eq!(items.len(), crate::bundled::BUNDLED.len(), "every bundled file is listed");
        assert!(items.iter().all(|i| i.kind == ItemKind::Plugin && i.file.is_empty()));
        assert!(items.iter().any(|i| i.category == "Brushes"));
        assert!(items.iter().any(|i| i.category == "Other"));
        // it cannot be removed, and switching it off empties the store and sticks
        s.remove_source(SHIPPED.into());
        assert!(s.sources().iter().any(|x| x.id == SHIPPED));
        s.set_on(SHIPPED.into(), false);
        assert!(s.items().is_empty());
        let again = Store::open(store.to_string_lossy().into_owned());
        assert!(again.sources().iter().any(|x| x.id == SHIPPED && !x.on));
        let _ = std::fs::remove_dir_all(&store);
    }

    #[test]
    fn a_digest_is_the_one_a_catalogue_writes() {
        assert_eq!(digest_of(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(digest_of(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    #[test]
    fn an_install_is_recorded_and_survives_a_reopen() {
        let dir = std::env::temp_dir().join(format!("procraft-store-installed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.to_string_lossy().into_owned();
        let store = Store::open(path.clone());
        let items = store.items();
        assert!(!items.is_empty(), "the shipped source lists something");
        let Some(item) = items.into_iter().next() else { return };
        assert!(store.installed().is_empty());
        assert!(store.keep_installed(item.id.clone(), "abc".to_owned()));
        let Some(one) = store.installed_one(item.id.clone()) else { return };
        assert_eq!(one.name, item.name);
        assert_eq!(one.kind, item.kind);
        assert_eq!(one.source, item.source);
        assert_eq!(one.digest, "abc");
        assert!(
            !store.keep_installed("com.nobody.nothing".to_owned(), "abc".to_owned()),
            "nothing is recorded about an item no source lists"
        );
        // the record is still there when the store is opened again
        let again = Store::open(path);
        assert_eq!(again.installed().len(), 1);
        assert!(again.forget_installed(item.id.clone()));
        assert!(!again.forget_installed(item.id), "forgetting what is not there is not an error");
        assert!(again.installed().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_details_file_carries_what_a_listing_needs() {
        let tags = vec!["ink".to_owned(), "rough".to_owned()];
        let pictures = vec!["preview.png".to_owned()];
        let text = package_details(
            "brushset",
            "Dry Ink",
            "R. Halloway",
            "Six brushes.",
            "Brushes",
            "CC BY 4.0",
            &tags,
            &pictures,
            "2026-11-22T10:00:00Z",
        );
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value.get("schema").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(value.get("kind").and_then(|v| v.as_str()), Some("brushset"));
        assert_eq!(value.get("name").and_then(|v| v.as_str()), Some("Dry Ink"));
        assert_eq!(value.get("tags").and_then(|v| v.as_array()).map(Vec::len), Some(2));
        assert_eq!(value.get("pictures").and_then(|v| v.as_array()).map(Vec::len), Some(1));
        assert_eq!(value.get("licence").and_then(|v| v.as_str()), Some("CC BY 4.0"));
    }

    #[test]
    fn a_date_is_written_and_read_back_the_way_a_read_record_keeps_it() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(1_772_000_000), "2026-02-25T06:13:20Z");
        assert_eq!(from_iso("2026-02-25T06:13:20Z"), Some(1_772_000_000));
        assert_eq!(from_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(from_iso("not a date"), None);
    }

    #[test]
    fn the_digest_is_the_one_a_catalogue_is_named_by() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }
}
