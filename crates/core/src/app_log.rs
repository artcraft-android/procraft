//! The app's log kept in a file as well, so that someone whose app crashed can send what led up to
//! it: on Android the log otherwise goes only to the system's, which nothing in the app can share.
//! It is `procraft.log` in the app's own folder, with the run before's kept beside it as
//! `procraft-previous.log`, as the desktop app keeps its own.
//!
//! The interface begins it with a header (the app, the device) and adds its own warnings and
//! errors; the core's log goes in from `info` up. Each line goes into the file as it comes, with no
//! buffer in between, so a crash in native code leaves every line before it. A log that reaches
//! [`MOST`] keeps its start and its latest lines (see [`cut`]), and what could name the person or
//! their files is left out of every line (see [`redact`]).

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Instant;

/// The log, and the run before's, in the app's own folder.
const LOG: &str = "procraft.log";
const PREVIOUS_LOG: &str = "procraft-previous.log";

/// How large a log grows, bytes.
const MOST: usize = 1 << 20;

/// What a log at [`MOST`] keeps of its start (the header, the GPU) and of its latest lines, bytes.
const HEAD: usize = 128 << 10;
const TAIL: usize = 512 << 10;

/// The most one write puts in, bytes.
const WRITE_MOST: usize = 64 << 10;

/// What stands where a log was cut.
const LEFT_OUT: &str = "[... lines left out here, to keep the log under 1 MB ...]\n";

/// Where the device's shared storage is: a file's path in it names the person's own file.
const SHARED_STORAGE: [&str; 4] = ["/storage/", "/sdcard/", "/mnt/", "/data/media/"];

/// The log's file, once begun.
static KEPT: Mutex<Option<LogFile>> = Mutex::new(None);

/// When the log began: each line says how long after.
static BEGAN: OnceLock<Instant> = OnceLock::new();

/// How serious a line of the interface's log is.
#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

impl LogLevel {
    fn level(self) -> log::Level {
        match self {
            LogLevel::Info => log::Level::Info,
            LogLevel::Warn => log::Level::Warn,
            LogLevel::Error => log::Level::Error,
        }
    }
}

/// Begins the log's file in `dir` (the app's files folder), the last run's kept beside it, with
/// `header` (the app, the device) at its top. Once a process: a later call (the app's screen made
/// again) goes on in the same file.
#[uniffi::export]
pub fn start_log_file(dir: String, header: String) {
    crate::device::start_log();
    BEGAN.get_or_init(Instant::now);
    let begun = {
        let mut kept = kept();
        if kept.is_some() {
            return;
        }
        *kept = LogFile::begin(Path::new(&dir));
        kept.is_some()
    };
    if begun {
        write(&header);
    } else {
        log::warn!("the log's file couldn't be made");
    }
}

/// A line of the interface's log (`tag`: the part of the app it comes from), into the file only:
/// the interface gives it to the system's log itself.
#[uniffi::export]
pub fn log_line(level: LogLevel, tag: String, message: String) {
    write(&line(level.level(), &tag, &message));
}

/// The core's log to Android's (through `android`) and into the file, from `info` up.
#[cfg(target_os = "android")]
pub(crate) fn install(android: android_logger::AndroidLogger) {
    struct Both(android_logger::AndroidLogger);

    impl log::Log for Both {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Info
        }

        fn log(&self, record: &log::Record) {
            if !self.enabled(record.metadata()) {
                return;
            }
            log::Log::log(&self.0, record);
            write(&line(record.level(), record.target(), &record.args().to_string()));
        }

        fn flush(&self) {}
    }

    if log::set_logger(Box::leak(Box::new(Both(android)))).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}

/// A line of the log: how long after it began, how serious, where from, and what.
fn line(level: log::Level, target: &str, message: &str) -> String {
    let ms = BEGAN.get_or_init(Instant::now).elapsed().as_millis();
    format!("[{ms:>7} ms {level:<5} {target}] {message}\n")
}

