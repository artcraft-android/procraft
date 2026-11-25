//! Prepared cover artwork for Store Package. The expensive inputs are shared by the picker
//! and all title and layout treatments.

mod ink;
mod layouts;
mod plugin;
pub(crate) use plugin::{plugin_cover, plugin_icon};
mod pulp;
mod surface;

use std::path::Path;
use std::sync::OnceLock;

use image::RgbaImage;
use procraft_engine::brush::Brush;
use procraft_engine::zip::ZipReader;
use tiny_skia::Pixmap;

use self::ink::BrushMarks;
use self::surface::{CoverArt, Scan};
use super::{PREVIEW_HEIGHT, PREVIEW_WIDTH, StorePreview, pack_name, seed_of};
use crate::CanvasSession;

macro_rules! templates {
    ($($group:ident: [$($variant:ident => $label:literal),+]),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum CoverTemplate {
            $($($variant,)+)+
        }

        $(const $group: &[CoverTemplate] = &[$(CoverTemplate::$variant),+];)+

        impl CoverTemplate {
            pub fn name(self) -> &'static str {
                match self { $($(Self::$variant => $label,)+)+ }
            }
        }
    };
}

templates! {
    STAMP: [
        BotanicalPrint => "Botanical Print", ImpactPoster => "Impact Poster",
        Constellation => "Constellation", BlockPrint => "Block Print",
        Overprint => "Overprint", RisingPlume => "Rising Plume", Eclipse => "Eclipse",
        SplitInk => "Split Ink", OffsetStrips => "Offset Strips", InkWindow => "Ink Window"
    ],
    BRUSH: [
        CoastalPrint => "Coastal Print", GraphicPoster => "Graphic Poster",
        CalligraphicGesture => "Calligraphic Gesture", WashFields => "Wash Fields",
        InkCurrent => "Ink Current", InkArchitecture => "Ink Architecture",
        HalftoneHorizon => "Halftone Horizon", InkWeave => "Ink Weave",
        SplitRegister => "Split Register", TornProofs => "Torn Proofs"
    ],
    PAPER: [
        CutPaperRelief => "Cut-Paper Relief", PaperPoster => "Paper Poster",
        FoldedFan => "Folded Fan", DeckledImpressions => "Deckled Impressions",
        PaperLoom => "Paper Loom", ConcentricCuts => "Concentric Cuts",
        SheetEdges => "Sheet Edges", LightTable => "Light Table",
        PaperMosaic => "Paper Mosaic", ArchiveTabs => "Archive Tabs"
    ],
}

enum Artwork {
    Ink(BrushMarks),
    Paper(Vec<Scan>),
}

/// Renderable cover inputs. Changing a title or layout reuses the same brush probes or scans.
pub struct StoreCover {
    seed: u64,
    artwork: Artwork,
    pulp: OnceLock<Pixmap>,
    square_pulp: OnceLock<Pixmap>,
}

impl StoreCover {
    pub fn templates(&self) -> Vec<CoverTemplate> {
        match &self.artwork {
            Artwork::Ink(inks) => inks.templates(),
            Artwork::Paper(scans) if !scans.is_empty() => PAPER.to_vec(),
            Artwork::Paper(_) => Vec::new(),
        }
    }

    pub fn recommended(&self) -> Option<CoverTemplate> {
        match &self.artwork {
            Artwork::Ink(inks) => inks.recommended(),
            Artwork::Paper(scans) if scans.len() > 3 && surface::varied(scans) => Some(CoverTemplate::PaperMosaic),
            Artwork::Paper(scans) if !scans.is_empty() => Some(CoverTemplate::CutPaperRelief),
            Artwork::Paper(_) => None,
        }
    }

    /// An unset template uses the recommendation. An unset title uses the full composition.
    pub fn render(&self, template: Option<CoverTemplate>, title: Option<&str>) -> StorePreview {
        self.draw(template, title, PREVIEW_HEIGHT, &self.pulp)
    }

