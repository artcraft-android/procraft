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
//! - A layer that blends other than normally goes through a blending node (`BLEND_MODE_MODULE`)
//!   on its way to the composite, set to Harmony's mode of the same name. Mattes are in the
//!   drawings already: a layer's drawings are what it shows through its matte.
//! - Each sound is a sound column (no node), its file in `audio/`, placed on whole frames with
//!   where in the file it starts and stops.

use std::collections::HashSet;
use std::ops::Range;

/// Where drawings are kept, in the scene folder.
pub const ELEMENTS: &str = "elements";
/// Where sounds are kept, in the scene folder.
pub const AUDIO: &str = "audio";
/// Node names a scene has already: no layer takes one of them.
const NODES: [&str; 4] = ["Composite", "Display", "Write", "Unconnected_Composite"];

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
    pub sounds: Vec<Sound>,
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
    /// How it blends with the layers under it (a mode Harmony has none of: normally).
    pub blend: String,
}

/// A sound column: a stretch of a sound file, played from a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    /// Its column's name, as [`Names`] gave it.
    pub name: String,
    /// Its file's name in [`AUDIO`] (two sounds may share one).
    pub file: String,
    /// The frame (from 0) it starts on.
    pub start: u32,
    /// Where in its file it starts and stops, seconds.
    pub clip: Range<f64>,
}

impl Sound {
    /// Frame `start` (from 0) and the stretch of the file to play from it, for a sound heard
    /// `at` seconds into the timeline from `from` seconds into its file, for `length` seconds, at
    /// `fps`. Harmony starts sounds on whole frames, so the stretch starts earlier in the file by
    /// the part of a frame `at` is past one; if that would be before the file's start, it starts
    /// on the next frame instead, missing what plays before it. None if nothing is left to play.
    pub fn placed(at: f64, from: f64, length: f64, fps: u32) -> Option<(u32, Range<f64>)> {
        if !(at.is_finite() && from.is_finite() && length.is_finite()) {
            return None;
        }
        let fps = f64::from(fps.max(1));
        let at = at.max(0.0);
        let stop = from + length;
        // a hair under a whole frame is that frame
        let mut frame = (at * fps + 1e-9).floor();
        let mut start = from - (at - frame / fps).max(0.0);
        if start < 0.0 {
            frame += 1.0;
            start = from + (frame / fps - at);
        }
        (stop - start > 1e-9).then_some((frame as u32, start..stop))
    }

    /// The frames (from 0) it lasts at `fps`, at least 1.
    pub fn frames(&self, fps: u32) -> u32 {
        let seconds = (self.clip.end - self.clip.start).max(0.0);
        ((seconds * f64::from(fps.max(1)) - 1e-6).ceil() as u32).max(1)
    }
}

/// Harmony's name (in the blending node's `blendMode`) for `mode`, as Harmony 21 and later write
/// it; none for normal and for the modes Harmony hasn't.
pub fn blend_keyword(mode: &str) -> Option<&'static str> {
    Some(match mode {
        "Multiply" => "eMULTIPLY_OVER",
        "Darken" => "eDARKEN_OVER",
        "ColorBurn" => "eCOLOR_BURN_OVER",
        "LinearBurn" => "eLINEAR_BURN_OVER",
        "Lighten" => "eLIGHTEN_OVER",
        "Screen" => "eSCREEN_OVER",
        "ColorDodge" => "eCOLOR_DODGE_OVER",
        "Add" => "eLINEAR_DODGE_OVER",
        "Overlay" => "eOVERLAY_OVER",
        "SoftLight" => "eSOFTLIGHT_OVER",
        "HardLight" => "eHARDLIGHT_OVER",
        "VividLight" => "eVIVIDLIGHT_OVER",
        "LinearLight" => "eLINEARLIGHT_OVER",
        "PinLight" => "ePINLIGHT_OVER",
        "Difference" => "eDIFFERENCE_OVER",
        "Exclusion" => "eEXCLUSION_OVER",
        "Subtract" => "eSUBTRACT_OVER",
        "Divide" => "eDIVIDE_OVER",
        "Hue" => "eHUE_OVER",
        "Saturation" => "eSATURATION_OVER",
        "Color" => "eCOLOR_OVER",
        // Harmony spells it so
        "Luminosity" => "eLUNINOSITY_OVER",
        // Normal, and Shade, Darker Color, Lighter Color and Hard Mix, which Harmony hasn't
        _ => return None,
    })
}