/// Writes `text` into the log's file, if it has begun.
fn write(text: &str) {
    if let Some(file) = kept().as_mut() {
        file.write(text);
    }
}

fn kept() -> MutexGuard<'static, Option<LogFile>> {
    KEPT.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The log's file, and how long it is.
struct LogFile {
    path: PathBuf,
    /// None while it is cut, or if it couldn't be opened again after.
    file: Option<File>,
    length: usize,
    /// The app's own folders, shortened in what is written.
    app: Vec<String>,
}

impl LogFile {
    /// A new log in `dir`, the last run's kept beside it; none if it can't be made.
    fn begin(dir: &Path) -> Option<LogFile> {
        let path = dir.join(LOG);
        let _ = fs::rename(&path, dir.join(PREVIOUS_LOG));
        let file = File::create(&path).ok()?;
        Some(LogFile { path, file: Some(file), length: 0, app: app_folders(dir) })
    }

    /// Writes `text` at the log's end, as whole lines: what could name the person left out, the
    /// log cut first if it would go past its most.
    fn write(&mut self, text: &str) {
        let mut text = trimmed(&redact(text, &self.app), WRITE_MOST);
        if !text.ends_with('\n') {
            text.push('\n');
        }
        if self.length + text.len() > MOST {
            self.cut();
        }
        let Some(file) = self.file.as_mut() else { return };
        if file.write_all(text.as_bytes()).is_ok() {
            self.length += text.len();
        }
    }

    /// The log, at its most, with its middle left out (see [`cut`]), in place of the whole.
    fn cut(&mut self) {
        // let go of it first: the cut log is put in its place
        self.file = None;
        let whole = fs::read(&self.path).unwrap_or_default();
        let kept = cut(&whole, HEAD, TAIL);
        let written = crate::project::write_atomically(&self.path, |tmp| {
            fs::write(tmp, &kept).map_err(|e| crate::project::file_error("the log", e))
        });
        // not written (the device full): begun again, rather than let grow past its most
        self.length = match written {
            Ok(()) => kept.len(),
            Err(_) => {
                let _ = File::create(&self.path);
                0
            }
        };
        self.file = OpenOptions::new().append(true).open(&self.path).ok();
    }
}

/// `log` with its middle left out: `head` bytes of its start and `tail` of its end, each to whole
/// lines, with a line saying so between them. As it is if it is no longer.
fn cut(log: &[u8], head: usize, tail: usize) -> Vec<u8> {
    if log.len() <= head + tail {
        return log.to_vec();
    }
    let start = log.get(..head).unwrap_or_default();
    let start = match start.iter().rposition(|&b| b == b'\n') {
        Some(i) => start.get(..=i).unwrap_or(start),
        None => start,
    };
    let end = log.get(log.len() - tail..).unwrap_or_default();
    let end = match end.iter().position(|&b| b == b'\n') {
        Some(i) => end.get(i + 1..).unwrap_or(end),
        None => end,
    };
    [start, LEFT_OUT.as_bytes(), end].concat()
}

/// `text`, or its start if it is longer than `most` bytes (to a line's end where there is one), with
/// a line saying how much was left out.
fn trimmed(text: &str, most: usize) -> String {
    if text.len() <= most {
        return text.to_owned();
    }
    let end = (0..=most).rev().find(|&i| text.is_char_boundary(i)).unwrap_or(0);
    let start = text.get(..end).unwrap_or_default();
    let start = match start.rfind('\n') {
        Some(i) if i > 0 => start.get(..=i).unwrap_or(start),
        _ => start,
    };
    let gap = if start.ends_with('\n') { "" } else { "\n" };
    format!("{start}{gap}[... {} KB more left out ...]\n", (text.len() - start.len()).div_ceil(1024))
}

