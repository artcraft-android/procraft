//! The gallery's workspaces, the same in every app: galleries of their own, each a folder of
//! artworks an [`ArtworkStore`](crate::ArtworkStore) is opened on. The first is the folder the app
//! has always kept its artworks in, so nothing moves; the others are folders named for them in a
//! folder beside it, where the one showing is remembered too.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::gallery::{ARTWORK_FILE, folders, name_of};

/// The first workspace's name.
const DEFAULT_NAME: &str = "Gallery";
/// The file, among the others' folders, naming the one showing (none: the first).
const SELECTED_FILE: &str = ".selected";
/// The longest a name is, in characters: it is a folder's name too.
const LONGEST_NAME: usize = 80;

/// One of the gallery's workspaces: its name and the folder its artworks are in.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Workspace {
    pub name: String,
    pub root: String,
    /// The gallery there has always been: it can't be renamed or deleted.
    pub is_default: bool,
}

/// A part of a name to sort by: digits as the number they write, the rest without its case.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Part {
    Number(u128),
    Text(String),
}

/// `name` in parts to sort by, so "Ink 2" comes before "Ink 10" and "ink" beside "Ink".
fn parts(name: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    let mut chars = name.chars().peekable();
    while let Some(first) = chars.peek().copied() {
        let digits = first.is_ascii_digit();
        let mut run = String::new();
        while let Some(c) = chars.peek().copied().filter(|c| c.is_ascii_digit() == digits) {
            run.push(c);
            chars.next();
        }
        parts.push(if digits {
            Part::Number(run.parse().unwrap_or(u128::MAX))
        } else {
            Part::Text(run.to_lowercase())
        });
    }
    parts
}

/// `name` as a folder can be named: nothing that parts a path, no line ends, no dots first, no
/// longer than a name is. None if nothing is left.
fn cleaned(name: &str) -> Option<String> {
    let plain: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let plain: String = plain.trim().trim_start_matches('.').trim().chars().take(LONGEST_NAME).collect();
    let plain = plain.trim_end();
    if plain.is_empty() { None } else { Some(plain.to_owned()) }
}

/// The same name, whatever its case: a folder's name is the same one either way on an iPad or a Mac.
fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

/// The gallery's workspaces, and which one shows.
#[derive(uniffi::Object)]
pub struct Workspaces {
    default_root: PathBuf,
    others: PathBuf,
}

impl Workspaces {
    fn first(&self) -> Workspace {
        Workspace {
            name: DEFAULT_NAME.to_owned(),
            root: self.default_root.to_string_lossy().into_owned(),
            is_default: true,
        }
    }

    fn other(dir: &Path) -> Workspace {
        Workspace { name: name_of(dir), root: dir.to_string_lossy().into_owned(), is_default: false }
    }

    /// The others, in their names' order.
    fn rest(&self) -> Vec<Workspace> {
        let mut rest: Vec<Workspace> =
            folders(&self.others).iter().filter(|d| !name_of(d).starts_with('.')).map(|d| Self::other(d)).collect();
        rest.sort_by_cached_key(|w| (parts(&w.name), w.name.clone()));
        rest
    }

    /// `base`, or `base` with the first number after it that makes a name no workspace has
    /// (`but` aside: the one being renamed).
    fn unused(&self, base: &str, but: Option<&str>) -> String {
        let taken: Vec<String> = std::iter::once(DEFAULT_NAME.to_owned())
            .chain(self.rest().into_iter().map(|w| w.name))
            .filter(|n| but != Some(n.as_str()))
            .collect();
        let free = |name: &str| !taken.iter().any(|t| same(t, name));
        if free(base) {
            return base.to_owned();
        }
        (2u32..).map(|i| format!("{base} {i}")).find(|n| free(n)).unwrap_or_else(|| base.to_owned())
    }

    fn selected_name(&self) -> Option<String> {
        let text = fs::read_to_string(self.others.join(SELECTED_FILE)).ok()?;
        let name = text.lines().next()?.trim();
        if name.is_empty() { None } else { Some(name.to_owned()) }
    }

    fn remember(&self, name: Option<&str>) {
        let file = self.others.join(SELECTED_FILE);
        match name {
            Some(name) => {
                let _ = fs::write(file, format!("{name}\n"));
            }
            None => {
                let _ = fs::remove_file(file);
            }
        }
    }
}

#[uniffi::export]
impl Workspaces {
    /// The workspaces of the gallery kept in `default_root` (the first one, as it has always
    /// been), the others in folders of their own in `others`.
    #[uniffi::constructor]
    pub fn open(default_root: String, others: String) -> Arc<Workspaces> {
        Arc::new(Workspaces { default_root: PathBuf::from(default_root), others: PathBuf::from(others) })
    }

    /// Every workspace: the first, then the others in their names' order.
    pub fn list(&self) -> Vec<Workspace> {
        std::iter::once(self.first()).chain(self.rest()).collect()
    }

