//! Toon Boom Harmony scenes: the timeline as a scene folder Harmony opens, each track a drawing
//! layer whose exposure sheet carries the timing.
//!
//! A scene folder holds `<name>.xstage` (the scene), `<name>.aux`, `scene.elementTable` and
//! `scene.versionTable` (small tables beside it), and each layer's drawings as PNG files in
//! `elements/<layer>/<layer>-<n>.png`. The format isn't documented; what is written here follows
//! scenes Harmony saves itself, as a Harmony 21 scene, which every later version opens (scenes
//! saved by Harmony 24 carry a checksum that can't be made from outside).
//!
//! - Each layer is a bitmap element (PNG drawings, no vector data) with a drawing column for its
//!   exposure sheet and a drawing (READ) node, all three named for the layer.
//! - The drawing nodes feed one composite, the bottom layer on port 0 (the back), and the
//!   composite feeds the display and a write node: the network a new scene starts with.

use std::collections::HashSet;
use std::ops::Range;

/// Where drawings are kept, in the scene folder.
pub const ELEMENTS: &str = "elements";

/// `pixmapFormat` of an element whose drawings are PNG files.
const PNG_PIXMAP_FORMAT: u32 = 10;
const SOURCE: &str = "Harmony Premium (HarmonyPremium) version 21.0.3 build 18334 2022-04-14 02:31:36";
const VERSION: u32 = 2103;
const BUILD: u32 = 18334;

/// A scene to write: its layers' drawings already numbered, and which each frame shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// Its file name (see [`Names`]).
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u32,
    /// Bottom first.
    pub layers: Vec<Layer>,
}

/// A drawing layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// Its name, as [`Names`] gave it (it names its folder and files too).
    pub name: String,
    /// Its drawings are numbered 1 to this, in the order they first show.
    pub drawings: u32,
    /// The drawing showing at each frame; none is blank.
    pub exposures: Vec<Option<u32>>,
}

/// Names Harmony takes for nodes, elements and the files named after them: ASCII letters,
/// digits, `_` and `-`, anything else made `_`. Each name given is new, whatever its case (the
/// names become folders).
#[derive(Clone, Debug, Default)]
pub struct Names {
    used: HashSet<String>,
}

impl Names {
    /// `name` as Harmony takes it; `fallback` if nothing of it is left.
    pub fn plain(name: &str, fallback: &str) -> String {
        let mut out = String::new();
        for c in name.trim().chars() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                out.push(c);
            } else if !out.ends_with('_') {
                out.push('_');
            }
        }
        let out = out.trim_matches('_');
        if out.is_empty() { fallback.to_owned() } else { out.to_owned() }
    }

    /// `name` made plain, and `_2`, `_3`... on the end if it has been given already.
    pub fn unique(&mut self, name: &str, fallback: &str) -> String {
        let base = Names::plain(name, fallback);
        let mut name = base.clone();
        let mut n = 2;
        while !self.used.insert(name.to_lowercase()) {
            name = format!("{base}_{n}");
            n += 1;
        }
        name
    }
}

/// Where drawing `n` of layer `layer` is, in the scene folder.
pub fn drawing_path(layer: &str, n: u32) -> String {
    format!("{ELEMENTS}/{layer}/{layer}-{n}.png")
}

/// A layer's exposure sheet as runs of one drawing, blanks left out: each drawing and the frames
/// (from 0) it shows for.
pub fn runs(exposures: &[Option<u32>]) -> Vec<(u32, Range<u32>)> {
    let mut out: Vec<(u32, Range<u32>)> = Vec::new();
    for (frame, shown) in exposures.iter().enumerate() {
        let frame = u32::try_from(frame).unwrap_or(u32::MAX);
        let Some(drawing) = *shown else { continue };
        match out.last_mut() {
            Some((d, run)) if *d == drawing && run.end == frame => run.end = frame + 1,
            _ => out.push((drawing, frame..frame + 1)),
        }
    }
    out
}

/// Runs of frames as Harmony writes them: frames from 1, each run `first-last` (or the frame
/// alone), separated by commas.
pub fn exposure_list(runs: &[Range<u32>]) -> String {
    runs.iter()
        .map(|r| if r.len() == 1 { format!("{}", r.start + 1) } else { format!("{}-{}", r.start + 1, r.end) })
        .collect::<Vec<_>>()
        .join(",")
}

