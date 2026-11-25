//! The animation as a Toon Boom Harmony scene, as the iPad's (CanvasActions.swift `shareHarmony`,
//! HarmonyExport.swift): the scene folder, zipped, saved where the person picks, the export card
//! showing how far it has come. Harmony reads WAV and AIFF, so each other sound the timeline plays
//! (an m4a, an mp3) is copied as WAV first by the system's own converter (a Mac's afconvert,
//! elsewhere ffmpeg where it is installed); a sound none can copy is left out of the scene, which
//! is whole without it.
//!
//! And how an export of several files (the scene, a sprite sheet and its JSON) is written: in a
//! folder of its own beside where they go, then moved into place once all are whole.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use procraft_core::{CanvasSession, HarmonyMade};

/// The scene's file name for an artwork named `name`: "Name Harmony.zip".
pub fn file_name(name: String) -> String {
    format!("{} Harmony.zip", procraft_core::export_file_name(name, false))
}

/// Share > Harmony Scene: the scene zipped where the person picks.
pub fn share(session: &Arc<CanvasSession>, card: &mut crate::export_card::ExportCard) {
    if card.busy() {
        return;
    }
    let Some(path) = crate::files::dialog()
        .set_title("Share as Harmony Scene")
        .set_file_name(file_name(session.name()))
        .add_filter("Harmony scene (zipped)", &["zip"])
        .save_file()
    else {
        return;
    };
    let path =
        if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")) { path } else { path.with_extension("zip") };
    let (Some(folder), Some(name)) = (path.parent().map(Path::to_path_buf), path.file_name()) else { return };
    let name = name.to_string_lossy().into_owned();
    write_placed(session, card, folder, move |session, part| {
        let at = part.join(&name);
        harmony_zip(session, &at)?;
        Ok(vec![at])
    });
}

/// The animation as a Harmony scene zipped at `path`, its sounds Harmony can't read copied as WAV
/// first (in a folder of their own, gone afterwards). Slow: off the window's thread.
pub fn harmony_zip(session: &CanvasSession, path: &Path) -> Result<HarmonyMade, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let sounds = std::env::temp_dir().join(format!("procraft-harmony-sounds-{}-{nanos}", std::process::id()));
    let mut wav = HashMap::new();
    for source in session.harmony_sounds_to_convert() {
        let out = sounds.join(format!("{}.wav", wav.len()));
        if wav_copy(Path::new(&source), &out) {
            wav.insert(source, out.to_string_lossy().into_owned());
        }
    }
    let made = session.export_harmony(path.to_string_lossy().into_owned(), wav).map_err(|e| e.to_string());
    let _ = std::fs::remove_dir_all(&sounds);
    made
}

/// The sound at `source` written at `to` as 16-bit WAV, its rate and channels as they are, by the
/// system's converter: a Mac's afconvert, elsewhere ffmpeg if it is installed. False if it can't
/// be (nothing is left at `to`).
pub fn wav_copy(source: &Path, to: &Path) -> bool {
    if to.parent().is_some_and(|p| std::fs::create_dir_all(p).is_err()) {
        return false;
    }
    let mut command = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("afconvert");
        c.args(["-f", "WAVE", "-d", "LEI16"]).arg(source).arg(to);
        c
    } else {
        let mut c = std::process::Command::new("ffmpeg");
        c.args(["-nostdin", "-y", "-loglevel", "error", "-i"]).arg(source).args(["-vn", "-c:a", "pcm_s16le"]).arg(to);
        c
    };
    // (no console window flashing up on Windows)
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000);
    let ran = command.stdin(std::process::Stdio::null()).output();
    // a WAV's header alone is 44 bytes: more is a sound
    let copied = ran.is_ok_and(|o| o.status.success()) && std::fs::metadata(to).is_ok_and(|m| m.len() > 44);
    if !copied {
        let _ = std::fs::remove_file(to);
    }
    copied
}