/// The app's own folders, from `dir`, its files' folder: the app's data folder, and its other name
/// on Android (`/data/data/<package>`).
fn app_folders(dir: &Path) -> Vec<String> {
    let Some(data) = dir.parent().filter(|d| d.components().count() > 2) else { return Vec::new() };
    let mut folders = vec![data.to_string_lossy().into_owned()];
    if let Some(package) = data.file_name() {
        let other = format!("/data/data/{}", package.to_string_lossy());
        if !folders.contains(&other) {
            folders.push(other);
        }
    }
    folders
}

/// `text` with what could name the person or their files left out:
/// - the name in a home folder's path (`/home/<name>`, `/Users/<name>`, `C:\Users\<name>`), as
///   `<home>`, the rest of the path kept;
/// - the path of a file in the device's shared storage (`/storage/...`, `/sdcard/...`), and the
///   address of a file another app handed over (`content://...`, `file://...`), with only the
///   file's kind (its extension) kept, and the app that handed it over;
/// - the app's own folders (`app`), shortened to `<app>`, the rest of the path kept: nothing in
///   them is named by the person (an artwork's folder has a name of its own).
pub(crate) fn redact(text: &str, app: &[String]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let taken = if c == '/' || c.is_ascii_alphabetic() { left_out(rest, app, &mut out) } else { None };
        let n = taken.unwrap_or_else(|| {
            out.push(c);
            c.len_utf8()
        });
        rest = rest.get(n..).unwrap_or_default();
    }
    out
}

/// What is said in place of what `rest` begins with, if that is left out (see [`redact`]), put in
/// `out`; the bytes of `rest` it stands for.
fn left_out(rest: &str, app: &[String], out: &mut String) -> Option<usize> {
    if let Some(folder) = app.iter().find(|a| !a.is_empty() && rest.starts_with(a.as_str())) {
        out.push_str("<app>");
        return Some(folder.len());
    }
    if let Some(n) = home(rest) {
        out.push_str("<home>");
        return Some(n);
    }
    if starts_with_any_case(rest, "content://") {
        let n = address_length(rest);
        let path = rest.get("content://".len()..n)?;
        // only the app that handed it over: an address with no file in it says nothing
        let (from, file) = path.split_once('/')?;
        out.push_str(&format!("content://{from}/...{}", extension(file)));
        return Some(n);
    }
    if starts_with_any_case(rest, "file://") {
        let n = address_length(rest);
        out.push_str(&format!("file://...{}", extension(rest.get(..n)?)));
        return Some(n);
    }
    if SHARED_STORAGE.iter().any(|root| rest.starts_with(root)) {
        let n = path_length(rest);
        out.push_str(&format!("<storage>/...{}", extension(rest.get(..n)?)));
        return Some(n);
    }
    None
}

/// How long the home folder's path at `rest`'s start is, with the name in it (`/home/<name>`,
/// `/Users/<name>`, `C:\Users\<name>`); none if it doesn't start with one.
fn home(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let drive = bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && matches!(bytes.get(2), Some(b'\\' | b'/'))
        && rest.get(3..8).is_some_and(|w| w.eq_ignore_ascii_case("users"))
        && matches!(bytes.get(8), Some(b'\\' | b'/'));
    let prefix =
        if drive { 9 } else { ["/home/", "/Users/"].iter().find(|p| rest.starts_with(*p)).map(|p| p.len())? };
    // to the next folder: a space doesn't end it, as a name may have one
    let name = rest.get(prefix..)?;
    let n = name.find(|c: char| "/\\\n\r\t\"'`<>()[]{}|:,;".contains(c)).unwrap_or(name.len());
    (n > 0).then_some(prefix + n)
}

/// How far the address at `rest`'s start goes: to a space, a quote or a bracket.
fn address_length(rest: &str) -> usize {
    rest.find(ends_address).unwrap_or(rest.len())
}

fn ends_address(c: char) -> bool {
    c.is_whitespace() || "\"'`<>()[]{}|".contains(c)
}