/// `text` made safe in an XML attribute.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The scene's own files beside its drawings, each a path in the scene folder and its text:
/// `<name>.xstage`, `<name>.aux`, `scene.elementTable` and `scene.versionTable`. `id` is the
/// scene's id (any number; a new one for each export).
pub fn files(scene: &Scene, id: u64) -> Vec<(String, String)> {
    vec![
        (format!("{}.xstage", scene.name), xstage(scene, id)),
        (format!("{}.aux", scene.name), version_file(scene)),
        ("scene.elementTable".to_owned(), element_table(scene)),
        (
            "scene.versionTable".to_owned(),
            format!("<!DOCTYPE Scene>\n<Scene EnableVersionDrawing=\"0\" Name=\"{}\"/>\n", escaped(&scene.name)),
        ),
    ]
}

/// An element's opening tag, left open for what follows.
fn element(layer: &Layer, id: usize) -> String {
    let name = escaped(&layer.name);
    format!(
        "<element id=\"{id}\" elementName=\"{name}\" elementFolder=\"{name}\" pixmapFormat=\"{PNG_PIXMAP_FORMAT}\" \
         scanType=\"2\" fieldChart=\"12\" vectorType=\"0\" rootFolder=\"{ELEMENTS}\""
    )
}

fn element_table(scene: &Scene) -> String {
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<elements source=\"{SOURCE}\" version=\"{VERSION}\" build=\"{BUILD}\">\n"
    );
    for (i, layer) in scene.layers.iter().enumerate() {
        xml += &format!(" {}/>\n", element(layer, i + 1));
    }
    xml + "</elements>\n"
}

fn version_file(scene: &Scene) -> String {
    let mut xml =
        format!("<!DOCTYPE SceneVersion>\n<SceneVersion VersionName=\"{}\" ID=\"1\">\n", escaped(&scene.name));
    for (i, layer) in scene.layers.iter().enumerate() {
        let name = escaped(&layer.name);
        xml += &format!(" <Element Name=\"{name}\" ID=\"{}\" Folder=\"{name}\">\n", i + 1);
        for n in 1..=layer.drawings.max(1) {
            xml += &format!("  <Drawing Key=\"\" Name=\"{n}\" Version=\"1\"/>\n");
        }
        xml += " </Element>\n";
    }
    xml + "</SceneVersion>\n"
}

