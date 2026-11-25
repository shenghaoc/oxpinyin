use crate::{Result, abi, require, workload};
use std::ffi::{CStr, c_void};
use std::io::Write;

// The subject reaches CoreFoundation through GLib's gettext (libintl) and, with it,
// a closure of about 400 images. A driver that links the subject lets dyld load that
// closure at launch; dlopen loads it after launch, which adds about 4 MB of resident
// file-backed pages and 0.3 MB of footprint to this process (measured with a C
// program that calls dlopen, and again with CoreFoundation alone). Linking the
// framework here puts that load back at launch, so the memory figures stay
// comparable with a driver linked to the subject.
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {}

fn rusage() -> Result<libc::rusage_info_v4> {
    // SAFETY: rusage_info_v4 contains integer fields and arrays that admit zero.
    let mut value: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    // SAFETY: getpid has no caller preconditions.
    let pid = unsafe { libc::getpid() };
    // SAFETY: value is writable storage with the RUSAGE_INFO_V4 layout.
    let status = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V4,
            (&mut value as *mut libc::rusage_info_v4).cast(),
        )
    };
    require(status == 0, "measurement: proc_pid_rusage failed")?;
    Ok(value)
}

fn process_cpu() -> Result<f64> {
    // SAFETY: rusage contains only integer fields and aggregates that admit zero.
    let mut value: libc::rusage = unsafe { std::mem::zeroed() };
    // SAFETY: value points to writable storage for RUSAGE_SELF's rusage output.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut value) };
    require(status == 0, "measurement: getrusage failed")?;
    Ok(value.ru_utime.tv_sec as f64
        + value.ru_stime.tv_sec as f64
        + (value.ru_utime.tv_usec + value.ru_stime.tv_usec) as f64 / 1e6)
}
#[expect(
    deprecated,
    reason = "The selected libc dependency exposes Mach timebase through this deprecated binding"
)]
fn timebase() -> Result<f64> {
    // SAFETY: mach_timebase_info contains only integer fields, which admit zero.
    let mut info: libc::mach_timebase_info = unsafe { std::mem::zeroed() };
    // SAFETY: info points to writable storage with the Mach API's output layout.
    let status = unsafe { libc::mach_timebase_info(&mut info) };
    require(
        status == 0 && info.denom != 0,
        "measurement: Mach timebase capture failed",
    )?;
    Ok(info.numer as f64 / info.denom as f64 / 1e9)
}

fn image(
    init: extern "C" fn(*const std::ffi::c_char, *const std::ffi::c_char) -> abi::Handle,
    path: &str,
    uuid: &str,
) -> Result<()> {
    // SAFETY: Dl_info contains only raw pointers, which admit null.
    let mut info: libc::Dl_info = unsafe { std::mem::zeroed() };
    // SAFETY: init is a live symbol in the loaded subject; info is writable.
    let status = unsafe { libc::dladdr(init as *const c_void, &mut info) };
    require(
        status != 0 && !info.dli_fname.is_null(),
        "measurement: wrong loaded image or UUID",
    )?;
    // SAFETY: successful dladdr supplied this non-null, NUL-terminated name
    // from the still-loaded image.
    let loaded = unsafe { CStr::from_ptr(info.dli_fname) }.to_str()?;
    require(
        std::fs::canonicalize(loaded)? == std::fs::canonicalize(path)?,
        "measurement: wrong loaded image or UUID",
    )?;
    // SAFETY: dladdr's base addresses the aligned, loader-mapped Mach-O header.
    let header = unsafe { std::slice::from_raw_parts(info.dli_fbase.cast::<u32>(), 8) };
    require(
        header[0] == 0xfeedfacf,
        "measurement: wrong loaded image or UUID",
    )?;
    // SAFETY: the mapped 64-bit Mach-O header is 32 bytes, followed by load commands.
    let commands = unsafe { info.dli_fbase.cast::<u8>().add(32) };
    // SAFETY: the loader mapped the command area of size sizeofcmds (header[5]).
    let commands = unsafe { std::slice::from_raw_parts(commands, header[5] as usize) };
    require(
        load_uuid(commands, header[4])? == uuid.as_bytes(),
        "measurement: wrong loaded image or UUID",
    )
}

/// The lowercase hexadecimal LC_UUID of an image, from its bounded load commands.
fn load_uuid(commands: &[u8], count: u32) -> Result<[u8; 32]> {
    let mut cursor = commands;
    for _ in 0..count {
        require(cursor.len() >= 8, "measurement: invalid Mach-O command")?;
        let command = u32::from_ne_bytes([cursor[0], cursor[1], cursor[2], cursor[3]]);
        let size = u32::from_ne_bytes([cursor[4], cursor[5], cursor[6], cursor[7]]) as usize;
        require(
            size >= 8 && size <= cursor.len(),
            "measurement: invalid Mach-O command",
        )?;
        if command == 0x1b {
            require(size == 24, "measurement: wrong loaded image or UUID")?;
            let mut actual = [0u8; 32];
            const HEX: &[u8; 16] = b"0123456789abcdef";
            for (i, byte) in cursor[8..24].iter().enumerate() {
                actual[2 * i] = HEX[(byte >> 4) as usize];
                actual[2 * i + 1] = HEX[(byte & 15) as usize];
            }
            return Ok(actual);
        }
        cursor = &cursor[size..];
    }
    Err("measurement: wrong loaded image or UUID".into())
}