/// How far the path at `rest`'s start goes: to the end of its line, a quote or a bracket, or a
/// colon, comma or semicolon before a space. A space doesn't end it: a file's name may have them.
fn path_length(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    let ends = |i: usize, b: u8| match b {
        b'\n' | b'\r' | b'\t' | b'"' | b'\'' | b'`' | b'<' | b'>' | b'(' | b')' | b'[' | b']' | b'{' | b'}' | b'|' => {
            true
        }
        b':' | b',' | b';' => bytes.get(i + 1).is_none_or(u8::is_ascii_whitespace),
        _ => false,
    };
    bytes.iter().enumerate().position(|(i, &b)| ends(i, b)).unwrap_or(bytes.len())
}

/// The extension of the file `path` names (with its dot), if it has a plain one; else nothing.
fn extension(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) if (1..=10).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric()) => {
            format!(".{ext}")
        }
        _ => String::new(),
    }
}

fn starts_with_any_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len()).is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-log-{name}-{}", std::process::id())).join("files");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_new_log_keeps_the_last_runs_beside_it() {
        let dir = folder("roll");
        let mut first = LogFile::begin(&dir).unwrap();
        first.write("the first run\n");
        drop(first);
        let mut second = LogFile::begin(&dir).unwrap();
        second.write("the second run\n");
        drop(second);
        assert_eq!(fs::read_to_string(dir.join(PREVIOUS_LOG)).unwrap(), "the first run\n");
        assert_eq!(fs::read_to_string(dir.join(LOG)).unwrap(), "the second run\n");
        // the one before that goes
        LogFile::begin(&dir).unwrap();
        assert_eq!(fs::read_to_string(dir.join(PREVIOUS_LOG)).unwrap(), "the second run\n");
        assert_eq!(fs::read_to_string(dir.join(LOG)).unwrap(), "");
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_log_at_its_most_keeps_its_start_and_its_latest_lines() {
        let dir = folder("cap");
        let mut log = LogFile::begin(&dir).unwrap();
        log.write("the header\n");
        for i in 0..40_000 {
            log.write(&format!("line {i} of the run, long enough to fill a megabyte soon\n"));
        }
        drop(log);
        let text = fs::read_to_string(dir.join(LOG)).unwrap();
        assert!(text.len() <= MOST, "{} bytes", text.len());
        assert!(text.starts_with("the header\nline 0 of the run"), "{}", &text[..100]);
        assert!(text.ends_with("line 39999 of the run, long enough to fill a megabyte soon\n"));
        assert!(text.contains(LEFT_OUT));
        // whole lines either side of the cut, and every line kept is whole
        assert!(text.lines().all(|l| l == "the header" || l == LEFT_OUT.trim_end() || l.ends_with("megabyte soon")));
        // the lines kept follow on from one another after the cut, up to the last
        let after: Vec<usize> = text
            .rsplit(LEFT_OUT)
            .next()
            .unwrap()
            .lines()
            .map(|l| l.split(' ').nth(1).unwrap().parse().unwrap())
            .collect();
        assert!(after.windows(2).all(|w| w[1] == w[0] + 1));
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_cut_keeps_whole_lines_from_either_end() {
        let log = b"aaaa\nbbbb\ncccc\ndddd\neeee\n";
        assert_eq!(cut(log, 100, 100), log.to_vec());
        let kept = String::from_utf8(cut(log, 7, 7)).unwrap();
        assert_eq!(kept, format!("aaaa\n{LEFT_OUT}eeee\n"));
        // a line longer than what is kept: cut where it falls
        let kept = String::from_utf8(cut(b"aaaaaaaaaa\nbbbbbbbbbb", 4, 4)).unwrap();
        assert_eq!(kept, format!("aaaa{LEFT_OUT}bbbb"));
    }

    #[test]
    fn a_write_too_long_is_trimmed_to_whole_lines() {
        assert_eq!(trimmed("short\n", 10), "short\n");
        assert_eq!(trimmed("one\ntwo\nthree\n", 9), "one\ntwo\n[... 1 KB more left out ...]\n");
        // no line's end in reach, and a letter of two bytes not split
        assert_eq!(trimmed("ééééé", 5), "éé\n[... 1 KB more left out ...]\n");
    }

    #[test]
    fn what_could_name_the_person_or_their_files_is_left_out() {
        let app = app_folders(Path::new("/data/user/0/io.github.artcraftandroid.procraft/files"));
        let r = |text: &str| redact(text, &app);
        // the app's own folders, shortened
        assert_eq!(
            r("keeping /data/user/0/io.github.artcraftandroid.procraft/files/artworks/a1b2/doc.json: no room"),
            "keeping <app>/files/artworks/a1b2/doc.json: no room"
        );
        assert_eq!(r("/data/data/io.github.artcraftandroid.procraft/cache/x"), "<app>/cache/x");
        // home folders: the name goes, the rest of the path stays
        assert_eq!(
            r("panicked at /home/jo/procraft/crates/core/src/lib.rs:10:5"),
            "panicked at <home>/procraft/crates/core/src/lib.rs:10:5"
        );
        assert_eq!(r("/Users/Jo Smith/x.rs"), "<home>/x.rs");
        assert_eq!(r(r"at C:\Users\jo\src\main.rs and c:/users/jo/x"), r"at <home>\src\main.rs and <home>/x");
        // a file of the person's: only its kind stays, whatever its name holds
        assert_eq!(
            r("java.io.FileNotFoundException: /storage/emulated/0/Download/My Drawing (2).psd: open failed: EACCES"),
            "java.io.FileNotFoundException: <storage>/...(2).psd: open failed: EACCES"
        );
        assert_eq!(r("reading /sdcard/Pictures/holiday.jpg, 2 MB"), "reading <storage>/....jpg, 2 MB");
        assert_eq!(
            r("opening content://com.android.providers.downloads.documents/document/msf%3A1000.procraft failed"),
            "opening content://com.android.providers.downloads.documents/....procraft failed"
        );
        assert_eq!(r("(file:///storage/emulated/0/Secret%20name.brushset)"), "(file://....brushset)");
        // nothing else changes: words, numbers, the system's own paths, an address with no file
        let plain = "GPU: Mali-G715 (Vulkan, driver 1.3), /vendor/lib64/egl/libGLES_mali.so at 0x7a, content://media é";
        assert_eq!(r(plain), plain);
        // and what has been through it once stays as it is
        let once = r(
            "at /storage/emulated/0/a.psd\nand content://x/y/z.png and /home/jo/q\n/data/data/io.github.artcraftandroid.procraft/f",
        );
        assert_eq!(once, "at <storage>/....psd\nand content://x/....png and <home>/q\n<app>/f");
        assert_eq!(r(&once), once);
        // a space in a file's path doesn't end it, so the rest of its line goes too
        assert_eq!(r("reading /storage/emulated/0/My Art/a b.png took 3 ms"), "reading <storage>/...");
    }

    #[test]
    fn written_lines_leave_out_what_could_name_the_person() {
        let dir = folder("redact");
        let mut log = LogFile::begin(&dir).unwrap();
        let given = format!(
            "[ 1 ms WARN  ProCraft] importing a PSD\njava.io.IOException: {}: gone\n/storage/emulated/0/Download/me.psd\n",
            dir.join("x").display()
        );
        log.write(&given);
        drop(log);
        let text = fs::read_to_string(dir.join(LOG)).unwrap();
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(
            text,
            format!(
                "[ 1 ms WARN  ProCraft] importing a PSD\njava.io.IOException: <app>{sep}files{sep}x: gone\n<storage>/....psd\n"
            )
        );
        fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn a_line_says_how_long_after_the_start_how_serious_and_where_from() {
        let l = line(log::Level::Warn, "ProCraft", "importing a PSD");
        assert!(l.starts_with('['), "{l}");
        assert!(l.ends_with(" ms WARN  ProCraft] importing a PSD\n"), "{l}");
    }
}
