//! Taking one item the store lists: the bytes checked against what the catalogue said about them,
//! then handed to whatever installs that kind.

use std::sync::Arc;

use procraft_core::{CanvasSession, ItemKind, StoreItem};

/// Takes the verified staged file, streaming brush sets into the library.
pub fn take(session: &Arc<CanvasSession>, item: &StoreItem, path: &std::path::Path) -> Result<(), String> {
    match item.kind {
        ItemKind::BrushSet => {
            session.install_store_brushes(item.clone(), path.to_string_lossy().into_owned()).map_or(Ok(()), Err)
        }
        ItemKind::Plugin => Err("A Store plugin requires review before it can be installed.".to_owned()),
        ItemKind::AssetPack | ItemKind::Brush | ItemKind::BrushLibrary => {
            Err("This Store listing is available for preview.".to_owned())
        }
    }
}

/// Takes a chosen file through the same private staging used by a Store download.
pub fn take_local_brushes(
    session: &Arc<CanvasSession>,
    item: &StoreItem,
    path: &std::path::Path,
) -> Result<(), String> {
    use std::io::Read as _;

    if item.kind != ItemKind::BrushSet {
        return take(session, item, path);
    }
    if !std::fs::metadata(path).is_ok_and(|metadata| metadata.is_file()) {
        return Err("Choose a regular brush set file.".to_owned());
    }
    procraft_core::check_listing_file(item, path)?;
    let failed = |error| format!("The brush set couldn't be staged: {error}");
    let stage = tempfile::tempdir().map_err(failed)?;
    let staged = stage.path().join("brushset");
    let source = std::fs::File::open(path).map_err(failed)?;
    let mut target = std::fs::File::create(&staged).map_err(failed)?;
    std::io::copy(&mut source.take(item.bytes + 1), &mut target).map_err(failed)?;
    drop(target);
    take(session, item, &staged)
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
            icon: String::new(),
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
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), bytes).unwrap();
        let check = |len, digest: &str| procraft_core::check_listing_file(&item(len, digest), file.path());
        assert!(check(bytes.len() as u64, &digest).is_ok());
        assert!(check(bytes.len() as u64 + 1, &digest).is_err());
        assert!(check(bytes.len() as u64, &"a".repeat(64)).is_err());
        assert!(check(0, "").is_err());
    }

    #[test]
    fn the_whole_way_from_a_catalogue_to_an_installed_set() {
        crate::store_decoder::register();
        let root = std::env::temp_dir().join(format!("procraft-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::create_dir_all(&root);
        // a file the repo already has, listed by a catalogue written for this test
        let set = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../legacy-brushes/Inking.brushset");
        let bytes = std::fs::read(&set).unwrap();
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
        assert!(procraft_core::check_listing_file(&item, &set).is_ok());
        let session = procraft_core::CanvasSession::new(32, 32);
        session.open_brushes(root.join("brushes").to_string_lossy().into_owned());
        assert_eq!(take_local_brushes(&session, &item, &set), Ok(()), "the chosen set is taken in");
        store.read_brush_installs(root.join("brushes").to_string_lossy().into_owned());
        assert_eq!(store.installed_listing(item.source.clone(), item.id.clone()).map(|one| one.digest), Some(digest));
        let count = session.brush_library().len();
        assert_eq!(take(&session, &item, &set), Ok(()), "a retry finds the same set");
        assert_eq!(session.brush_library().len(), count);
        assert!(session.delete_brush_set(0));
        assert!(session.keep_brushes(root.join("brushes").to_string_lossy().into_owned()));
        store.read_brush_installs(root.join("brushes").to_string_lossy().into_owned());
        assert!(store.installed_listing(item.source, item.id).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
