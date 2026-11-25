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
const PENDING: &str = "pending-brushes";
const PAYLOAD: &str = "payload";
const DECODED_BRUSHES: &str = "decoded-brushes-v1";
const LISTING: &str = "listing.json";
static PENDING_WRITES: Mutex<()> = Mutex::new(());

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
pub const STORE_SPECIMEN_LIMIT: usize = 6;
pub const STORE_MAKER_IMAGE_LIMIT: usize = 4;
pub(crate) const STORE_CONTENTS_LIMIT: usize = 128;
pub(crate) const PREVIEWS_MOST: usize = 1 + STORE_CONTENTS_LIMIT + STORE_MAKER_IMAGE_LIMIT;
const ITEM_BYTES: usize = 256 * 1024 * 1024;
const PLUGIN_BYTES: usize = 32 * 1024 * 1024;
const TAGS_MOST: usize = 12;
const TAG_CHARS: usize = 32;

/// The categories a catalogue may name, and the one a value outside them falls back to.
pub const CATEGORIES: [&str; 8] =
    ["Brushes", "Papers and Textures", "Colour", "Layers", "Interface", "Effects", "Canvas", "Other"];

/// What a catalogue says an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, uniffi::Enum)]
pub enum ItemKind {
    Plugin,
    BrushSet,
    AssetPack,
    Brush,
    BrushLibrary,
}

impl ItemKind {
    /// The words the interface shows for it.
    pub fn text(self) -> &'static str {
        match self {
            ItemKind::Plugin => "Plugin",
            ItemKind::BrushSet => "Brush set",
            ItemKind::Brush => "Brush",
            ItemKind::BrushLibrary => "Brush library",
            ItemKind::AssetPack => "Asset pack",
        }
    }

    pub fn browse_group(self) -> Self {
        match self {
            Self::Brush | Self::BrushLibrary => Self::BrushSet,
            other => other,
        }
    }

    pub fn supports_install(self) -> bool {
        matches!(self, Self::Plugin | Self::BrushSet)
    }
}

/// One item a catalogue lists, as the store draws it. An empty string is a field the catalogue
/// left out.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, uniffi::Record)]
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
    /// Separate static square artwork for compact listing surfaces.
    #[serde(default)]
    pub icon: String,
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

impl StoreItem {
    /// The cover followed by the listing's gallery, in order and with duplicates removed.
    pub fn gallery(&self) -> Vec<&str> {
        let mut gallery = Vec::new();
        for address in std::iter::once(&self.preview).chain(&self.previews) {
            if !address.is_empty() && !gallery.contains(&address.as_str()) {
                gallery.push(address.as_str());
                if gallery.len() == PREVIEWS_MOST {
                    break;
                }
            }
        }
        gallery
    }
}

#[uniffi::export]
pub fn store_gallery(item: StoreItem) -> Vec<String> {
    item.gallery().into_iter().map(str::to_owned).collect()
}

/// Selects up to five listings for a UTC day, independently of catalogue order.
#[uniffi::export]
pub fn store_featured_order(listings: Vec<String>, day: u64) -> Vec<u32> {
    let mut order: Vec<_> = listings.iter().enumerate().collect();
    order.sort_by_cached_key(|(_, listing)| {
        let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
        hash.update(&day.to_le_bytes());
        hash.update(listing.as_bytes());
        let digest = hash.finish();
        let rank = digest.as_ref().first_chunk::<8>().copied().unwrap_or_default();
        (rank, *listing)
    });
    order.into_iter().take(5).map(|(index, _)| index as u32).collect()
}

/// What a fetch brought back: the bytes when it worked, and why when it did not.
#[derive(Clone, Debug, uniffi::Record)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub failure: String,
}

/// The exact plugin package presented for approval.
#[derive(uniffi::Object)]
pub struct StorePluginReview {
    name: String,
    message: String,
    item: StoreItem,
    source_address: String,
    previous: Option<crate::plugins::StorePluginState>,
    plugins: Arc<crate::Plugins>,
    package: Mutex<Option<crate::plugin_file::Package>>,
}

#[uniffi::export]
impl StorePluginReview {
    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn message(&self) -> String {
        self.message.clone()
    }

    pub fn is_update(&self) -> bool {
        self.previous.is_some()
    }
}

#[derive(uniffi::Record)]
pub struct StorePluginInspection {
    pub review: Option<Arc<StorePluginReview>>,
    pub failure: Option<String>,
}

impl StorePluginInspection {
    fn from_result(result: Result<StorePluginReview, String>) -> Self {
        match result {
            Ok(review) => Self { review: Some(Arc::new(review)), failure: None },
            Err(failure) => Self { review: None, failure: Some(failure) },
        }
    }
}

/// What the store installed: what its card may say, and the digest an update is told by.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, uniffi::Record)]
pub struct Installed {
    pub id: String,
    pub kind: ItemKind,
    pub name: String,
    pub version: String,
    pub publisher: String,
    /// The source it came from, empty when it came with the app.
    pub source: String,
    /// A fingerprint of the approved catalogue address, without storing any credentials.
    #[serde(default)]
    pub source_pin: String,
    /// The digest of the file that was taken, which a second copy under the same id changes.
    pub digest: String,
    /// Seconds since the epoch.
    pub at: u64,
}

impl Installed {
    pub(crate) fn from_item(item: &StoreItem) -> Self {
        Self {
            id: item.id.clone(),
            kind: item.kind,
            name: item.name.clone(),
            version: item.version.clone(),
            publisher: item.publisher.clone(),
            source: item.source.clone(),
            source_pin: String::new(),
            digest: item.sha256.clone(),
            at: now(),
        }
    }

    pub(crate) fn matches(&self, item: &StoreItem) -> bool {
        self.id == item.id && self.source == item.source
    }
}

/// The outcome of taking one queued brush set into the saved library.
#[derive(uniffi::Record)]
pub struct StoreBrushResult {
    pub item: StoreItem,
    pub failure: Option<String>,
}

