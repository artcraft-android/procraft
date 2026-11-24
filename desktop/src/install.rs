//! Taking one item the store lists: the bytes checked against what the catalogue said about them,
//! then handed to whatever installs that kind.

use std::sync::Arc;

use procraft_core::{CanvasSession, ItemKind, Plugins, StoreItem};

/// Why `bytes` cannot be the file the catalogue listed, if they cannot be. The catalogue's size and
/// digest are what a web address cannot quietly swap out.
pub fn refusal(item: &StoreItem, bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return Some("The file came through empty.".to_owned());
    }
    if item.bytes > 0 && bytes.len() as u64 != item.bytes {
        return Some(format!("The file is {} bytes, and the catalogue said {}.", bytes.len(), item.bytes));
    }
    if !item.sha256.is_empty() && procraft_core::digest_of(bytes) != item.sha256 {
        return Some("The file is not the one the catalogue lists, so it was not taken.".to_owned());
    }
    None
}

/// Installs `bytes` as `item`, by kind. Ok means the app has it now; Err says why not, in words a
/// person can read.
pub fn take(
    session: &Arc<CanvasSession>,
    plugins: &Arc<Plugins>,
    item: &StoreItem,
    bytes: &[u8],
) -> Result<(), String> {
    if let Some(no) = refusal(item, bytes) {
        return Err(no);
    }
    match item.kind {
        // None is a plugin taken in; Some is the reason it was not
        ItemKind::Plugin => plugins.install(bytes.to_vec()).map_or(Ok(()), Err),
        // None is nothing readable in the file
        ItemKind::BrushSet => session
            .import_any_brushes(bytes.to_vec(), None, 0)
            .map(|_| ())
            .ok_or_else(|| "No brushes could be read from it.".to_owned()),
        ItemKind::AssetPack => Err("Asset packs need the asset library, which the app does not have yet.".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(bytes: u64, sha256: &str) -> StoreItem {
        StoreItem {
            id: "com.example.dry-ink".into(),
            kind: ItemKind::BrushSet,
            name: "Dry Ink".into(),
            version: "2.1.0".into(),
            publisher: "R. Halloway".into(),
            description: "Six brushes.".into(),
            licence: "CC BY 4.0".into(),
            requires: "Interface 2".into(),
            category: "Brushes".into(),
            tags: Vec::new(),
            preview: String::new(),
            previews: Vec::new(),
            body: String::new(),
            file: "https://example.org/dry-ink.brushset".into(),
            bytes,
            sha256: sha256.into(),
            source: "a1b2c3".into(),
            source_name: "Ink and Grain".into(),
        }
    }

    #[test]
    fn a_file_of_the_wrong_size_or_digest_is_refused_before_anything_is_installed() {
        let bytes = b"the file the catalogue listed";
        let digest = procraft_core::digest_of(bytes);
        assert_eq!(refusal(&item(bytes.len() as u64, &digest), bytes), None, "the one it lists is taken");
        assert!(refusal(&item(bytes.len() as u64 + 1, &digest), bytes).is_some(), "a size that is off is refused");
        assert!(
            refusal(&item(bytes.len() as u64, &digest), b"another file").is_some(),
            "a digest that is off is refused"
        );
        assert!(refusal(&item(0, ""), b"").is_some(), "an empty file is refused");
        // nothing said about the file means nothing to check against
        assert_eq!(refusal(&item(0, ""), bytes), None);
    }

    #[test]
    fn a_plugin_file_is_taken_in_and_still_lists_once() {
        let root = std::env::temp_dir().join(format!("procraft-plugin-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/core/bundled-plugins/colour-labels.procraft-plugin");
        let Ok(bytes) = std::fs::read(&file) else { return };
        let plugins = procraft_core::Plugins::with_bundled(root.join("plugins").to_string_lossy().into_owned());
        let before = plugins.plugins().len();
        let mut listed = item(bytes.len() as u64, &procraft_core::digest_of(&bytes));
        listed.kind = ItemKind::Plugin;
        let Ok(session) = procraft_core::CanvasSession::open(root.join("work").to_string_lossy().into_owned()) else {
            return;
        };
        assert_eq!(take(&session, &plugins, &listed, &bytes), Ok(()), "the plugin file is taken in");
        assert_eq!(plugins.plugins().len(), before, "taking it in again does not add a second card");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_whole_way_from_a_catalogue_to_an_installed_set() {
        let root = std::env::temp_dir().join(format!("procraft-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        // a file the repo already has, listed by a catalogue written for this test
        let set = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../legacy-brushes/Inking.brushset");
        let Ok(bytes) = std::fs::read(&set) else { return };
        let digest = procraft_core::digest_of(&bytes);
        let catalogue = root.join("catalogue.json");
        let listing = format!(
            concat!(
                r#"{{"schema":1,"store":{{"name":"Dry Run","publisher":"R. Halloway","updated":"2026-11-22"}},"#,
                r#""items":[{{"id":"com.example.inking","kind":"brushset","name":"Inking","publisher":"R. Halloway","#,
                r#""version":"1.0.0","description":"A set.","category":"Brushes","#,
                r#""file":"https://example.invalid/inking.brushset","bytes":{},"sha256":"{}"}}]}}"#
            ),
            bytes.len(),
            digest
        );
        assert!(std::fs::write(&catalogue, listing).is_ok(), "the test writes its own catalogue");
        let store = procraft_core::Store::open(root.join("store").to_string_lossy().into_owned());
        assert_eq!(store.add_source(catalogue.to_string_lossy().into_owned()), None);
        let listed = store.items().into_iter().find(|item| item.id == "com.example.inking");
        assert!(listed.is_some(), "the catalogue lists the set");
        let Some(item) = listed else { return };
        assert_eq!(refusal(&item, &bytes), None, "the file on disk is the one the catalogue lists");
        let Ok(session) = procraft_core::CanvasSession::open(root.join("work").to_string_lossy().into_owned()) else {
            return;
        };
        let plugins = procraft_core::Plugins::new();
        assert_eq!(take(&session, &plugins, &item, &bytes), Ok(()), "the set is taken in");
        store.keep_installed(item.id.clone(), digest.clone());
        assert_eq!(
            store.installed_one(item.id.clone()).map(|one| one.digest),
            Some(digest),
            "what was taken is recorded with its digest"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