/// The scene file.
fn xstage(scene: &Scene, id: u64) -> String {
    let id = format!("{id:016x}");
    let frames = scene.frames.max(1);
    // the camera fits the frame's height; both follow the resolution as Harmony's own presets do
    let per_unit = f64::from(scene.height) / 3750.0;
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project source=\"{SOURCE}\" version=\"{VERSION}\" build=\"{BUILD}\" creator=\"harmony\">\n <elements>\n"
    );
    for (i, layer) in scene.layers.iter().enumerate() {
        xml += &format!("  {}>\n   <drawings>\n", element(layer, i + 1));
        for n in 1..=layer.drawings.max(1) {
            xml += &format!("    <dwg name=\"{n}\"/>\n");
        }
        xml += "   </drawings>\n  </element>\n";
    }
    let (w, h) = (scene.width, scene.height);
    xml += &format!(
        r#" </elements>
 <options>
  <metrics unitAspectRatioX="4" unitAspectRatioY="3" numberOfUnitsX="24" numberOfUnitsY="24" numberOfUnitsZ="12"/>
  <resolution size="{w},{h}" fovFit="VerticalFitFov" fov="41.112090439166927" projection="PerspectiveProjection"/>
  <framerate val="{fps}"/>
  <zdragging val="true"/>
  <zOrderCompatibilityWith7_3 val="false"/>
  <pixelPerModelUnitForVectorLayers val="{per_unit}"/>
  <pixelPerModelUnitForBitmapLayers val="{per_unit}"/>
  <canvasForBitmapLayers size="{w2},{h2}"/>
  <cameraInSymbols val="true"/>
  <scaleFactor val="1"/>
  <colorSpace val="sRGB"/>
 </options>
 <timelineMarkers/>
 <scenes>
  <scene name="Top" id="{id}" nbframes="{frames}" startFrame="1" stopFrame="{frames}">
   <columns>
"#,
        fps = scene.fps.max(1),
        w2 = w.saturating_mul(2),
        h2 = h.saturating_mul(2),
    );
    for (i, layer) in scene.layers.iter().enumerate() {
        let column = i + 1;
        xml += &format!(
            "    <column type=\"0\" name=\"{}\" displayOrder=\"{i}\" width=\"100\" anonymous=\"true\" id=\"{column}\">\n",
            escaped(&layer.name)
        );
        // each drawing once, with every frame it shows at
        let runs = runs(&layer.exposures);
        for n in 1..=layer.drawings {
            let shown: Vec<Range<u32>> = runs.iter().filter(|(d, _)| *d == n).map(|(_, r)| r.clone()).collect();
            if !shown.is_empty() {
                xml += &format!(
                    "     <elementSeq exposures=\"{}\" val=\"{n}\" id=\"{column}\"/>\n",
                    exposure_list(&shown)
                );
            }
        }
        xml += "    </column>\n";
    }
    // the node view: the drawings in a row over the composite, the top layer leftmost
    let spacing = 160i64;
    let count = scene.layers.len() as i64;
    let center = (count - 1).max(0) * spacing / 2;
    xml += &format!(
        r#"   </columns>
   <options>
    <defaultDisplay val="Display"/>
   </options>
   <rootgroup name="Top">
    <options>
     <collapsed val="false"/>
    </options>
    <nodeslist>
     <module type="COMPOSITE" name="Composite" pos="{center},0,1" publishUnderTab="Composite">
      <options>
       <collapsed val="false"/>
       <version val="1"/>
      </options>
      <attrs>
{composite}
      </attrs>
     </module>
     <module type="DISPLAY" name="Display" pos="{display},100,2" publishUnderTab="Display">
      <options>
       <collapsed val="false"/>
       <version val="1"/>
      </options>
     </module>
     <module type="WRITE" name="Write" pos="{write},100,3" publishUnderTab="Write">
      <options>
       <collapsed val="false"/>
       <version val="1"/>
      </options>
      <attrs>
       <exportToMovie val="false"/>
       <drawingName val="frames/final-"/>
       <moviePath val="frames/output"/>
       <movieFormat val="com.toonboom.quicktime.legacy"/>
       <movieAudio/>
       <movieVideo/>
       <movieVideoaudio/>
       <leadingZeros val="3"/>
       <start val="1"/>
       <drawingType val="TGA"/>
       <enabling>
        <filter val="ALWAYS"/>
        <filterName/>
        <filterResX val="720"/>
        <filterResY val="540"/>
       </enabling>
       <scriptMovie val="false"/>
       <colorSpace/>
       <compositePartitioning val="NoCompositePartitioning"/>
       <zPartitionRange val="1" defaultValue="1"/>
       <cleanUpPartitionFolders val="true"/>
      </attrs>
     </module>
"#,
        composite = indented(COMPOSITE_ATTRS, 7),
        display = center + 50,
        write = center - 150,
    );
    for (i, layer) in scene.layers.iter().enumerate() {
        let name = escaped(&layer.name);
        let x = (count - 1 - i as i64) * spacing;
        xml += &format!(
            r#"     <module type="READ" name="{name}" pos="{x},-120,{z}" publishUnderTab="{name}">
      <options>
       <collapsed val="false"/>
       <version val="1"/>
      </options>
      <attrs>
{attrs}
      </attrs>
     </module>
"#,
            z = i + 4,
            attrs = indented(&READ_ATTRS.replace("{column}", &name), 7),
        );
    }
    xml += "    </nodeslist>\n    <linkedlist>\n     <link out=\"Composite\" in=\"Display\"/>\n     <link out=\"Composite\" in=\"Write\"/>\n";
    for (i, layer) in scene.layers.iter().enumerate() {
        xml += &format!("     <link out=\"{}\" in=\"Composite\" inport=\"{i}\"/>\n", escaped(&layer.name));
    }
    xml += &format!(
        r#"    </linkedlist>
   </rootgroup>
   <unconnectedComposite>
    <module type="COMPOSITE" name="Unconnected_Composite" pos="0,0,0" publishUnderTab="Unconnected_Composite">
     <options>
      <collapsed val="false"/>
      <version val="1"/>
     </options>
     <attrs>
{composite}
     </attrs>
    </module>
   </unconnectedComposite>
   <metas>
    <meta type="guideList" name="guideList" creator="Harmony Premium" version="1.0">
     <guideList selectedGuide="-1"/>
    </meta>
    <meta type="alignmentGuideList" name="alignmentGuideList" creator="Harmony Premium" version="1.0">
     <alignmentGuideList selectedGuide="-1"/>
    </meta>
   </metas>
   <timelineOrderer/>
  </scene>
 </scenes>
 <symbols>
  <folder name="Symbols">
   <scene id="{id}"/>
  </folder>
 </symbols>
 <timeline>
  <scene id="{id}"/>
 </timeline>
</project>
"#,
        composite = indented(COMPOSITE_ATTRS, 6),
    );
    xml
}