#[uniffi::export]
pub fn apply_queued_store_brushes(dir: String, session: Arc<crate::CanvasSession>) -> Vec<StoreBrushResult> {
    let _writing = PENDING_WRITES.lock().unwrap_or_else(|error| error.into_inner());
    pending_at(Path::new(&dir))
        .into_iter()
        .map(|(folder, item)| {
            let decoded = folder.join(DECODED_BRUSHES);
            let failure = if decoded.is_file() {
                let set = std::fs::File::open(decoded)
                    .map(std::io::BufReader::new)
                    .map_err(|error| error.to_string())
                    .and_then(crate::brush_library::read_decoded_store_set);
                session.install_decoded_store_brushes(item.clone(), set)
            } else {
                session.install_store_brushes(item.clone(), folder.join(PAYLOAD).to_string_lossy().into_owned())
            };
            if let Err(error) = std::fs::remove_dir_all(&folder) {
                log::warn!("removing a processed Store brush set: {error}");
            }
            StoreBrushResult { item, failure }
        })
        .collect()
}

fn pending_at(dir: &Path) -> Vec<(PathBuf, StoreItem)> {
    let Ok(entries) = std::fs::read_dir(dir.join(PENDING)) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|entry| {
            if entry.file_name().to_string_lossy().starts_with('.') || !entry.file_type().ok()?.is_dir() {
                return None;
            }
            let folder = entry.path();
            let bytes = std::fs::read(folder.join(LISTING)).ok()?;
            Some((folder, serde_json::from_slice(&bytes).ok()?))
        })
        .collect()
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
    local_network: bool,
}