/// Writes an export on a thread of its own (among those the app waits for as it ends), `card`
/// showing how far the core says it has come. `write` makes its files in the folder it is handed,
/// a part folder of its own beside `to`, so what is there stays until the new files are whole;
/// they are then moved into `to`. Cancelled or failed, the part folder goes and nothing in `to`
/// changes; a failure is told.
pub(crate) fn write_placed(
    session: &Arc<CanvasSession>,
    card: &mut crate::export_card::ExportCard,
    to: PathBuf,
    write: impl FnOnce(&CanvasSession, &Path) -> Result<Vec<PathBuf>, String> + Send + 'static,
) {
    if card.busy() {
        return;
    }
    session.reset_export_progress();
    let following = Arc::clone(session);
    let exporting = card.start(move || following.export_progress());
    let session = Arc::clone(session);
    crate::files::write_meanwhile(true, move || {
        let part = part_folder(&to);
        // a panic on the way (the GPU's driver, say) is said as any failure, the card let go
        let written = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| write(&session, &part)))
            .unwrap_or_else(|panic| Err(format!("it stopped: {}", procraft_gpu::panic_message(panic.as_ref()))));
        let placed = written
            .and_then(|files| if exporting.cancelled() { Ok(false) } else { place(&to, &part, &files).map(|()| true) });
        let _ = std::fs::remove_dir_all(&part);
        match placed {
            Ok(made) => exporting.ended(made),
            Err(e) => {
                exporting.ended(false);
                if !exporting.cancelled() {
                    crate::files::tell_later("Can't share", &e);
                }
            }
        }
    });
}

/// A new part folder in `to` for an export's files while they are written.
pub(crate) fn part_folder(to: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    to.join(format!(".procraft-export-{nanos}.part"))
}

/// `files`, written in the part folder `part`, moved into `to` under their own names (over any
/// there of the same name), and `part` gone.
pub(crate) fn place(to: &Path, part: &Path, files: &[PathBuf]) -> Result<(), String> {
    for f in files {
        let Some(name) = f.file_name() else { continue };
        std::fs::rename(f, to.join(name)).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(part);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-desktop-harmony-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_animation_is_zipped_as_a_harmony_scene_a_layer_for_each_track() {
        let s = CanvasSession::new(40, 40);
        if !s.prepare_offscreen() {
            eprintln!("skipped: no GPU device");
            return;
        }
        s.set_name("Bounce".into());
        // two frames, red then blue, by Animation Assist: put on a timeline for the scene
        let first = s.layers().first().map(|l| l.id).unwrap();
        let ids = [first, s.add_layer()];
        for (id, [r, g, b]) in ids.into_iter().zip([[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]) {
            s.set_color(r, g, b);
            assert!(s.fill_layer(id));
        }
        assert!(s.set_animation_assist(true));
        let to = folder("scene");
        let name = file_name(s.name());
        assert_eq!(name, "Bounce Harmony.zip");
        // as Share writes it: in a part folder, then put in place
        let part = part_folder(&to);
        std::fs::create_dir_all(&part).unwrap();
        let made = harmony_zip(&s, &part.join(&name)).unwrap();
        place(&to, &part, &[part.join(&name)]).unwrap();
        assert!(!part.exists());
        assert!(made.layers >= 1 && made.drawings >= 2, "{made:?}");
        assert_eq!(made.sounds, 0);
        let zip = procraft_engine::zip::ZipReader::open(&to.join(&name)).unwrap();
        let names: Vec<String> = zip.names().map(str::to_owned).collect();
        assert!(names.contains(&"Bounce/Bounce.xstage".to_owned()), "{names:?}");
        assert!(names.contains(&"Bounce/scene.elementTable".to_owned()), "{names:?}");
        let drawings = names.iter().filter(|n| n.ends_with(".png")).count();
        assert_eq!(drawings as u32, made.drawings, "{names:?}");
        let _ = std::fs::remove_dir_all(&to);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_sound_harmony_can_t_read_is_copied_as_wav() {
        let dir = folder("sound");
        // a tenth of a second of a tone as a WAV, made an AAC (m4a) by the system's converter
        let rate = 22_050u32;
        let samples: Vec<i16> = (0..rate / 10).map(|i| ((i as f32 * 0.1).sin() * 8000.0) as i16).collect();
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        std::fs::write(dir.join("tone.wav"), &wav).unwrap();
        let m4a = dir.join("tone.m4a");
        let made = std::process::Command::new("afconvert")
            .args(["-f", "m4af", "-d", "aac"])
            .arg(dir.join("tone.wav"))
            .arg(&m4a)
            .status()
            .is_ok_and(|s| s.success());
        if !made {
            eprintln!("skipped: afconvert couldn't make an m4a");
            return;
        }
        let back = dir.join("copies/0.wav");
        assert!(wav_copy(&m4a, &back));
        let bytes = std::fs::read(&back).unwrap();
        assert_eq!(bytes.get(..4), Some(&b"RIFF"[..]));
        assert_eq!(bytes.get(8..12), Some(&b"WAVE"[..]));
        // what can't be read leaves nothing
        std::fs::write(dir.join("not a sound.m4a"), b"nothing").unwrap();
        assert!(!wav_copy(&dir.join("not a sound.m4a"), &dir.join("copies/1.wav")));
        assert!(!dir.join("copies/1.wav").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