/// Each line of `text` moved in `spaces`.
fn indented(text: &str, spaces: usize) -> String {
    let pad = " ".repeat(spaces);
    text.lines().map(|l| format!("{pad}{l}")).collect::<Vec<_>>().join("\n")
}

/// A bitmap composite: its inputs flattened in port order, port 0 at the back.
const COMPOSITE_ATTRS: &str = r#"<compositeMode val="compositeBitmap"/>
<flattenOutput val="true"/>
<flattenVector val="false"/>
<composite2d val="false"/>
<composite3d val="false"/>
<outputZ val="LEFTMOST"/>
<outputZInputPort val="1"/>
<applyFocus val="true"/>
<multiplier val="1" defaultValue="1"/>
<tvgPalette val="compositedPalette"/>
<mergeVector val="false"/>"#;

/// A drawing node's attributes as Harmony writes them for a new layer, reading column
/// `{column}`. The drawings are the scene's size, so they sit as they are (a pixel of the drawing
/// a pixel of the scene), and their alpha is straight (`applyMatteToColor` Y), as PNG keeps it.
const READ_ATTRS: &str = r#"<enable3d val="false"/>
<faceCamera val="false"/>
<cameraAlignment val="NO_CAMERA_ALIGNMENT"/>
<offset>
 <separate val="true"/>
 <x val="0" defaultValue="0"/>
 <y val="0" defaultValue="0"/>
 <z val="0" defaultValue="0"/>
</offset>
<scale>
 <separate val="true"/>
 <inFields val="false"/>
 <xy val="1" defaultValue="1"/>
 <x val="1" defaultValue="1"/>
 <y val="1" defaultValue="1"/>
 <z val="1" defaultValue="1"/>
</scale>
<rotation>
 <separate val="false"/>
 <anglex val="0" defaultValue="0"/>
 <angley val="0" defaultValue="0"/>
 <anglez val="0" defaultValue="0"/>
</rotation>
<angle val="0" defaultValue="0"/>
<skew val="0" defaultValue="0"/>
<pivot>
 <x val="0" defaultValue="0"/>
 <y val="0" defaultValue="0"/>
 <z val="0" defaultValue="0"/>
</pivot>
<splineOffset>
 <x val="0" defaultValue="0"/>
 <y val="0" defaultValue="0"/>
 <z val="0" defaultValue="0"/>
</splineOffset>
<ignoreParentPegScaling val="false"/>
<disableFieldRendering val="false"/>
<depth val="0"/>
<enableMinMaxAngle val="false"/>
<minAngle val="-360" defaultValue="-360"/>
<maxAngle val="360" defaultValue="360"/>
<nailForChildren val="false"/>
<ikHoldOrientation val="false"/>
<ikHoldX val="false"/>
<ikHoldY val="false"/>
<ikExcluded val="false"/>
<ikCanRotate val="true"/>
<ikCanTranslateX val="false"/>
<ikCanTranslateY val="false"/>
<ikBoneX val="0.20000000000000001" defaultValue="0.20000000000000001"/>
<ikBoneY val="0" defaultValue="0"/>
<ikStiffness val="1" defaultValue="1"/>
<drawing>
 <elementMode val="true"/>
 <element col="{column}">
  <layer/>
 </element>
 <customName>
  <name/>
  <extension val="tga"/>
  <fieldChart val="12" defaultValue="12"/>
 </customName>