/// One source held: what was saved, the catalogue last read from it, and how that read went.
#[derive(Clone, Debug)]
struct Held {
    saved: Saved,
    catalogue: Option<Catalogue>,
    read: u64,
    failure: String,
    refresh: Arc<()>,
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
    source_writes: Mutex<()>,
    /// What has been installed from the store, by catalogue id.
    installed: Mutex<Vec<Installed>>,
    installed_brushes: Mutex<Vec<Installed>>,
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
        let installed = installed_at(&root).into_iter().filter(|one| one.kind != ItemKind::BrushSet).collect();
        Store::with_transport(&root, sources_at(&root), installed, std::sync::Arc::new(crate::fetch::Web))
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
        list.extend(self.brush_records().iter().cloned());
        list.sort_by_key(|one| std::cmp::Reverse(one.at));
        list
    }

    /// One installed item, including the catalogue that supplied it.
    pub fn installed_listing(&self, source: String, id: String) -> Option<Installed> {
        let matches = |one: &&Installed| one.source == source && one.id == id;
        self.brush_records().iter().find(matches).cloned().or_else(|| self.records().iter().find(matches).cloned())
    }

    /// Rereads the brush installations in the persisted libraries.
    pub fn read_brush_installs(&self, dir: String) {
        *self.brush_records() = crate::brush_libraries::installed_in(Path::new(&dir));
    }

    /// Reads brush-install state from an open library, including changes awaiting the next save.
    pub fn sync_brush_installs(&self, session: Arc<crate::CanvasSession>) {
        self.follow_brush_installs(&session);
    }

    /// Forgets a specific installed listing.
    pub fn forget_listing(&self, source: String, id: String) -> bool {
        let mut list = self.records();
        let kept: Vec<_> = list.iter().filter(|one| one.source != source || one.id != id).cloned().collect();
        if kept.len() == list.len() || !self.write_installed_list(&kept) {
            return false;
        }
        *list = kept;
        true
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

    /// Adds a local or web catalogue. None when it was added, or the reason it was not.
    pub fn add_source(&self, address: String) -> Option<String> {
        self.add_source_with_local_network(address, false)
    }

    /// Allows this catalogue and links on its exact HTTPS origin to reach its local host.
    pub fn add_source_with_local_network(&self, address: String, local_network: bool) -> Option<String> {
        let address = address.trim().to_owned();
        let local_network = local_network && crate::fetch::web_address(&address).is_ok();
        if address.is_empty() {
            return Some("the address is empty".to_owned());
        }
        let already_added = |held: &[Held]| held.iter().any(|h| h.saved.address == address);
        if already_added(&self.held()) {
            return Some("that address is already added".to_owned());
        }
        let mut saved = Saved { address: address.clone(), on: true, local_network, ..Saved::default() };
        let (mut catalogue, bytes) = match self.read(&saved) {
            Ok(read) => read,
            Err(reason) => return Some(reason),
        };
        let _writing = self.writing_sources();
        {
            let held = self.held();
            if already_added(&held) {
                return Some("that address is already added".to_owned());
            }
            saved.id = fresh_id(&address, &held);
        }
        for item in &mut catalogue.items {
            item.source.clone_from(&saved.id);
        }
        self.keep(&saved, &bytes, &catalogue);
        self.held().push(Held {
            saved,
            catalogue: Some(catalogue),
            read: now(),
            failure: String::new(),
            refresh: Arc::new(()),
        });
        self.keep_sources();
        None
    }

    /// Switches a source on or off: off hides its items and stops it being read.
    pub fn set_on(&self, id: String, on: bool) {
        let _writing = self.writing_sources();
        {
            let mut held = self.held();
            let Some(h) = held.iter_mut().find(|h| h.saved.id == id) else { return };
            h.saved.on = on;
        }
        self.keep_sources();
    }

    /// Removes a known source and its cache. It leaves what was installed from it.
    pub fn remove_source(&self, id: String) {
        let Some(dir) = self.cache_dir(&id) else { return };
        let _writing = self.writing_sources();
        {
            let mut held = self.held();
            let Some(at) = held.iter().position(|h| h.saved.id == id) else { return };
            held.remove(at);
        }
        self.keep_sources();
        if let (Some(root), Ok(target)) = (&self.dir, dir.canonicalize())
            && root
                .canonicalize()
                .is_ok_and(|root| target.parent() == Some(root.as_path()) && target.file_name() == dir.file_name())
        {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Reads a catalogue again. A failed refresh leaves the last successful copy available.
    pub fn refresh(&self, id: String) -> Option<String> {
        if id == SHIPPED {
            return None;
        }
        let token = Arc::new(());
        let saved = {
            let _writing = self.writing_sources();
            let mut held = self.held();
            let Some(h) = held.iter_mut().find(|h| h.saved.id == id) else {
                return Some("that source is gone".to_owned());
            };
            h.refresh = Arc::clone(&token);
            h.saved.clone()
        };
        let read = self.read(&saved);
        let _writing = self.writing_sources();
        {
            let held = self.held();
            let Some(h) = held.iter().find(|h| h.saved.id == id) else {
                return Some("that source is gone".to_owned());
            };
            if !Arc::ptr_eq(&h.refresh, &token) {
                return None;
            }
        }
        match &read {
            Ok((catalogue, bytes)) => self.keep(&saved, bytes, catalogue),
            Err(reason) => self.keep_read_failure(&saved.id, reason),
        }
        let mut held = self.held();
        let h = held.iter_mut().find(|h| h.saved.id == id)?;
        match read {
            Ok((catalogue, _)) => {
                h.catalogue = Some(catalogue);
                h.read = now();
                h.failure.clear();
                None
            }
            Err(reason) => {
                h.failure.clone_from(&reason);
                Some(reason)
            }
        }
    }

    /// When a source's catalogue was last read, in seconds since the epoch.
    pub fn read_at(&self, id: String) -> u64 {
        self.held().iter().find(|h| h.saved.id == id).map_or(0, |h| h.read)
    }

    /// Downloads the exact selected listing to a new file and checks its size and digest.
    /// The caller owns the file after success. An unsuccessful download leaves no file.
    pub fn download_listing(&self, item: StoreItem, path: String) -> Option<String> {
        let Some(listed) = self.listing(&item.source, &item.id).filter(|listed| {
            listed.sha256 == item.sha256
                && listed.kind == item.kind
                && listed.version == item.version
                && listed.bytes == item.bytes
        }) else {
            return Some("the listing has changed; open it again".to_owned());
        };
        let result = (|| {
            crate::fetch::web_address(&listed.file)?;
            let cap = listing_size(&listed)?;
            let path = Path::new(&path);
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .map_err(|e| format!("the download could not be staged: {e}"))?;
            let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
            let result = self
                .transport
                .get_into_scoped(
                    &listed.file,
                    cap,
                    crate::fetch::FETCH_TIME,
                    self.local_origin(&listed.source).as_deref(),
                    &mut HashWriter { inner: &mut file, digest: &mut digest },
                )
                .and_then(|size| check_listing(&listed, size as u64, digest.finish().as_ref()));
            drop(file);
            if result.is_err() {
                let _ = std::fs::remove_file(path);
            }
            result
        })();
        result.err()
    }

    /// Downloads a brush set into the queue used when no Android canvas is open.
    pub fn queue_brush_listing(&self, item: StoreItem) -> Option<String> {
        if item.kind != ItemKind::BrushSet {
            return Some("Choose a brush set to add to the library.".to_owned());
        }
        let result = (|| {
            let dir = self.dir.as_ref().ok_or("the Store folder is unavailable")?.join(PENDING);
            std::fs::create_dir_all(&dir).map_err(|e| format!("the download couldn't be queued: {e}"))?;
            let stage = tempfile::Builder::new()
                .prefix(".download-")
                .tempdir_in(&dir)
                .map_err(|e| format!("the download couldn't be staged: {e}"))?;
            if let Some(error) =
                self.download_listing(item.clone(), stage.path().join(PAYLOAD).to_string_lossy().into_owned())
            {
                return Err(error);
            }
            let payload = stage.path().join(PAYLOAD);
            let mut decoded = crate::store_decode::decode(crate::StoreDecodeKind::BrushSet, &payload)?;
            let mut prepared = std::fs::File::create(stage.path().join(DECODED_BRUSHES)).map_err(|e| e.to_string())?;
            std::io::copy(&mut decoded, &mut prepared).map_err(|e| e.to_string())?;
            prepared.sync_all().map_err(|e| e.to_string())?;
            drop(prepared);
            std::fs::remove_file(payload).map_err(|e| e.to_string())?;
            let listing = serde_json::to_vec(&item).map_err(|e| e.to_string())?;
            std::fs::write(stage.path().join(LISTING), listing)
                .map_err(|e| format!("the download couldn't be queued: {e}"))?;
            let _writing = PENDING_WRITES.lock().unwrap_or_else(|error| error.into_inner());
            let key = sha256_hex(format!("{}:{}", item.source, item.id).as_bytes());
            let target = dir.join(key);
            if target.exists() {
                return Ok(());
            }
            std::fs::rename(stage.path(), &target).map_err(|e| format!("the download couldn't be queued: {e}"))
        })();
        result.err()
    }

    pub fn queued_brushes(&self) -> Vec<StoreItem> {
        self.dir.as_deref().map_or_else(Vec::new, |dir| pending_at(dir).into_iter().map(|(_, item)| item).collect())
    }

    /// Downloads and inspects a plugin without installing it.
    pub fn review_plugin_listing(&self, item: StoreItem, plugins: Arc<crate::Plugins>) -> StorePluginInspection {
        let result = (|| {
            self.plugin_source(&item)?;
            let dir = self.dir.as_ref().ok_or("The Store folder is unavailable.")?;
            let stage =
                tempfile::tempdir_in(dir).map_err(|error| format!("The download couldn't be staged: {error}"))?;
            let path = stage.path().join(PAYLOAD);
            if let Some(reason) = self.download_listing(item.clone(), path.to_string_lossy().into_owned()) {
                return Err(reason);
            }
            self.prepare_plugin_review(item, plugins, &path)
        })();
        StorePluginInspection::from_result(result)
    }

    /// Inspects a manually selected file using the same checks as a Store download.
    pub fn review_plugin_file(
        &self,
        item: StoreItem,
        plugins: Arc<crate::Plugins>,
        path: String,
    ) -> StorePluginInspection {
        StorePluginInspection::from_result(self.prepare_plugin_review(item, plugins, Path::new(&path)))
    }

    /// Installs the exact plugin presented for approval, after the user confirms the review.
    pub fn approve_plugin_listing(&self, approval: Arc<StorePluginReview>) -> Option<String> {
        let result = (|| {
            let _writing = self.writing_sources();
            let source = self.plugin_source(&approval.item)?;
            if source.address != approval.source_address {
                return Err("The catalogue address changed. Review the plugin again.".to_owned());
            }
            self.check_plugin_origin(&approval.item, &source.address, approval.previous.as_ref())?;
            let package = approval
                .package
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
                .ok_or("This plugin review has already been used.")?;
            approval.plugins.install_reviewed_store_plugin(package, &approval.previous)?;
            let mut record = Installed::from_item(&approval.item);
            record.source_pin = sha256_hex(source.address.as_bytes());
            self.save_install(record).then_some(()).ok_or_else(|| "The install record couldn't be saved.".to_owned())
        })();
        result.err()
    }

    /// Fetches a picture by its address: an item's preview, at the size a card or a page shows it.
    pub fn fetch_picture(&self, address: String) -> Fetched {
        if let Err(why) = crate::fetch::web_address(&address) {
            return Fetched { bytes: Vec::new(), failure: why };
        }
        match self.transport.get(&address, crate::store_media::PICTURE_BYTES, crate::fetch::FETCH_TIME) {
            Ok(bytes) => Fetched { bytes, failure: String::new() },
            Err(why) => Fetched { bytes: Vec::new(), failure: why },
        }
    }
}

impl Store {
    fn local_origin(&self, source: &str) -> Option<String> {
        self.held()
            .iter()
            .find(|held| held.saved.id == source && held.saved.on && held.saved.local_network)
            .map(|held| held.saved.address.clone())
    }

    pub(crate) fn fetch_media_bytes(&self, source: &str, address: &str, cap: usize) -> Result<Vec<u8>, String> {
        crate::fetch::web_address(address)?;
        self.transport.get_scoped(address, cap, crate::fetch::FETCH_TIME, self.local_origin(source).as_deref())
    }

    pub(crate) fn copy_media(
        &self,
        source: &str,
        address: &str,
        cap: usize,
        out: &mut dyn std::io::Write,
    ) -> Result<usize, String> {
        crate::fetch::web_address(address)?;
        self.transport.get_into_scoped(
            address,
            cap,
            std::time::Duration::from_secs(120),
            self.local_origin(source).as_deref(),
            out,
        )
    }
}

/// Decodes a Store preview within a pixel budget, then scales it to the presentation size.
pub fn store_picture(bytes: &[u8], edge: u32) -> Option<image::RgbaImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    if decoded.width().max(decoded.height()) > edge {
        return Some(decoded.resize(edge, edge, image::imageops::FilterType::Triangle).into_rgba8());
    }
    Some(decoded.into_rgba8())
}

/// What a web source that has never been read says on its row.
const NOT_READ: &str = "it has not been read yet.";

/// The source that ships with the app: the plugins compiled into the binary, listed like any
/// others. It is never stored, cannot be removed, and can be switched off like the rest.
const SHIPPED: &str = "procraft";
const SHIPPED_NAME: &str = "ProCraft Store";

/// The shipped source: the bundled plugins as one catalogue.
fn shipped_held(on: bool) -> Held {
    Held {
        saved: Saved {
            id: SHIPPED.to_owned(),
            name: SHIPPED_NAME.to_owned(),
            address: String::new(),
            on,
            local_network: false,
        },
        catalogue: Some(Catalogue {
            name: SHIPPED_NAME.to_owned(),
            publisher: "ProCraft".to_owned(),
            items: shipped_items().to_vec(),
        }),
        read: 0,
        failure: String::new(),
        refresh: Arc::new(()),
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
                    icon: String::new(),
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

#[cfg(test)]
pub(crate) fn bundled_plugin_for_cover_test() -> Option<StoreItem> {
    shipped_items().first().cloned()
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
            source_writes: Mutex::new(()),
            installed: Mutex::new(installed),
            installed_brushes: Mutex::new(Vec::new()),
            transport,
        })
    }

    /// Reads a source from its web address, or says why it could not be read.
    fn read_web(&self, saved: &Saved) -> Result<(Catalogue, Vec<u8>), String> {
        crate::fetch::web_address(&saved.address)?;
        let bytes = self.transport.get_scoped(
            &saved.address,
            CATALOGUE_BYTES,
            crate::fetch::FETCH_TIME,
            saved.local_network.then_some(&saved.address),
        )?;
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

    fn brush_records(&self) -> std::sync::MutexGuard<'_, Vec<Installed>> {
        self.installed_brushes.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn follow_brush_installs(&self, session: &crate::CanvasSession) {
        *self.brush_records() = session.store_brush_installs();
    }

    fn plugin_source(&self, item: &StoreItem) -> Result<SourceInfo, String> {
        if item.kind != ItemKind::Plugin {
            return Err("This listing isn't for a plugin.".to_owned());
        }
        let held = self.held();
        let source = held
            .iter()
            .find(|source| source.saved.id == item.source && source.saved.on)
            .ok_or("The catalogue is unavailable.")?;
        let same =
            source.catalogue.as_ref().is_some_and(|catalogue| catalogue.items.iter().any(|listed| listed == item));
        if !same || item.file.is_empty() {
            return Err("The plugin listing changed. Open it again.".to_owned());
        }
        Ok(described(source))
    }

    fn check_plugin_origin(
        &self,
        item: &StoreItem,
        address: &str,
        previous: Option<&crate::plugins::StorePluginState>,
    ) -> Result<(), String> {
        let Some(previous) = previous else { return Ok(()) };
        let records = self.records();
        let pin = sha256_hex(address.as_bytes());
        let same = records.iter().any(|record| {
            record.kind == ItemKind::Plugin
                && record.id == item.id
                && record.version == previous.version
                && record.source == item.source
                && (record.source_pin.is_empty() || record.source_pin == pin)
        });
        same.then_some(())
            .ok_or_else(|| "Store updates must come from the catalogue that installed the existing plugin.".to_owned())
    }

    fn prepare_plugin_review(
        &self,
        item: StoreItem,
        plugins: Arc<crate::Plugins>,
        path: &Path,
    ) -> Result<StorePluginReview, String> {
        let source = self.plugin_source(&item)?;
        listing_size(&item)?;
        let bytes = crate::plugin_file::read_plugin_file(path).ok_or("The plugin file couldn't be read.")?;
        let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
        check_listing(&item, bytes.len() as u64, digest.as_ref())?;
        let package = crate::plugin_file::read_decoded_plugin(crate::store_decode::decode_bytes(
            crate::StoreDecodeKind::Plugin,
            &bytes,
        )?)?;
        let manifest = &package.manifest;
        if item.id != manifest.id || item.version != manifest.version {
            return Err("The plugin file's ID or version differs from the catalogue listing.".to_owned());
        }
        if !manifest.publisher.is_empty() && plain(&manifest.publisher) != item.publisher {
            return Err("The publisher in the plugin file differs from the catalogue listing.".to_owned());
        }
        let previous = plugins.store_plugin_state(manifest)?;
        self.check_plugin_origin(&item, &source.address, previous.as_ref())?;

        let name = plain(&manifest.name);
        let origin = source
            .address
            .strip_prefix("https://")
            .and_then(|url| url.split(['/', '?', '#']).next())
            .and_then(|authority| authority.rsplit('@').next())
            .map(|host| plain(host).chars().take(100).collect::<String>())
            .unwrap_or_else(|| "Local catalogue".to_owned());
        let mut lines = vec![format!("Version {}", manifest.version), format!("{} · {origin}", plain(&source.name))];
        if !manifest.publisher.is_empty() {
            lines.push(format!("{} · Publisher identity unverified", plain(&manifest.publisher)));
        } else {
            lines.push("Publisher identity unverified".to_owned());
        }
        let contents: Vec<_> = manifest
            .holds
            .iter()
            .map(|part| match part {
                crate::PluginPart::Hooks => "plugin code",
                crate::PluginPart::Filter => "filters",
                crate::PluginPart::Brushes => "brushes",
            })
            .collect();
        lines.push(format!("Package contains {}", contents.join(", ")));
        let added: Vec<_> = manifest
            .permissions
            .iter()
            .filter(|permission| previous.as_ref().is_some_and(|previous| !previous.permissions.contains(permission)))
            .collect();
        if !added.is_empty() {
            let added: Vec<_> = added.iter().map(|permission| permission.review_label()).collect();
            lines.push(format!("New permissions in this update\n{}", added.join(", ")));
        }
        if !manifest.permissions.is_empty() {
            let permissions: Vec<_> = manifest.permissions.iter().map(|permission| permission.review_label()).collect();
            lines.push(format!("Requested permissions\n{}", permissions.join(", ")));
        }
        if manifest.controls.iter().any(|control| matches!(control, crate::PluginControl::Folder { .. })) {
            lines.push(
                "ProCraft can read or write files on its behalf in folders you choose for this plugin.".to_owned(),
            );
            if previous.is_some() {
                lines.push("This update can use folders you already chose for the plugin.".to_owned());
            }
        }
        Ok(StorePluginReview {
            name,
            message: lines.join("\n\n"),
            item,
            source_address: source.address,
            previous,
            plugins,
            package: Mutex::new(Some(package)),
        })
    }

    fn save_install(&self, one: Installed) -> bool {
        let mut list = self.records();
        let mut updated: Vec<_> =
            list.iter().filter(|old| old.id != one.id || old.source != one.source).cloned().collect();
        updated.push(one);
        if !self.write_installed_list(&updated) {
            return false;
        }
        *list = updated;
        true
    }

    fn write_installed_list(&self, list: &[Installed]) -> bool {
        let Some(dir) = &self.dir else { return true };
        let text = write_installed(list);
        write_atomically(&dir.join(INSTALLED_FILE), |tmp| {
            std::fs::write(tmp, &text).map_err(|e| file_error("saving what was installed", e))
        })
        .is_ok()
    }

    fn cache_dir(&self, id: &str) -> Option<PathBuf> {
        valid_source_id(id).then(|| self.dir.as_ref().map(|d| d.join(id))).flatten()
    }

    fn writing_sources(&self) -> std::sync::MutexGuard<'_, ()> {
        self.source_writes.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn keep_sources(&self) {
        let Some(dir) = &self.dir else { return };
        let text = write_sources(&self.held());
        let written = write_atomically(&dir.join(SOURCES_FILE), |tmp| {
            std::fs::write(tmp, &text).map_err(|e| file_error("saving the sources", e))
        });
        if let Err(e) = written {
            log::warn!("keeping the store's sources: {e}");
        }
    }

    fn read(&self, saved: &Saved) -> Result<(Catalogue, Vec<u8>), String> {
        if web(&saved.address) { self.read_web(saved) } else { read_local(saved) }
    }

    fn listing(&self, source: &str, id: &str) -> Option<StoreItem> {
        self.held()
            .iter()
            .find(|h| h.saved.id == source && h.saved.on)?
            .catalogue
            .as_ref()?
            .items
            .iter()
            .find(|item| item.id == id)
            .cloned()
    }

    #[cfg(test)]
    fn installed_one(&self, id: String) -> Option<Installed> {
        self.records().iter().find(|one| one.id == id).cloned()
    }

    #[cfg(test)]
    fn keep_installed(&self, id: String, digest: String) -> bool {
        let Some(mut item) = self.items().into_iter().find(|item| item.id == id) else { return false };
        item.sha256 = digest;
        self.save_install(Installed::from_item(&item))
    }

    #[cfg(test)]
    fn forget_installed(&self, id: String) -> bool {
        self.installed_one(id.clone()).is_some_and(|one| self.forget_listing(one.source, id))
    }

    #[cfg(test)]
    fn fetch_item(&self, id: String) -> Fetched {
        let result = self
            .items()
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| "no source lists that item".to_owned())
            .and_then(|item| {
                if item.file.is_empty() {
                    return Err("it came with the app, so there is nothing to fetch".to_owned());
                }
                crate::fetch::web_address(&item.file)?;
                self.transport.get(&item.file, listing_size(&item)?, crate::fetch::FETCH_TIME)
            });
        match result {
            Ok(bytes) => Fetched { bytes, failure: String::new() },
            Err(failure) => Fetched { bytes: Vec::new(), failure },
        }
    }

    fn keep_read_failure(&self, id: &str, reason: &str) {
        let Some(dir) = self.cache_dir(id) else { return };
        let path = dir.join(READ_FILE);
        let Ok(contents) = std::fs::read_to_string(&path) else { return };
        let Ok(mut record) = serde_json::from_str::<serde_json::Value>(&contents) else { return };
        let Some(fields) = record.as_object_mut() else { return };
        fields.insert("failure".to_owned(), serde_json::json!(reason));
        fields.insert("failed_at".to_owned(), serde_json::json!(iso(now())));
        let result = write_atomically(&path, |tmp| {
            std::fs::write(tmp, record.to_string()).map_err(|error| file_error("saving catalogue status", error))
        });
        if let Err(error) = result {
            log::warn!("saving catalogue status: {error}");
        }
    }

    fn keep(&self, saved: &Saved, bytes: &[u8], catalogue: &Catalogue) {
        if let Some(dir) = &self.dir {
            keep_catalogue(dir, saved, bytes, catalogue);
        }
    }
}