    /// The one the gallery shows: the one picked last, or the first if that one is gone.
    pub fn selected(&self) -> Workspace {
        self.selected_name()
            .and_then(|name| self.rest().into_iter().find(|w| w.name == name))
            .unwrap_or_else(|| self.first())
    }

    /// `workspace` is the one the gallery shows from now on.
    pub fn select(&self, workspace: Workspace) {
        self.remember(if workspace.is_default { None } else { Some(&workspace.name) });
    }

    /// A new, empty workspace named `name`, as a folder can be named and with a number after it if
    /// another has the name. None if nothing of the name is left, or it can't be made.
    pub fn create(&self, name: String) -> Option<Workspace> {
        let dir = self.others.join(self.unused(&cleaned(&name)?, None));
        fs::create_dir_all(&dir).ok()?;
        Some(Self::other(&dir))
    }

    /// `workspace` named `name` (as `create` would name it), still the one showing if it was.
    /// None if it is the first, nothing of the name is left, or its folder can't be renamed.
    pub fn rename(&self, workspace: Workspace, name: String) -> Option<Workspace> {
        if workspace.is_default {
            return None;
        }
        let name = cleaned(&name)?;
        if name == workspace.name {
            return Some(workspace);
        }
        let shown = self.selected_name().as_deref() == Some(workspace.name.as_str());
        let to = self.others.join(self.unused(&name, Some(&workspace.name)));
        fs::rename(&workspace.root, &to).ok()?;
        let renamed = Self::other(&to);
        if shown {
            self.remember(Some(&renamed.name));
        }
        Some(renamed)
    }

    /// `workspace` gone, with every artwork in it; if it was showing, the first shows. The first
    /// stays.
    pub fn delete(&self, workspace: Workspace) {
        if workspace.is_default {
            return;
        }
        let _ = fs::remove_dir_all(&workspace.root);
        if self.selected_name().as_deref() == Some(workspace.name.as_str()) {
            self.remember(None);
        }
    }