/// The counters read around the typing loop.
struct Loop {
    fingerprint: u64,
    scale: f64,
    entered: libc::rusage_info_v4,
    left: libc::rusage_info_v4,
    cpu: f64,
    start: u64,
    stop: u64,
}

fn timed(cycle: &mut workload::Cycle<'_>, cycles: usize) -> Result<Loop> {
    let mut fingerprint = workload::FNV_OFFSET;
    let scale = timebase()?;
    let entered = rusage()?;
    let cpu_before = process_cpu()?;
    let start = super::now_ns();
    for _ in 0..cycles {
        fingerprint ^= cycle.run();
        fingerprint = fingerprint.wrapping_mul(workload::FNV_PRIME);
    }
    let stop = super::now_ns();
    let cpu = process_cpu()? - cpu_before;
    let left = rusage()?;
    Ok(Loop {
        fingerprint,
        scale,
        entered,
        left,
        cpu,
        start,
        stop,
    })
}

fn complete(
    measured: &Loop,
    memory: &libc::rusage_info_v4,
    cycle: &workload::Cycle<'_>,
    expected: u64,
) -> bool {
    measured.start != 0
        && measured.stop > measured.start
        && measured.cpu > 0.0
        && measured.left.ri_instructions > measured.entered.ri_instructions
        && measured.left.ri_cycles > measured.entered.ri_cycles
        && measured.left.ri_phys_footprint != 0
        && memory.ri_lifetime_max_phys_footprint != 0
        && !cycle.failed
        && cycle.operations == expected
}

fn emit(
    measured: &Loop,
    memory: &libc::rusage_info_v4,
    cycles: usize,
    operations: u64,
) -> Result<()> {
    let Loop {
        fingerprint,
        scale,
        entered,
        left,
        cpu,
        start,
        stop,
    } = measured;
    let mut out = std::io::stdout().lock();
    writeln!(
        out,
        "{{\"pid\":{},\"operations\":{operations},\"cycles\":{cycles},\"fingerprint\":\"{fingerprint:016x}\",\"wall_seconds\":{:.9},\"cpu_seconds\":{cpu:.9},\"loop_instructions\":{},\"loop_cpu_cycles\":{},\"loop_runnable_seconds\":{:.9},\"loop_end_phys_footprint_bytes\":{},\"lifetime_max_phys_footprint_bytes\":{}}}",
        std::process::id(),
        (stop - start) as f64 / 1e9,
        left.ri_instructions - entered.ri_instructions,
        left.ri_cycles - entered.ri_cycles,
        left.ri_runnable_time.wrapping_sub(entered.ri_runnable_time) as f64 * scale,
        left.ri_phys_footprint,
        memory.ri_lifetime_max_phys_footprint
    )?;
    Ok(())
}

/// Open the subject and prove that the loaded image is the selected one.
fn open_subject(args: &[String]) -> Result<(abi::Library, abi::Core)> {
    require(args.len() == 5, "measurement: wrong loaded image or UUID")?;
    let library = abi::Library::open(&args[2])?;
    let core = abi::Core::load(&library, "pinyin")?;
    image(core.init, &args[2], &args[3])?;
    Ok((library, core))
}

/// macOS runs every `bisect` mode that the shared dispatch names.
pub(crate) fn refusal(_mode: Option<&str>) -> Option<&'static str> {
    None
}

pub(crate) fn native(args: &[String]) -> Result<()> {
    let (library, core) = open_subject(args)?;
    let cycles = super::strict_cycles()?;
    let typing = abi::Typing::load(&library, "pinyin")?;
    let guess = abi::Guess::load(&library, "pinyin")?;
    let user = abi::UserDir::temporary("bisect-measure-")?;
    let paths = abi::ContextPaths::new(&args[4], user.path())?;
    let ctx = abi::Context::open(&core, &paths)?;
    let instance = abi::Instance::open(&typing, ctx.handle)?;
    require(
        (typing.options)(ctx.handle, 0x18a),
        "measurement: set_options failed",
    )?;
    let mut cycle = workload::Cycle {
        typing: &typing,
        guess: &guess,
        instance: instance.handle,
        operations: 0,
        failed: false,
    };
    let expected = workload::operations() * cycles as u64;
    let measured = timed(&mut cycle, cycles)?;
    drop(instance);
    drop(ctx);
    let memory = rusage()?;
    drop(user);
    require(
        complete(&measured, &memory, &cycle, expected),
        "measurement: incomplete workload or broken required capture",
    )?;
    emit(&measured, &memory, cycles, cycle.operations)
}