fn valid_source_id(id: &str) -> bool {
    id.len() == 6 && id.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn listing_size(item: &StoreItem) -> Result<usize, String> {
    let max = if item.kind == ItemKind::Plugin { PLUGIN_BYTES } else { ITEM_BYTES };
    usize::try_from(item.bytes)
        .ok()
        .filter(|&size| size > 0 && size <= max)
        .ok_or_else(|| format!("the listed file is outside the Store's {} MiB limit", max / (1024 * 1024)))
}

fn check_listing(item: &StoreItem, len: u64, digest: &[u8]) -> Result<(), String> {
    if len != item.bytes {
        return Err(format!("the file is {len} bytes and the catalogue says {}", item.bytes));
    }
    if hex(digest) != item.sha256 {
        return Err("the file does not match the catalogue".to_owned());
    }
    Ok(())
}

pub fn check_listing_file(item: &StoreItem, path: &Path) -> Result<(), String> {
    use std::io::Read as _;
    let cap = listing_size(item)?;
    let file = std::fs::File::open(path).map_err(|e| format!("the file could not be read: {e}"))?;
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let len = std::io::copy(
        &mut file.take(cap as u64 + 1),
        &mut HashWriter { inner: &mut std::io::sink(), digest: &mut digest },
    )
    .map_err(|e| format!("the file could not be read: {e}"))?;
    check_listing(item, len, digest.finish().as_ref())
}

struct HashWriter<'a> {
    inner: &'a mut dyn std::io::Write,
    digest: &'a mut ring::digest::Context,
}