    /// How many artworks `workspace` holds, those in its stacks too: what deleting it deletes.
    pub fn artwork_count(&self, workspace: Workspace) -> u32 {
        let artwork = |dir: &PathBuf| dir.join(ARTWORK_FILE).is_file();
        folders(Path::new(&workspace.root))
            .iter()
            .map(|dir| if artwork(dir) { 1 } else { folders(dir).iter().filter(|d| artwork(d)).count() })
            .sum::<usize>() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ArtworkStore;

    fn open(tag: &str) -> (PathBuf, Arc<Workspaces>) {
        let base = std::env::temp_dir().join(format!(
            "procraft-workspaces-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&base);
        let at = |name: &str| base.join(name).to_string_lossy().into_owned();
        let _ = ArtworkStore::open(at("artworks"));
        (base.clone(), Workspaces::open(at("artworks"), at("workspaces")))
    }

    fn names(all: &Workspaces) -> Vec<String> {
        all.list().into_iter().map(|w| w.name).collect()
    }

    fn artwork(root: &str, folder: Option<String>) -> String {
        let dir = ArtworkStore::open(root.to_owned()).new_folder(folder);
        let json = serde_json::json!({ "name": "A", "document": { "width": 8, "height": 8 } });
        fs::write(Path::new(&dir).join(ARTWORK_FILE), json.to_string()).unwrap();
        dir
    }

    #[test]
    fn the_first_workspace_is_the_gallery_as_it_was() {
        let (base, all) = open("first");
        let list = all.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Gallery");
        assert!(list[0].is_default);
        assert_eq!(PathBuf::from(&list[0].root), base.join("artworks"));
        assert_eq!(all.selected(), list[0]);
        // nothing is made until a workspace is
        assert!(!base.join("workspaces").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn workspaces_are_listed_by_name_after_the_first() {
        let (base, all) = open("order");
        for name in ["Ink 10", "sketches", "Ink 2", "Comics"] {
            assert!(all.create(name.to_owned()).is_some());
        }
        assert_eq!(names(&all), ["Gallery", "Comics", "Ink 2", "Ink 10", "sketches"]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_name_is_made_one_a_folder_can_have_and_no_other_has() {
        let (base, all) = open("names");
        let made = |name: &str| all.create(name.to_owned()).map(|w| w.name);
        assert_eq!(made("  Work/Clients: 2026 ").as_deref(), Some("Work-Clients- 2026"));
        assert_eq!(made("..hidden").as_deref(), Some("hidden"));
        assert_eq!(made("two\nlines").as_deref(), Some("two lines"));
        // the first one's name is taken, and a name in another case is the same folder
        assert_eq!(made("Gallery").as_deref(), Some("Gallery 2"));
        assert_eq!(made("gallery").as_deref(), Some("gallery 3"));
        assert_eq!(made("HIDDEN").as_deref(), Some("HIDDEN 2"));
        assert_eq!(made(&"x".repeat(200)).map(|n| n.chars().count()), Some(LONGEST_NAME));
        for nothing in ["", "   ", "...", " . "] {
            assert_eq!(made(nothing), None, "{nothing:?}");
        }
        // each is a folder among the others, none outside
        for w in all.list().into_iter().skip(1) {
            assert_eq!(Path::new(&w.root).parent(), Some(base.join("workspaces").as_path()), "{w:?}");
            assert!(Path::new(&w.root).is_dir());
        }
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn the_workspace_picked_is_remembered_until_it_is_gone() {
        let (base, all) = open("selected");
        let ink = all.create("Ink".to_owned()).unwrap();
        let comics = all.create("Comics".to_owned()).unwrap();
        all.select(ink.clone());
        assert_eq!(all.selected(), ink);
        // read again as the app starts again
        let again = Workspaces::open(
            base.join("artworks").to_string_lossy().into_owned(),
            base.join("workspaces").to_string_lossy().into_owned(),
        );
        assert_eq!(again.selected(), ink);
        all.select(comics.clone());
        assert_eq!(all.selected(), comics);
        all.select(all.list()[0].clone());
        assert!(all.selected().is_default);
        // its folder taken away from under it, the first shows
        all.select(ink.clone());
        fs::remove_dir_all(&ink.root).unwrap();
        assert!(all.selected().is_default);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_workspace_renamed_keeps_its_artworks_and_stays_the_one_showing() {
        let (base, all) = open("rename");
        let ink = all.create("Ink".to_owned()).unwrap();
        all.create("Comics".to_owned()).unwrap();
        artwork(&ink.root, None);
        all.select(ink.clone());
        let pens = all.rename(ink.clone(), " Pens ".to_owned()).unwrap();
        assert_eq!(pens.name, "Pens");
        assert_eq!(names(&all), ["Gallery", "Comics", "Pens"]);
        assert_eq!(all.selected(), pens);
        assert_eq!(all.artwork_count(pens.clone()), 1);
        assert_eq!(ArtworkStore::open(pens.root.clone()).list(None).len(), 1);
        // the same name is no change, another's name gets a number, its own in another case is its own
        assert_eq!(all.rename(pens.clone(), "Pens".to_owned()), Some(pens.clone()));
        let numbered = all.rename(pens.clone(), "comics".to_owned()).unwrap();
        assert_eq!(numbered.name, "comics 2");
        let cased = all.rename(numbered, "COMICS 2".to_owned()).unwrap();
        assert_eq!(cased.name, "COMICS 2");
        assert_eq!(all.selected(), cased);
        // one not showing renamed leaves the one showing
        let comics = all.list().into_iter().find(|w| w.name == "Comics").unwrap();
        assert_eq!(all.rename(comics, "Strips".to_owned()).map(|w| w.name).as_deref(), Some("Strips"));
        assert_eq!(all.selected(), cased);
        // nothing left of the name, or the first one: no
        assert_eq!(all.rename(cased.clone(), " / ".to_owned()).map(|w| w.name).as_deref(), Some("-"));
        assert_eq!(all.rename(all.selected(), "  ".to_owned()), None);
        assert_eq!(all.rename(all.list()[0].clone(), "Mine".to_owned()), None);
        assert_eq!(names(&all)[0], "Gallery");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn a_workspace_deleted_goes_with_its_artworks_and_the_first_never_does() {
        let (base, all) = open("delete");
        let first = all.list()[0].clone();
        artwork(&first.root, None);
        let ink = all.create("Ink".to_owned()).unwrap();
        let comics = all.create("Comics".to_owned()).unwrap();
        // two loose and two in a stack, and a folder that is neither
        let loose = [artwork(&ink.root, None), artwork(&ink.root, None)];
        let store = ArtworkStore::open(ink.root.clone());
        store.stack(vec![artwork(&ink.root, None), artwork(&ink.root, None)]);
        fs::create_dir_all(Path::new(&ink.root).join("stray")).unwrap();
        assert_eq!(all.artwork_count(ink.clone()), 4);
        assert_eq!(all.artwork_count(comics.clone()), 0);
        assert_eq!(all.artwork_count(first.clone()), 1);
        all.select(ink.clone());
        all.delete(ink.clone());
        assert!(!Path::new(&loose[0]).exists());
        assert!(!Path::new(&ink.root).exists());
        assert_eq!(names(&all), ["Gallery", "Comics"]);
        assert!(all.selected().is_default);
        // one not showing deleted leaves the one showing
        let pens = all.create("Pens".to_owned()).unwrap();
        all.select(comics.clone());
        all.delete(pens);
        assert_eq!(all.selected(), comics);
        all.delete(first.clone());
        assert_eq!(all.artwork_count(first), 1);
        assert_eq!(names(&all), ["Gallery", "Comics"]);
        let _ = fs::remove_dir_all(&base);
    }
}