    /// Reflows the same template in a square, retaining the complete composition.
    pub fn render_square(&self, template: Option<CoverTemplate>, title: Option<&str>) -> StorePreview {
        self.draw(template, title, PREVIEW_WIDTH, &self.square_pulp)
    }

    fn draw(
        &self,
        template: Option<CoverTemplate>,
        title: Option<&str>,
        height: u32,
        pulp: &OnceLock<Pixmap>,
    ) -> StorePreview {
        let title = title.map(str::trim).filter(|name| !name.is_empty());
        let picked = template.filter(|choice| self.templates().contains(choice)).or_else(|| self.recommended());
        let mut art = CoverArt::new(title, height);
        let substrate = match (&self.artwork, picked) {
            (Artwork::Ink(inks), Some(template)) => {
                layouts::ink::draw(&mut art, inks, template);
                Some(pulp.get_or_init(|| pulp::pressed_pulp(self.seed, height)))
            }
            (Artwork::Paper(scans), Some(template)) => {
                layouts::paper::draw(&mut art, scans, template);
                None
            }
            _ => {
                layouts::fallback(&mut art);
                Some(pulp.get_or_init(|| pulp::pressed_pulp(self.seed, height)))
            }
        };
        StorePreview { width: PREVIEW_WIDTH, height, rgba: art.finish(substrate) }
    }
}

impl CanvasSession {
    pub fn brush_set_store_cover(&self, library: u32, set: u32, seed_key: &str) -> Option<StoreCover> {
        let brushes = self.brush_set_brushes(library, set)?;
        let seed = seed_of(seed_key);
        let marks = BrushMarks::probe(self, &brushes, seed);
        Some(StoreCover { seed, artwork: Artwork::Ink(marks), pulp: OnceLock::new(), square_pulp: OnceLock::new() })
    }

    /// Named specimens are rendered independently so export can write the exact picker cover.
    pub fn brush_set_store_specimens(&self, library: u32, set: u32, name: &str) -> Vec<StorePreview> {
        let Some(brushes) = self.brush_set_brushes(library, set) else { return Vec::new() };
        self.store_specimens(library, set, &brushes, name)
    }
}

pub fn asset_pack_store_cover(path: &Path) -> Option<StoreCover> {
    let name = pack_name(path)?;
    let zip = ZipReader::open(path)?;
    Some(StoreCover {
        seed: seed_of(&name),
        artwork: Artwork::Paper(surface::pack_scans(&zip)),
        pulp: OnceLock::new(),
        square_pulp: OnceLock::new(),
    })
}

pub(super) fn stamp_specimen(session: &CanvasSession, brush: &Brush) -> Option<RgbaImage> {
    BrushMarks::stamp_settings(brush).then(|| BrushMarks::specimen_tap(session, brush)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cover_layout_draws_a_complete_square_with_or_without_a_title() {
        let session = CanvasSession::new(32, 32);
        let marks = BrushMarks::probe(&session, &[], 42);
        let paper = Scan::of(RgbaImage::from_pixel(40, 60, image::Rgba([230, 218, 190, 255]))).unwrap();
        let paper = StoreCover {
            seed: 42,
            artwork: Artwork::Paper(vec![paper]),
            pulp: OnceLock::new(),
            square_pulp: OnceLock::new(),
        };
        let check = |pixels: &[u8]| {
            assert_eq!(pixels.len(), (PREVIEW_WIDTH * PREVIEW_WIDTH * 4) as usize);
            assert!(pixels.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
        };
        for title in [Some("Studio proof"), None] {
            for &layout in STAMP.iter().chain(BRUSH) {
                // The geometry can be exercised without a GPU or a particular brush purchase.
                let mut art = CoverArt::new(title, PREVIEW_WIDTH);
                layouts::ink::draw(&mut art, &marks, layout);
                check(&art.finish(None));
            }
            for &layout in PAPER {
                let square = paper.render_square(Some(layout), title);
                assert_eq!((square.width, square.height), (PREVIEW_WIDTH, PREVIEW_WIDTH));
                check(&square.rgba);
            }
        }
    }
}
