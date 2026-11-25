//! Start the same executable in a small entry point with no app state or UI.
use std::fs::OpenOptions;
#[cfg(not(test))]
use std::process::{Command, Stdio};
use std::sync::Arc;
#[cfg(not(test))]
use std::time::{Duration, Instant};

use procraft_core::{StoreDecodeKind, StoreDecoder};

const SWITCH: &str = "--store-decode";
#[cfg(not(test))]
const TIMEOUT: Duration = Duration::from_secs(60);

pub fn register() {
    procraft_core::set_store_decoder(Arc::new(ProcessDecoder));
}

struct ProcessDecoder;

impl StoreDecoder for ProcessDecoder {
    fn decode(&self, kind: StoreDecodeKind, input: String, output: String) -> Option<String> {
        decode_with_worker(kind, &input, &output).err()
    }
}

#[cfg(not(test))]
fn decode_with_worker(kind: StoreDecodeKind, input: &str, output: &str) -> Result<(), String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    use std::io::Write as _;
    let mut command = Command::new(executable);
    command
        .args([SWITCH, kind.name(), input, output])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = command.spawn().map_err(|e| format!("The Store decoder couldn't start: {e}"))?;
    #[cfg(windows)]
    let _job = match limited_job(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("The Store decoder couldn't be limited: {error}"));
        }
    };
    if !child.stdin.take().is_some_and(|mut pipe| pipe.write_all(&[1]).is_ok()) {
        let _ = child.kill();
        let _ = child.wait();
        return Err("The Store decoder couldn't receive its start signal.".to_owned());
    }
    let start = Instant::now();
    let reason = loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err("The Store decoder closed before finishing.".to_owned()),
            Ok(None) if start.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => break "The Store decoder took too long.".to_owned(),
            Err(error) => break format!("The Store decoder couldn't be reached: {error}"),
        }
    };
    let _ = child.kill();
    let _ = child.wait();
    Err(reason)
}

#[cfg(all(windows, not(test)))]
#[allow(unsafe_code)]
fn limited_job(child: &std::process::Child) -> windows::core::Result<std::os::windows::io::OwnedHandle> {
    use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::*;

    // The job is unnamed. Its handle remains owned until the parent reaps the worker.
    let job = unsafe { OwnedHandle::from_raw_handle(CreateJobObjectW(None, windows::core::PCWSTR::null())?.0) };
    let handle = HANDLE(job.as_raw_handle());
    let mut limit = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limit.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_PROCESS_MEMORY;
    limit.BasicLimitInformation.ActiveProcessLimit = 1;
    limit.ProcessMemoryLimit = 1280 << 20;

    // Both handles are live, and the information buffer matches the requested limit class.
    unsafe {
        SetInformationJobObject(
            handle,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limit).cast(),
            std::mem::size_of_val(&limit) as u32,
        )?;
        AssignProcessToJobObject(handle, HANDLE(child.as_raw_handle()))?;
    }
    Ok(job)
}

fn run(kind: StoreDecodeKind, input: &std::path::Path, output: &std::path::Path) -> Result<(), String> {
    let input = std::fs::File::open(input).map_err(|e| e.to_string())?;
    let output = OpenOptions::new().write(true).open(output).map_err(|e| e.to_string())?;
    procraft_core::run_store_decoder(kind, input, output)
        .then_some(())
        .ok_or_else(|| "The Store decoder couldn't write its result.".to_owned())
}

pub fn run_if_requested() -> Option<bool> {
    let mut args = std::env::args_os().skip(1);
    if args.next()?.to_str()? != SWITCH {
        return None;
    }
    let mut arguments = || {
        let kind = StoreDecodeKind::from_name(args.next()?.to_str()?)?;
        let input = std::path::PathBuf::from(args.next()?);
        let output = std::path::PathBuf::from(args.next()?);
        args.next().is_none().then_some((kind, input, output))
    };
    use std::io::Read as _;
    let mut ready = [0];
    Some(arguments().is_some_and(|(kind, input, output)| {
        std::io::stdin().read_exact(&mut ready).is_ok() && ready == [1] && run(kind, &input, &output).is_ok()
    }))
}

#[cfg(test)]
fn decode_with_worker(kind: StoreDecodeKind, input: &str, output: &str) -> Result<(), String> {
    run(kind, input.as_ref(), output.as_ref())
}