/// Names Harmony takes for nodes, elements and the files named after them: ASCII letters,
/// digits, `_` and `-`, anything else made `_`. Each name given is new, whatever its case (the
/// names become folders).
#[derive(Clone, Debug, Default)]
pub struct Names {
    used: HashSet<String>,
}

impl Names {
    /// Names for a scene's layers and sounds: none of them takes the name of a node every scene
    /// has.
    pub fn scene() -> Names {
        Names { used: NODES.iter().map(|n| n.to_lowercase()).collect() }
    }

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

/// Where sound file `file` is, in the scene folder.
pub fn sound_path(file: &str) -> String {
    format!("{AUDIO}/{file}")
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
    let fps = scene.fps.max(1);
    for (i, sound) in scene.sounds.iter().enumerate() {
        let file = escaped(&sound.file);
        xml += &format!(
            "    <column type=\"1\" name=\"{}\" displayOrder=\"{}\" width=\"100\" anonymous=\"false\" disabled=\"false\" videoSync=\"true\" soundLocked=\"false\">\n",
            escaped(&sound.name),
            scene.layers.len() + i
        );
        xml += &format!(
            "     <soundSequence startFrame=\"{}\" stopFrame=\"{}\" name=\"{file}\" displayName=\"{file}\" soundColorIsValid=\"false\" soundColor=\"#000000FF\" clippingTimeStart=\"{}\" clippingTimeStop=\"{}\">\n",
            sound.start + 1,
            sound.start + sound.frames(fps),
            sound.clip.start,
            sound.clip.end
        );
        xml += "      <lipSync showSoundForm=\"0\"/>\n     </soundSequence>\n    </column>\n";
    }
    // the blending nodes, each layer's that blends other than normally, named for it
    let mut names = Names::scene();
    for name in scene.layers.iter().map(|l| &l.name).chain(scene.sounds.iter().map(|s| &s.name)) {
        names.unique(name, "Layer");
    }
    let blends: Vec<Option<(String, &str)>> = scene
        .layers
        .iter()
        .map(|l| blend_keyword(&l.blend).map(|k| (names.unique(&format!("{}_Blending", l.name), "Blending"), k)))
        .collect();
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
        if let Some(Some((blending, keyword))) = blends.get(i) {
            let blending = escaped(blending);
            xml += &format!(
                r#"     <module type="BLEND_MODE_MODULE" name="{blending}" pos="{x},-60,{z}" publishUnderTab="{blending}">
      <options>
       <collapsed val="false"/>
       <version val="1"/>
      </options>
      <attrs>
       <blendMode val="{keyword}" version="2"/>
       <clampingMode val="ApplyClampingOnRbgChannels"/>
       <flashBlendMode val="eNORMAL_BLEND"/>
      </attrs>
     </module>
"#,
                z = scene.layers.len() + i + 4,
            );
        }
    }
    xml += "    </nodeslist>\n    <linkedlist>\n     <link out=\"Composite\" in=\"Display\"/>\n     <link out=\"Composite\" in=\"Write\"/>\n";
    for (i, layer) in scene.layers.iter().enumerate() {
        let name = escaped(&layer.name);
        match blends.get(i) {
            Some(Some((blending, _))) => {
                let blending = escaped(blending);
                xml += &format!("     <link out=\"{name}\" in=\"{blending}\"/>\n");
                xml += &format!("     <link out=\"{blending}\" in=\"Composite\" inport=\"{i}\"/>\n");
            }
            _ => xml += &format!("     <link out=\"{name}\" in=\"Composite\" inport=\"{i}\"/>\n"),
        }
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

/// A scene being put together, a picture at a time: the background, then each track's frames.
pub struct SceneBuilder {
    art: crate::Artwork,
    name: String,
    names: Names,
    /// The background's layer and the number its drawing is kept by.
    background: Option<(String, Option<u32>)>,
    tracks: Vec<TrackDrawings>,
    /// Each drawing to write: its layer, its number there, and the number the app keeps it by.
    drawings: Vec<(String, u32, u32)>,
    kept: u32,
}

/// A track's drawings as they come: its layer's name, each picture by where it is painted and the
/// hash of that, and how many frames each blend mode shows in.
struct TrackDrawings {
    layer: String,
    drawings: Drawings<([u32; 4], u64)>,
    blends: Vec<(String, u32)>,
}

impl SceneBuilder {
    pub fn new(art: crate::Artwork) -> SceneBuilder {
        let mut names = Names::scene();
        let name = Names::plain(&art.name, "Scene");
        let background = art.background.then(|| (names.unique("Background", "Background"), None));
        let tracks = art
            .tracks
            .iter()
            .map(|t| TrackDrawings { layer: names.unique(t, "Layer"), drawings: Drawings::new(), blends: Vec::new() })
            .collect();
        SceneBuilder { art, name, names, background, tracks, drawings: Vec::new(), kept: 0 }
    }

    /// A picture told: the number the app is to keep it by, if it is a new drawing.
    pub fn add(&mut self, p: &crate::Picture) -> Option<u32> {
        match p.kind {
            crate::BACKGROUND => {
                let (_, kept) = self.background.as_mut()?;
                p.painted?;
                let number = self.kept;
                self.kept += 1;
                *kept = Some(number);
                Some(number)
            }
            crate::TRACK => {
                let track = self.tracks.get_mut(p.track as usize)?;
                if p.matte {
                    track.drawings.show(None);
                    return None;
                }
                let count = |blends: &mut Vec<(String, u32)>| {
                    if p.blend.is_empty() {
                        return;
                    }
                    match blends.iter_mut().find(|(b, _)| *b == p.blend) {
                        Some((_, n)) => *n += 1,
                        None => blends.push((p.blend.clone(), 1)),
                    }
                };
                if p.held {
                    track.drawings.hold();
                    if track.drawings.exposures.last().copied().flatten().is_some() {
                        count(&mut track.blends);
                    }
                    return None;
                }
                let (n, new) = track.drawings.show(p.painted.map(|at| (at, p.hash)))?;
                count(&mut track.blends);
                if !new {
                    return None;
                }
                let number = self.kept;
                self.kept += 1;
                self.drawings.push((track.layer.clone(), n, number));
                Some(number)
            }
            _ => None,
        }
    }

    /// The scene folder's files, and what to say of it.
    pub fn finish(mut self) -> (Vec<crate::File>, String) {
        let (w, h, frames) = (self.art.width, self.art.height, self.art.frames);
        let folder = self.name.clone();
        let mut files = Vec::new();
        let mut layers = Vec::new();
        let image = |path: String, kept: u32| crate::File::Image(path, w, h, vec![[kept, 0, 0, w, h, 0, 0]]);
        if let Some((layer, Some(kept))) = self.background.take() {
            files.push(image(format!("{folder}/{}", drawing_path(&layer, 1)), kept));
            layers.push(Layer {
                name: layer,
                drawings: 1,
                exposures: vec![Some(1); frames as usize],
                blend: "Normal".to_owned(),
            });
        }
        for (layer, n, kept) in &self.drawings {
            files.push(image(format!("{folder}/{}", drawing_path(layer, *n)), *kept));
        }
        for track in self.tracks {
            // a track that never showed anything has no drawings to be a layer
            if track.drawings.count() == 0 {
                continue;
            }
            // a layer has one mode: the one most of its frames show with
            let blend = track.blends.iter().max_by_key(|(_, n)| *n).map_or("Normal".to_owned(), |(b, _)| b.clone());
            let mut exposures = track.drawings.exposures.clone();
            exposures.resize(frames as usize, None);
            layers.push(Layer { name: track.layer, drawings: track.drawings.count(), exposures, blend });
        }
        // each sound file once, named apart from the columns
        let mut file_names = Names::default();
        let mut named: Vec<(u32, String)> = Vec::new();
        let mut sounds = Vec::new();
        for s in &self.art.sounds {
            let Some((start, clip)) = Sound::placed(s.at, s.from, s.length, self.art.fps) else { continue };
            let file = match named.iter().find(|(f, _)| *f == s.file) {
                Some((_, file)) => file.clone(),
                None => {
                    let file = format!("{}.{}", file_names.unique(&s.stem, "Sound"), s.extension);
                    files.push(crate::File::Sound(format!("{folder}/{}", sound_path(&file)), s.file));
                    named.push((s.file, file.clone()));
                    file
                }
            };
            sounds.push(Sound { name: self.names.unique(&s.name, "Sound"), file, start, clip });
        }
        let scene = Scene { name: folder.clone(), width: w, height: h, fps: self.art.fps, frames, layers, sounds };
        // a number of its own for the scene, from what it is
        let id = scene_id(&self.art);
        for (file, text) in self::files(&scene, id) {
            files.push(crate::File::Bytes(format!("{folder}/{file}"), text.into_bytes()));
        }
        let drawings: u32 = scene.layers.iter().map(|l| l.drawings).sum();
        let said = format!(
            "{} {}, {drawings} {}",
            scene.layers.len(),
            if scene.layers.len() == 1 { "layer" } else { "layers" },
            if drawings == 1 { "drawing" } else { "drawings" }
        );
        (files, said)
    }
}

/// A scene's id from its artwork (FNV-1a of its name, size and length).
fn scene_id(art: &crate::Artwork) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let bytes =
        art.name.bytes().chain(art.width.to_le_bytes()).chain(art.height.to_le_bytes()).chain(art.frames.to_le_bytes());
    for b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_sound_starts_on_a_whole_frame() {
        let near = |a: Option<(u32, Range<f64>)>, b: (u32, Range<f64>)| {
            let a = a.expect("placed");
            a.0 == b.0 && (a.1.start - b.1.start).abs() < 1e-9 && (a.1.end - b.1.end).abs() < 1e-9
        };
        // on a frame: as it is
        assert!(near(Sound::placed(0.5, 1.0, 2.0, 12), (6, 1.0..3.0)));
        // a twentieth of a second past frame 6: that much earlier in the file, from frame 6
        assert!(near(Sound::placed(0.55, 1.0, 2.0, 12), (6, 0.95..3.0)));
        // too near the file's start for that: from frame 7, missing what plays before it
        assert!(near(Sound::placed(0.55, 0.02, 2.0, 12), (7, (0.02 + 7.0 / 12.0 - 0.55)..2.02)));
        // nothing left
        assert_eq!(Sound::placed(0.55, 0.0, 0.01, 12), None);
        assert_eq!(Sound::placed(f64::NAN, 0.0, 1.0, 12), None);
        let sound = Sound { name: "S".into(), file: "S.wav".into(), start: 0, clip: 0.0..2.0 };
        assert_eq!(sound.frames(12), 24);
        let sound = Sound { clip: 0.0..2.01, ..sound };
        assert_eq!(sound.frames(12), 25);
    }

    #[test]
    fn a_scene_is_its_drawings_its_sounds_and_its_files() {
        use crate::{Artwork, BACKGROUND, File, Picture, TRACK};
        let art = Artwork {
            width: 40,
            height: 30,
            fps: 12,
            frames: 3,
            name: "Walk cycle".into(),
            tracks: vec!["Line art".into()],
            sounds: vec![crate::Sound {
                name: "Voice".into(),
                file: 0,
                stem: "voice".into(),
                extension: "wav".into(),
                at: 0.0,
                from: 0.0,
                length: 1.0,
            }],
            background: true,
        };
        let mut s = SceneBuilder::new(art);
        let painted = Some([0, 0, 40, 30]);
        assert_eq!(s.add(&Picture { kind: BACKGROUND, painted, hash: 1, ..Picture::default() }), Some(0));
        let line = |hash: u64| Picture {
            kind: TRACK,
            painted: Some([1, 1, 2, 2]),
            hash,
            blend: "Multiply".into(),
            ..Picture::default()
        };
        assert_eq!(s.add(&line(5)), Some(1));
        assert_eq!(s.add(&Picture { kind: TRACK, held: true, blend: "Multiply".into(), ..Picture::default() }), None);
        assert_eq!(s.add(&line(5)), None, "the first drawing again");
        let (files, said) = s.finish();
        assert_eq!(said, "2 layers, 2 drawings");
        let paths: Vec<&str> = files
            .iter()
            .map(|f| match f {
                File::Bytes(p, _) | File::Image(p, ..) | File::Sound(p, _) => p.as_str(),
            })
            .collect();
        assert!(paths.contains(&"Walk_cycle/elements/Background/Background-1.png"), "{paths:?}");
        assert!(paths.contains(&"Walk_cycle/elements/Line_art/Line_art-1.png"));
        assert!(paths.contains(&"Walk_cycle/audio/voice.wav"));
        assert!(paths.contains(&"Walk_cycle/Walk_cycle.xstage"));
        let stage = files.iter().find_map(|f| match f {
            File::Bytes(p, b) if p.ends_with(".xstage") => Some(String::from_utf8(b.clone()).unwrap()),
            _ => None,
        });
        let stage = stage.unwrap();
        assert!(stage.contains("eMULTIPLY_OVER"), "the line art blends as it showed");
        assert!(stage.contains("exposures=\"1-3\" val=\"1\""));
    }
}