impl std::io::Write for HashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let count = self.inner.write(bytes)?;
        let written = bytes.get(..count).ok_or_else(|| std::io::Error::other("invalid write count"))?;
        self.digest.update(written);
        Ok(count)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
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
        let failure = if cached.is_some() { failure } else { NOT_READ.to_owned() };
        return Held { saved, catalogue: cached, read, failure, refresh: Arc::new(()) };
    }
    match read_local(&saved) {
        Ok((catalogue, bytes)) => {
            keep_catalogue(root, &saved, &bytes, &catalogue);
            Held { saved, catalogue: Some(catalogue), read: now(), failure: String::new(), refresh: Arc::new(()) }
        }
        // a file that will not read now still shows the copy it had, with the reason it is stale
        Err(reason) => Held { saved, catalogue: cached, read, failure: reason, refresh: Arc::new(()) },
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
            if h.saved.local_network {
                one.insert("local_network".to_owned(), serde_json::Value::Bool(true));
            }
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
            if !valid_source_id(&id) {
                return None;
            }
            let name = s.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_owned();
            let address =
                s.get("url").and_then(|v| v.as_str()).or_else(|| s.get("path").and_then(|v| v.as_str()))?.to_owned();
            let on = s.get("on").and_then(|v| v.as_bool()).unwrap_or(true);
            let local_network = s.get("local_network").and_then(|v| v.as_bool()).unwrap_or(false);
            (!address.is_empty()).then_some(Saved { id, name, address, on, local_network })
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
/// nothing else. The file itself stays the set's own; this is what the store reads beside it. Its
/// first picture is the one a catalogue's card shows, so it is named on its own too.
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
    let preview = pictures.first().map_or("", String::as_str);
    package_details_with_cover(
        kind,
        name,
        author,
        description,
        category,
        licence,
        tags,
        pictures,
        preview,
        "",
        exported,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn package_details_with_cover(
    kind: &str,
    name: &str,
    author: &str,
    description: &str,
    category: &str,
    licence: &str,
    tags: &[String],
    pictures: &[String],
    preview: &str,
    icon: &str,
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
        "preview": preview,
        "icon": icon,
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

pub(crate) fn now() -> u64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(since) => since.as_secs(),
        Err(_) => 0,
    }
}

/// `secs` since the epoch, as `YYYY-MM-DDTHH:MM:SSZ`.
pub(crate) fn iso(secs: u64) -> String {
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

fn sha256_hex(bytes: &[u8]) -> String {
    hex(ring::digest::digest(&ring::digest::SHA256, bytes).as_ref())
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut text = String::with_capacity(2 * bytes.len());
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
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
    let artwork_link = |key| {
        value
            .get(key)
            .and_then(|v| v.as_str())
            .map(plain)
            .filter(|link| https(link) && link.chars().count() <= LINK_CHARS)
            .unwrap_or_default()
    };
    let preview = artwork_link("preview");
    let icon = artwork_link("icon");
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
        icon,
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
        "brush" => Some(ItemKind::Brush),
        "brushlibrary" => Some(ItemKind::BrushLibrary),
        "asset pack" => Some(ItemKind::AssetPack),
        _ => None,
    }
}

/// The catalogue's spelling of a kind, as an installed record keeps it.
fn kind_key(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Plugin => "plugin",
        ItemKind::BrushSet => "brushset",
        ItemKind::Brush => "brush",
        ItemKind::BrushLibrary => "brushlibrary",
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
                "source_pin": one.source_pin,
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
        source_pin: text_of("source_pin"),
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

/// The HTTPS links an item page can display, bounded by the gallery and link limits.
fn links_of(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let Some(value) = value else { return Some(Vec::new()) };
    let list = value.as_array()?;
    let mut links = Vec::with_capacity(list.len().min(PREVIEWS_MOST));
    for one in list.iter().take(PREVIEWS_MOST) {
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
        Saved { id: "a1b2c3".into(), name: String::new(), address: address.into(), on: true, local_network: false }
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
    fn featured_order_uses_the_day_and_every_candidate_without_repeating_a_listing() {
        let listings: Vec<_> = (0..16).map(|id| format!("{}:item-{id}", id % 3)).collect();
        let selected = |listings: Vec<String>, day| {
            store_featured_order(listings.clone(), day)
                .into_iter()
                .map(|index| listings[index as usize].clone())
                .collect::<Vec<_>>()
        };
        let current = selected(listings.clone(), 20_000);
        assert_eq!(current.len(), 5);
        assert_eq!(current.iter().collect::<std::collections::HashSet<_>>().len(), 5);
        assert_eq!(current, selected(listings.iter().rev().cloned().collect(), 20_000));
        assert_ne!(current, selected(listings.clone(), 20_001));
        let over_time: std::collections::HashSet<_> =
            (20_000..20_030).flat_map(|day| selected(listings.clone(), day)).collect();
        assert_eq!(over_time.len(), listings.len());
        assert_eq!(selected(listings[..2].to_vec(), 20_000).len(), 2);
        assert!(selected(Vec::new(), 20_000).is_empty());
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
    fn selected_downloads_accept_both_sources_and_remove_a_failed_stage() {
        let root = tempfile::tempdir().unwrap();
        let first = "first brush file";
        let second = "a different brush file";
        let catalogue = |file: &str, bytes: &str| {
            serde_json::json!({
                "schema": 1,
                "store": {"name": "Example", "publisher": "Maker", "updated": "2026-11-25"},
                "items": [{
                    "id": "org.example.ink", "kind": "brushset", "name": "Ink", "publisher": "Maker",
                    "description": "Brushes", "version": "1.0.0", "file": file,
                    "bytes": bytes.len(), "sha256": digest_of(bytes.as_bytes())
                }]
            })
            .to_string()
        };
        let first_url = "https://example.org/first";
        let second_url = "https://example.org/second";
        let a_url = "https://example.org/a.json";
        let b_url = "https://example.org/b.json";
        let c_url = "https://example.org/c.json";
        let answers = [
            (a_url, catalogue(first_url, first)),
            (b_url, catalogue(second_url, second)),
            (c_url, catalogue(first_url, "wrong on purpose")),
            (first_url, first.to_owned()),
            (second_url, second.to_owned()),
        ]
        .into_iter()
        .map(|(address, body)| (address.to_owned(), Ok(body.into_bytes())))
        .collect();
        let store = Store::open_with(root.path().to_string_lossy().into_owned(), Arc::new(Fake(answers)));
        for url in [a_url, b_url, c_url] {
            assert_eq!(store.add_source(url.to_owned()), None);
        }
        for (at, url, expected) in [(0, a_url, first), (1, b_url, second), (2, c_url, "")] {
            let source = store.sources().into_iter().find(|source| source.address == url).unwrap();
            let listing = store.listing(&source.id, "org.example.ink").unwrap();
            let path = root.path().join(format!("payload-{at}"));
            let result = store.download_listing(listing, path.to_string_lossy().into_owned());
            if expected.is_empty() {
                assert!(result.is_some());
                assert!(!path.exists());
            } else {
                assert_eq!(result, None);
                assert_eq!(std::fs::read_to_string(path).unwrap(), expected);
            }
        }
    }

    #[test]
    fn preview_limit_holds_every_package_picture_without_dropping_the_item() {
        let mut catalogue: serde_json::Value = serde_json::from_str(CATALOGUE).unwrap();
        let pictures =
            (0..PREVIEWS_MOST + 2).map(|index| format!("https://example.org/picture-{index}.png")).collect::<Vec<_>>();
        catalogue["items"][0]["preview"] = serde_json::json!(pictures[0]);
        catalogue["items"][0]["previews"] = serde_json::json!(pictures);
        let parsed = parse(catalogue.to_string().as_bytes(), &saved("test")).unwrap();
        let gallery = parsed.items[0].gallery();
        assert_eq!(gallery.len(), PREVIEWS_MOST);
        assert!(gallery[0].ends_with("picture-0.png"));
        assert!(gallery[PREVIEWS_MOST - 1].ends_with(&format!("picture-{}.png", PREVIEWS_MOST - 1)));
    }

    #[test]
    fn store_plugins_are_bound_to_their_listing_and_require_approval_of_new_permissions() {
        use crate::plugin_file::tests::{coded, pack};
        use crate::plugin_run::{
            MODULE,
            tests::{ACTS, position_module},
        };
        use serde_json::json;

        let root = tempfile::tempdir().unwrap();
        let text = |path: &Path| path.to_string_lossy().into_owned();
        let store = Store::open(text(&root.path().join("store")));
        let plugins = crate::Plugins::with_store(text(&root.path().join("plugins")));
        let id = "org.example.reviewed";
        let v1 = position_module("", "(i64.const 0)");
        let first = pack(&coded(id, &v1), &[(MODULE, &v1)]);
        let v2 = wat::parse_str(ACTS).unwrap();
        let mut manifest = coded(id, &v2);
        manifest["version"] = json!("2.0.0");
        manifest["compat"]["procraft"]["interface"] = json!(2);
        manifest["hooks"] = json!([]);
        manifest["permissions"] = json!(["artwork.write"]);
        manifest["actions"] = json!([{ "key": "grid", "label": "Add a grid layer" }]);
        let update = pack(&manifest, &[(MODULE, &v2)]);

        let listings = |path: &Path, id: &str, version: &str, bytes: &[u8]| {
            let catalogue = json!({
                "schema": 1,
                "store": { "name": "Test Catalogue", "updated": "2026-11-25" },
                "items": [{
                    "id": id, "kind": "plugin", "name": "A plugin", "publisher": "Someone",
                    "version": version, "description": "A test plugin", "file": "https://example.org/download",
                    "bytes": bytes.len(), "sha256": digest_of(bytes),
                }]
            });
            std::fs::write(path, catalogue.to_string()).unwrap();
        };
        let source_file = root.path().join("source.json");
        let plugin_file = root.path().join("plugin");
        std::fs::write(&plugin_file, &first).unwrap();
        listings(&source_file, "org.example.other", "1.0.0", &first);
        assert_eq!(store.add_source(text(&source_file)), None);
        let source = added(&store).unwrap().id;
        let other = store.listing(&source, "org.example.other").unwrap();
        let review = store.review_plugin_file(other, plugins.clone(), text(&plugin_file));
        assert!(review.failure.unwrap().contains("ID or version"));
        assert!(plugins.plugins().is_empty());

        listings(&source_file, id, "1.1.0", &first);
        assert_eq!(store.refresh(source.clone()), None);
        let wrong_version = store.listing(&source, id).unwrap();
        assert!(
            store
                .review_plugin_file(wrong_version, plugins.clone(), text(&plugin_file))
                .failure
                .unwrap()
                .contains("ID or version")
        );

        listings(&source_file, id, "1.0.0", &first);
        assert_eq!(store.refresh(source.clone()), None);
        let listed = store.listing(&source, id).unwrap();
        let inspected = store.review_plugin_file(listed, plugins.clone(), text(&plugin_file));
        assert!(inspected.failure.is_none(), "{:?}", inspected.failure);
        assert!(plugins.plugins().is_empty(), "review hasn't installed any code");
        assert!(store.installed_listing(source.clone(), id.to_owned()).is_none());
        let approved = inspected.review.unwrap();
        assert_eq!(store.approve_plugin_listing(approved.clone()), None);
        assert!(store.approve_plugin_listing(approved).is_some(), "an approval is single-use");
        assert!(plugins.set_on(id.to_owned(), true));

        std::fs::write(&plugin_file, &update).unwrap();
        let elsewhere = root.path().join("other.json");
        listings(&elsewhere, id, "2.0.0", &update);
        assert_eq!(store.add_source(text(&elsewhere)), None);
        let other = store.sources().into_iter().find(|entry| entry.address == text(&elsewhere)).unwrap();
        let cross_source = store.listing(&other.id, id).unwrap();
        let rejected = store.review_plugin_file(cross_source, plugins.clone(), text(&plugin_file));
        assert!(rejected.failure.unwrap().contains("catalogue that installed"));

        listings(&source_file, id, "2.0.0", &update);
        assert_eq!(store.refresh(source.clone()), None);
        let listed = store.listing(&source, id).unwrap();
        let inspected = store.review_plugin_file(listed, plugins.clone(), text(&plugin_file));
        assert!(inspected.failure.is_none(), "{:?}", inspected.failure);
        let reviewed = inspected.review.unwrap();
        assert!(reviewed.is_update());
        assert!(reviewed.message().contains("New permissions in this update"));
        assert!(reviewed.message().contains("Change artwork and layers"));
        assert_eq!(plugins.plugins().first().unwrap().version, "1.0.0");
        assert_eq!(store.approve_plugin_listing(reviewed), None);
        assert_eq!(plugins.plugins().first().unwrap().version, "2.0.0");
        assert_eq!(
            store.installed_listing(source, id.to_owned()).unwrap().source_pin,
            digest_of(text(&source_file).as_bytes())
        );
    }

    #[test]
    fn a_store_brush_listing_requires_a_real_brush_set() {
        let root = tempfile::tempdir().unwrap();
        let file = |name: &str| root.path().join(name);
        let brush = file("brush");
        let library = file("library");
        let set = file("set");
        let session = crate::CanvasSession::new(8, 8);
        session.write_brush_file(0, 0, brush.to_string_lossy().into_owned()).unwrap();
        session.write_brush_library_file(library.to_string_lossy().into_owned(), None).unwrap();
        session.write_brush_set_file(0, set.to_string_lossy().into_owned(), None).unwrap();
        for wrong in [brush, library] {
            assert!(crate::brush_files::decode_store_brush_set(std::fs::File::open(wrong).unwrap()).is_err());
        }
        assert!(!crate::brush_files::decode_store_brush_set(std::fs::File::open(set).unwrap()).unwrap().1.is_empty());
    }

    #[test]
    fn a_broken_plugin_cover_uses_the_same_artwork_as_a_missing_cover() {
        let root = tempfile::tempdir().unwrap();
        let bad = "https://example.org/bad.png";
        let answers = [(bad.to_owned(), Ok(b"not an image".to_vec()))].into_iter().collect();
        let store = Store::open_with(root.path().to_string_lossy().into_owned(), Arc::new(Fake(answers)));
        let item = bundled_plugin_for_cover_test().unwrap();
        let missing = store.fetch_store_cover(item.clone()).unwrap();
        let mut broken = item;
        broken.preview = bad.to_owned();
        assert_eq!(store.fetch_store_cover(broken), Some(missing));
    }

    #[test]
    fn video_stages_a_valid_clip_and_removes_a_rejected_file() {
        let root = tempfile::tempdir().unwrap();
        let good = "https://example.org/demo.webm?version=2";
        let bad = "https://example.org/bad.webm";
        let oversized = "https://example.org/big.mp4";
        let header = b"\x1a\x45\xdf\xa3\x87\x42\x82\x84webm\x18\x53\x80\x67\x92\x15\x49\xa9\x66\x81\x00\x16\x54\xae\x6b\x81\x00\x1f\x43\xb6\x75\x81\x00";
        let answers = [
            (good.to_owned(), Ok(header.to_vec())),
            (bad.to_owned(), Ok(b"wrong format".to_vec())),
            (oversized.to_owned(), Ok(vec![0; 25 * 1024 * 1024 + 1])),
        ]
        .into_iter()
        .collect();
        let store = Store::open_with(root.path().to_string_lossy().into_owned(), Arc::new(Fake(answers)));
        let path = root.path().join("preview.webm");
        let text = path.to_string_lossy().into_owned();
        assert!(store.download_store_video(bad.to_owned(), text.clone()).is_some());
        assert!(!path.exists());
        assert!(store.download_store_video(oversized.to_owned(), text.clone()).is_some());
        assert!(!path.exists());
        assert_eq!(store.download_store_video(good.to_owned(), text), None);
        assert_eq!(std::fs::read(path).unwrap(), header);
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
        assert_eq!(value.get("preview").and_then(|v| v.as_str()), Some("preview.png"));
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
