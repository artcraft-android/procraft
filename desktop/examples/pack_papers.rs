//! Packs the repo's paper triplets into one shareable Papers.assetlibrary, ready to host.
//! cargo run -p procraft-desktop --example pack_papers -- [out.assetlibrary]

use std::path::PathBuf;

use procraft_core::CanvasSession;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = std::env::args().nth(1).unwrap_or_else(|| "Papers.assetlibrary".to_string());
    let papers = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/papers");

    // a scratch shelf to build the library in, thrown away after
    let temp = std::env::temp_dir().join(format!("procraft-papers-{}", std::process::id()));
    std::fs::create_dir_all(&temp)?;

    let session = CanvasSession::new(64, 64);
    session.open_assets(temp.to_string_lossy().into_owned());
    let lib = session.new_asset_library().ok_or("could not create a library")?;
    session.rename_asset_library(lib, "Papers".to_string());
    let count =
        session.import_asset_paper_pack(papers.to_string_lossy().into_owned(), lib, 0).ok_or("no papers came in")?;
    if !session.export_asset_library(lib, out.clone()) {
        return Err("could not write the library".into());
    }
    println!("packed {count} papers into {out}");
    let _ = std::fs::remove_dir_all(&temp);
    Ok(())
}