</drawing>
<readOverlay val="true"/>
<readLineArt val="true"/>
<readColorArt val="true"/>
<readUnderlay val="true"/>
<overlayArtDrawingMode val="VectorDrawingMode"/>
<lineArtDrawingMode val="VectorDrawingMode"/>
<colorArtDrawingMode val="VectorDrawingMode"/>
<underlayArtDrawingMode val="VectorDrawingMode"/>
<pencilLineDeformationPreserveThickness val="false"/>
<pencilLineDeformationQuality val="Low"/>
<pencilLineDeformationSmooth val="1"/>
<pencilLineDeformationFitError val="3" defaultValue="3"/>
<readColor val="true"/>
<readTransparency val="true"/>
<colorTransformation val="Linear"/>
<colorSpace val="sRGB"/>
<applyMatteToColor val="Y"/>
<enableLineTexture val="true"/>
<antialiasingQuality val="HIGH"/>
<antialiasingExponent val="1" defaultValue="1"/>
<opacity val="100" defaultValue="100"/>
<textureFilter val="NEAREST_FILTERED"/>
<adjustPencilThickness val="false"/>
<normalLineArtThickness val="true"/>
<zoomIndependentLineArtThickness val="zoomIndependent"/>
<multLineArtThickness val="1" defaultValue="1"/>
<addLineArtThickness val="0" defaultValue="0"/>
<minLineArtThickness val="0" defaultValue="0"/>
<maxLineArtThickness val="0" defaultValue="0"/>
<useDrawingPivot val="APPLY_ON_READ_TRANSFORM"/>
<flipHor val="false"/>
<flipVert val="false"/>
<turnBeforeAlignment val="false"/>
<noClipping val="false"/>
<xClipFactor val="0"/>
<yClipFactor val="0"/>
<alignmentRule val="ASIS"/>
<morphingVelo val="0" defaultValue="0"/>
<canAnimate val="true"/>
<tileHorizontal val="false"/>
<tileVertical val="false"/>"#;

/// Numbers a layer's drawings as they come, a frame at a time: a picture met before (by
/// `key`, such as a hash of its pixels) is the drawing it was then.
#[derive(Clone, Debug, Default)]
pub struct Drawings<K> {
    known: Vec<K>,
    pub exposures: Vec<Option<u32>>,
}

impl<K: PartialEq> Drawings<K> {
    pub fn new() -> Drawings<K> {
        Drawings { known: Vec::new(), exposures: Vec::new() }
    }

    /// The next frame shows the picture known by `key` (none: nothing). Returns its drawing's
    /// number and whether it is new (to be written).
    pub fn show(&mut self, key: Option<K>) -> Option<(u32, bool)> {
        let Some(key) = key else {
            self.exposures.push(None);
            return None;
        };
        let (n, new) = match self.known.iter().position(|k| *k == key) {
            Some(i) => (i + 1, false),
            None => {
                self.known.push(key);
                (self.known.len(), true)
            }
        };
        let n = u32::try_from(n).unwrap_or(u32::MAX);
        self.exposures.push(Some(n));
        Some((n, new))
    }

    /// The last frame's drawing shows again (held).
    pub fn hold(&mut self) {
        let last = self.exposures.last().copied().flatten();
        self.exposures.push(last);
    }

    pub fn count(&self) -> u32 {
        u32::try_from(self.known.len()).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xml;

    fn walk() -> Scene {
        Scene {
            name: "Walk".into(),
            width: 1920,
            height: 1080,
            fps: 12,
            frames: 8,
            layers: vec![
                Layer { name: "Background".into(), drawings: 1, exposures: vec![Some(1); 8] },
                // 1 held 3, 2 held 2, a blank, 1 again for 2 (a cycle)
                Layer {
                    name: "Character".into(),
                    drawings: 2,
                    exposures: vec![Some(1), Some(1), Some(1), Some(2), Some(2), None, Some(1), Some(1)],
                },
            ],
        }
    }

    #[test]
    fn names_are_plain_and_each_new() {
        assert_eq!(Names::plain("  Line art (final) ", "Layer"), "Line_art_final");
        assert_eq!(Names::plain("Ébauche", "Layer"), "bauche");
        assert_eq!(Names::plain("✏️", "Layer"), "Layer");
        assert_eq!(Names::plain("a-b_c9", "Layer"), "a-b_c9");
        let mut names = Names::default();
        assert_eq!(names.unique("Track", "Layer"), "Track");
        assert_eq!(names.unique("track", "Layer"), "track_2");
        assert_eq!(names.unique("Track!", "Layer"), "Track_3");
        assert_eq!(names.unique("", "Layer"), "Layer");
        assert_eq!(names.unique("", "Layer"), "Layer_2");
    }

    #[test]
    fn exposures_become_runs_of_one_drawing() {
        let exposures = [Some(1), Some(1), Some(1), Some(2), Some(2), None, Some(1), Some(1)];
        assert_eq!(runs(&exposures), [(1, 0..3), (2, 3..5), (1, 6..8)]);
        assert_eq!(runs(&[None, None]), []);
        assert_eq!(exposure_list(&[0..3, 6..8]), "1-3,7-8");
        assert_eq!(exposure_list(&[4..5, 7..9]), "5,8-9");
        assert_eq!(drawing_path("Character", 2), "elements/Character/Character-2.png");
    }

    #[test]
    fn drawings_are_numbered_as_they_first_show() {
        let mut d = Drawings::new();
        assert_eq!(d.show(Some("a")), Some((1, true)));
        d.hold();
        assert_eq!(d.show(Some("b")), Some((2, true)));
        assert_eq!(d.show(None), None);
        d.hold();
        assert_eq!(d.show(Some("a")), Some((1, false)));
        assert_eq!(d.count(), 2);
        assert_eq!(d.exposures, [Some(1), Some(1), Some(2), None, None, Some(1)]);
    }

    #[test]
    fn the_scene_has_a_column_for_each_layer_with_its_holds() {
        let files = files(&walk(), 0xABCD);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["Walk.xstage", "Walk.aux", "scene.elementTable", "scene.versionTable"]);
        let stage = xml::parse(&files[0].1).expect("the scene reads as XML");
        assert_eq!(stage.name, "project");

        let scene = stage.find("scene").unwrap();
        assert_eq!(scene.attribute("id"), Some("000000000000abcd"));
        assert_eq!(scene.attribute("nbframes"), Some("8"));
        assert_eq!(scene.attribute("stopFrame"), Some("8"));
        assert_eq!(stage.find("framerate").unwrap().attribute("val"), Some("12"));
        assert_eq!(stage.find("resolution").unwrap().attribute("size"), Some("1920,1080"));

        // an element for each layer, with its drawings
        let elements = stage.child("elements").unwrap().all("element");
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[1].attribute("elementName"), Some("Character"));
        assert_eq!(elements[1].attribute("pixmapFormat"), Some("10"));
        assert_eq!(elements[1].attribute("rootFolder"), Some("elements"));
        let drawings: Vec<_> = elements[1].all("dwg").iter().map(|d| d.attribute("name").unwrap()).collect();
        assert_eq!(drawings, ["1", "2"]);

