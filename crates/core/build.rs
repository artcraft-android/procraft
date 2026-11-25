//! The key the big pictures of our shapes and grains are kept on disk under (see
//! `src/big_pictures.rs`), given to the code as PROCRAFT_DRAWING_KEY: FNV-1a of the code that
//! draws them and the tables it draws from, so that a build that draws them differently makes them
//! again rather than reading another's.

/// The files whose code or tables the pictures are drawn by.
const DRAWING: [&str; 3] = ["../engine/src/textures.rs", "../engine/src/stock_tones.rs", "src/stand_ins.rs"];

/// Source files that determine which built-in brush each archived preview belongs to.
const STOCK_BRUSHES: [&str; 2] = ["src/stock_brushes.rs", "../engine/src/library.rs"];

fn main() {
    println!("cargo:rustc-env=PROCRAFT_DRAWING_KEY={:016x}", source_key(&DRAWING));
    println!("cargo:rustc-env=PROCRAFT_STOCK_BRUSH_KEY={:016x}", source_key(&STOCK_BRUSHES));
}

fn source_key(files: &[&str]) -> u64 {
    let mut key = 0xcbf2_9ce4_8422_2325u64;
    for path in files {
        println!("cargo:rerun-if-changed={path}");
        let text = std::fs::read(path).unwrap_or_default();
        // (the same for a checkout with Windows line ends)
        for byte in path.bytes().chain(text.into_iter().filter(|&b| b != b'\r')) {
            key = (key ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    }
    key
}