        // a column for each, its drawings exposed where they show
        let columns = scene.child("columns").unwrap().all("column");
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].attribute("name"), Some("Background"));
        let seq: Vec<_> = columns[0].all("elementSeq");
        assert_eq!(seq.len(), 1);
        assert_eq!(seq[0].attribute("exposures"), Some("1-8"));
        let seq: Vec<(_, _)> = columns[1]
            .all("elementSeq")
            .iter()
            .map(|s| (s.attribute("val").unwrap(), s.attribute("exposures").unwrap()))
            .collect();
        assert_eq!(seq, [("1", "1-3,7-8"), ("2", "4-5")]);
        assert!(columns.iter().all(|c| c.attribute("type") == Some("0")));

        // a READ node for each layer reading its column, into the composite bottom first
        let reads: Vec<_> = stage.all("module").into_iter().filter(|m| m.attribute("type") == Some("READ")).collect();
        assert_eq!(reads.len(), 2);
        let element = reads[1].find("element").unwrap();
        assert_eq!(element.attribute("col"), Some("Character"));
        let links: Vec<_> = stage
            .all("link")
            .iter()
            .map(|l| (l.attribute("out").unwrap(), l.attribute("in").unwrap(), l.attribute("inport")))
            .collect();
        assert_eq!(
            links,
            [
                ("Composite", "Display", None),
                ("Composite", "Write", None),
                ("Background", "Composite", Some("0")),
                ("Character", "Composite", Some("1")),
            ]
        );
    }

    #[test]
    fn the_tables_list_every_element_and_drawing() {
        let files = files(&walk(), 1);
        let aux = xml::parse(&files[1].1).unwrap();
        let elements = aux.all("Element");
        assert_eq!(elements.len(), 2);
        assert_eq!(elements[1].attribute("Folder"), Some("Character"));
        assert_eq!(elements[1].all("Drawing").len(), 2);
        let table = xml::parse(&files[2].1).unwrap();
        let ids: Vec<_> = table.all("element").iter().map(|e| e.attribute("id").unwrap()).collect();
        assert_eq!(ids, ["1", "2"]);
        let version = xml::parse(&files[3].1).unwrap();
        assert_eq!(version.attribute("Name"), Some("Walk"));
    }

    #[test]
    fn names_are_escaped_in_the_scene() {
        let mut scene = walk();
        scene.name = "A&B".into();
        let files = files(&scene, 1);
        assert!(files[3].1.contains("Name=\"A&amp;B\""));
        assert!(xml::parse(&files[0].1).is_some());
    }
}
