use crate::{Result, abi, output, require, workload};
use std::ffi::{CStr, c_void};
use std::io::Write;
use std::path::Path;
use std::ptr::{null, null_mut};
use windows_sys::Win32::{
    Foundation::{CloseHandle, FILETIME, HMODULE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING,
    },
    System::{
        Diagnostics::Debug::IMAGE_NT_HEADERS64,
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            GetModuleFileNameW, GetModuleHandleA, GetModuleHandleExW, GetModuleHandleW,
            GetProcAddress,
        },
        Performance::{QueryPerformanceCounter, QueryPerformanceFrequency},
        ProcessStatus::{EnumProcessModules, GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        SystemInformation::GetSystemDirectoryW,
        SystemServices::{IMAGE_DOS_HEADER, IMAGE_IMPORT_BY_NAME, IMAGE_IMPORT_DESCRIPTOR},
        Threading::{ExitProcess, GetCurrentProcess, GetProcessTimes},
        WindowsProgramming::{IMAGE_THUNK_DATA64, QueryProcessCycleTime},
    },
};

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
pub(crate) fn load(path: &str) -> Result<libloading::Library> {
    // SAFETY: The selected subject implements the C ABI used by this process.
    Ok(unsafe { libloading::Library::new(path)? })
}
fn module(address: *const c_void) -> Result<HMODULE> {
    let mut handle = null_mut();
    // SAFETY: address is a symbol in the live subject; unchanged-refcount avoids
    // acquiring a second loader ownership reference.
    let found = unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            address.cast(),
            &mut handle,
        )
    };
    require(found != 0, "measurement: wrong loaded image")?;
    Ok(handle)
}
fn module_path(handle: HMODULE) -> Result<String> {
    let mut path = [0u16; 32768];
    // SAFETY: path is a writable UTF-16 buffer with the supplied capacity.
    let length =
        unsafe { GetModuleFileNameW(handle, path.as_mut_ptr(), path.len() as u32) } as usize;
    require(
        length != 0 && length < path.len(),
        "measurement: module path failed",
    )?;
    Ok(String::from_utf16(&path[..length])?)
}
fn file_id(path: &str) -> Result<(u32, u32, u32)> {
    let path = wide(path);
    // SAFETY: path is live and NUL-terminated; optional pointers are null.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    require(
        handle != INVALID_HANDLE_VALUE,
        "measurement: wrong loaded image",
    )?;
    // SAFETY: BY_HANDLE_FILE_INFORMATION contains integer fields that admit zero.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: handle is open and info is writable storage for the identity output.
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) };
    // SAFETY: this is the owned CreateFileW handle, not yet closed.
    unsafe { CloseHandle(handle) };
    require(ok != 0, "measurement: file identity failed")?;
    Ok((
        info.dwVolumeSerialNumber,
        info.nFileIndexHigh,
        info.nFileIndexLow,
    ))
}

pub(crate) fn release(library: &libloading::Library) -> Result<extern "C" fn(*mut c_void)> {
    // SAFETY: init is the documented function exported by the live subject.
    let init = unsafe {
        *library.get::<extern "C" fn(*const i8, *const i8) -> abi::Handle>(b"pinyin_init")?
    };
    let product = module(init as *const c_void)?;
    product_release(product)
}
fn product_release(product: HMODULE) -> Result<extern "C" fn(*mut c_void)> {
    let base = product.cast::<u8>();
    // SAFETY: the live PE image starts at its aligned, loader-mapped DOS header.
    let dos = unsafe { &*base.cast::<IMAGE_DOS_HEADER>() };
    // SAFETY: e_lfanew locates the NT header within this loader-mapped image.
    let nt = unsafe { base.add(dos.e_lfanew as usize) }.cast::<IMAGE_NT_HEADERS64>();
    // SAFETY: the mapped NT header has the aligned 64-bit layout of this image.
    let nt = unsafe { &*nt };
    let rva = nt.OptionalHeader.DataDirectory[1].VirtualAddress;
    require(rva != 0, "measurement: product GLib ownership failed")?;
    // SAFETY: this nonzero import directory RVA addresses the mapped descriptor array.
    let mut descriptor = unsafe { base.add(rva as usize) }.cast::<IMAGE_IMPORT_DESCRIPTOR>();
    // SAFETY: descriptor is an aligned element of the loader's terminated import array.
    while unsafe { (*descriptor).Name != 0 } {
        // SAFETY: descriptor still addresses the same mapped import record.
        let name_rva = unsafe { (*descriptor).Name };
        // SAFETY: the nonzero Name RVA points inside the mapped image.
        let name = unsafe { base.add(name_rva as usize) };
        // SAFETY: the mapped import name is a NUL-terminated byte string.
        let bytes = unsafe { CStr::from_ptr(name.cast()) }.to_bytes();
        if bytes.len() >= 8 && bytes[..8].eq_ignore_ascii_case(b"glib-2.0") {
            // SAFETY: name is the live, NUL-terminated import name.
            let glib = unsafe { GetModuleHandleA(name) };
            // SAFETY: the subject keeps its imported GLib module loaded, and
            // g_free is a NUL-terminated symbol name.
            let release = unsafe { GetProcAddress(glib, c"g_free".as_ptr().cast()) }
                .ok_or("measurement: product GLib ownership failed")?;
            // SAFETY: descriptor addresses the aligned, mapped import record.
            let thunk = unsafe { (*descriptor).Anonymous };
            // SAFETY: this import descriptor stores the original lookup RVA here.
            let original = unsafe { thunk.OriginalFirstThunk };
            require(original != 0, "measurement: product GLib ownership failed")?;
            // SAFETY: original addresses the mapped import lookup array.
            let mut names = unsafe { base.add(original as usize) }.cast::<IMAGE_THUNK_DATA64>();
            // SAFETY: descriptor is still a live, mapped import record.
            let first = unsafe { (*descriptor).FirstThunk };
            // SAFETY: FirstThunk addresses the corresponding mapped import address array.
            let mut bound = unsafe { base.add(first as usize) }.cast::<IMAGE_THUNK_DATA64>();
            while {
                // SAFETY: names points to an aligned slot of the terminated lookup array.
                let lookup = unsafe { (*names).u1 };
                // SAFETY: the slot holds a lookup encoding, with zero as the terminator.
                unsafe { lookup.AddressOfData != 0 }
            } {
                // SAFETY: names still points to the same mapped lookup slot.
                let lookup = unsafe { (*names).u1 };
                // SAFETY: Ordinal views the same u64 as the ordinal-flag encoding.
                if unsafe { lookup.Ordinal } & (1u64 << 63) == 0 {
                    // SAFETY: names still points to the mapped lookup slot.
                    let lookup = unsafe { (*names).u1 };
                    // SAFETY: a nonzero, non-ordinal slot contains a name RVA.
                    let entry_rva = unsafe { lookup.AddressOfData };
                    // SAFETY: the name RVA points to a mapped import-by-name record.
                    let entry =
                        unsafe { base.add(entry_rva as usize) }.cast::<IMAGE_IMPORT_BY_NAME>();
                    // SAFETY: entry points to the aligned, mapped import-by-name record.
                    let entry = unsafe { &*entry };
                    // SAFETY: the record's trailing Name is NUL-terminated in the mapped image.
                    if unsafe { CStr::from_ptr(entry.Name.as_ptr().cast()) }.to_bytes() == b"g_free"
                    {
                        // SAFETY: bound is the aligned address slot corresponding to this lookup entry.
                        let slot = unsafe { (*bound).u1 };
                        // SAFETY: the loader filled this address-table slot with a function address.
                        let address = unsafe { slot.Function };
                        require(
                            release as usize == address as usize,
                            "measurement: product GLib ownership failed",
                        )?;
                        // SAFETY: the verified loader-bound export is GLib's g_free with this ABI.
                        let release = unsafe {
                            std::mem::transmute::<
                                unsafe extern "system" fn() -> isize,
                                extern "C" fn(*mut c_void),
                            >(release)
                        };
                        return Ok(release);
                    }
                }
                // SAFETY: a nonterminal lookup entry is followed by another entry or terminator.
                names = unsafe { names.add(1) };
                // SAFETY: the import address array advances in lockstep with the lookup array.
                bound = unsafe { bound.add(1) };
            }
            return Err("measurement: product GLib ownership failed".into());
        }
        // SAFETY: a nonterminal import descriptor is followed by another descriptor or terminator.
        descriptor = unsafe { descriptor.add(1) };
    }
    Err("measurement: product GLib ownership failed".into())
}

fn modules(product: HMODULE) -> Result<Vec<String>> {
    let mut handles = [null_mut(); 128];
    let mut needed = 0;
    let mut system = [0u16; 260];
    // SAFETY: GetCurrentProcess returns a pseudo-handle and has no caller preconditions.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: handles has the supplied byte capacity and needed is writable.
    let status = unsafe {
        EnumProcessModules(
            process,
            handles.as_mut_ptr(),
            std::mem::size_of_val(&handles) as u32,
            &mut needed,
        )
    };
    require(
        status != 0 && needed as usize <= std::mem::size_of_val(&handles),
        "measurement: module capture failed",
    )?;
    // SAFETY: system is writable UTF-16 storage of the supplied capacity.
    let length = unsafe { GetSystemDirectoryW(system.as_mut_ptr(), system.len() as u32) } as usize;
    require(
        length != 0 && length < system.len(),
        "measurement: system directory failed",
    )?;
    let system = String::from_utf16(&system[..length])?.to_lowercase();
    // SAFETY: a null name requests the current executable, with no ownership transfer.
    let executable = unsafe { GetModuleHandleW(null()) };
    let mut paths = Vec::new();
    for handle in &handles[..needed as usize / std::mem::size_of::<HMODULE>()] {
        if *handle == product || *handle == executable {
            continue;
        }
        let path = module_path(*handle)?;
        let lower = path.to_lowercase();
        let name = lower.rsplit('\\').next().unwrap_or("");
        let crt = ["ucrtbase", "vcruntime", "msvcp1"]
            .iter()
            .any(|prefix| name.starts_with(prefix));
        if crt
            || !lower
                .strip_prefix(&system)
                .is_some_and(|rest| rest.starts_with('\\'))
        {
            paths.push(path);
        }
    }
    require(!paths.is_empty(), "measurement: module capture failed")?;
    Ok(paths)
}

fn dependencies(snapshot: &[String], expected: &str) -> Result<()> {
    let mut count = 0;
    for line in expected.lines() {
        require(
            snapshot.iter().any(|path| path.eq_ignore_ascii_case(line)),
            "measurement: dependency identity mismatch",
        )?;
        count += 1;
    }
    require(
        count == snapshot.len(),
        "measurement: dependency identity mismatch",
    )
}
fn modules_json(out: &mut impl Write, snapshot: &[String]) -> Result<()> {
    out.write_all(b"[")?;
    for (i, path) in snapshot.iter().enumerate() {
        if i != 0 {
            out.write_all(b",")?;
        }
        output::string(out, path)?;
    }
    out.write_all(b"]")?;
    Ok(())
}
pub(crate) fn now_ns() -> u64 {
    let mut ticks = 0;
    let mut frequency = 0;
    // SAFETY: frequency is a writable i64 matching the LARGE_INTEGER output.
    if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
        return 0;
    }
    // SAFETY: ticks is a writable i64 matching the LARGE_INTEGER output.
    if unsafe { QueryPerformanceCounter(&mut ticks) } == 0 {
        return 0;
    }
    ((ticks as u128 * 1_000_000_000) / frequency as u128) as u64
}
pub(crate) fn cpu_ns() -> u64 {
    cpu_ticks().unwrap_or(0) * 100
}
fn cpu_ticks() -> Result<u64> {
    // SAFETY: FILETIME contains only integer fields, which admit zero.
    let mut creation: FILETIME = unsafe { std::mem::zeroed() };
    let mut exit = creation;
    let mut kernel = creation;
    let mut user = creation;
    // SAFETY: GetCurrentProcess returns a pseudo-handle and has no caller preconditions.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: process denotes the current process and all four FILETIME outputs are writable.
    let status =
        unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) };
    require(status != 0, "measurement: process time failed")?;
    let ticks = |time: FILETIME| ((time.dwHighDateTime as u64) << 32) | time.dwLowDateTime as u64;
    Ok(ticks(kernel) + ticks(user))
}
fn process_cycles() -> Result<u64> {
    let mut cycles = 0;
    // SAFETY: GetCurrentProcess returns a pseudo-handle and has no caller preconditions.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: process denotes the current process and cycles is writable.
    let status = unsafe { QueryProcessCycleTime(process, &mut cycles) };
    require(status != 0, "measurement: process cycles failed")?;
    Ok(cycles)
}
pub(crate) fn fresh_user(path: &Path) -> Result<()> {
    match std::fs::create_dir(path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    require(
        std::fs::read_dir(path)?.next().is_none()
            && !std::fs::metadata(path)?.permissions().readonly(),
        "user directory must be fresh and empty",
    )
}
pub(crate) fn immediate_exit() -> ! {
    // SAFETY: Memory stops intentionally terminate without destructors.
    unsafe { ExitProcess(0) }
}

/// The loaded subject, with the module identity proved against the selected path.
struct Subject {
    core: abi::Core,
    product: HMODULE,
    image: String,
    library: abi::Library,
}

fn open_subject(args: &[String]) -> Result<Subject> {
    require(args.len() == 5, "measurement: wrong loaded image")?;
    let library = abi::Library::open(&args[2]).map_err(|_| "measurement: wrong loaded image")?;
    let core = abi::Core::load(&library, "pinyin")?;
    let product = module(core.init as *const c_void)?;
    let image = module_path(product)?;
    require(
        file_id(&image)? == file_id(&args[2])?,
        "measurement: wrong loaded image",
    )?;
    product_release(product)?;
    Ok(Subject {
        core,
        product,
        image,
        library,
    })
}

fn preflight(image: &str, modules: &[String]) -> Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(b"{\"library\":")?;
    output::string(&mut out, image)?;
    out.write_all(b",\"modules\":")?;
    modules_json(&mut out, modules)?;
    writeln!(out, "}}")?;
    Ok(())
}

fn manifest() -> Result<String> {
    Ok(std::fs::read_to_string(
        std::env::var_os("MEASURE_MODULES").ok_or("measurement: missing module manifest")?,
    )?)
}

/// The counters read around the typing loop.
struct Loop {
    fingerprint: u64,
    cycles_before: u64,
    cycles_end: u64,
    cpu_before: u64,
    cpu_end: u64,
    start: u64,
    stop: u64,
}

fn timed(cycle: &mut workload::Cycle<'_>, cycles: usize) -> Result<Loop> {
    let mut fingerprint = workload::FNV_OFFSET;
    let cycles_before = process_cycles()?;
    let cpu_before = cpu_ticks()?;
    let start = now_ns();
    for _ in 0..cycles {
        fingerprint ^= cycle.run();
        fingerprint = fingerprint.wrapping_mul(workload::FNV_PRIME);
    }
    let stop = now_ns();
    let cpu_end = cpu_ticks()?;
    let cycles_end = process_cycles()?;
    Ok(Loop {
        fingerprint,
        cycles_before,
        cycles_end,
        cpu_before,
        cpu_end,
        start,
        stop,
    })
}

fn memory_counters() -> Result<PROCESS_MEMORY_COUNTERS> {
    // SAFETY: PROCESS_MEMORY_COUNTERS contains only integer fields, which admit zero.
    let mut value: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
    value.cb = std::mem::size_of_val(&value) as u32;
    // SAFETY: GetCurrentProcess returns a pseudo-handle and has no caller preconditions.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: value is writable, with cb and the buffer size set for this structure.
    let status = unsafe {
        GetProcessMemoryInfo(
            process,
            &mut value,
            std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    require(status != 0, "measurement: memory capture failed")?;
    Ok(value)
}

fn complete(
    measured: &Loop,
    memory: &PROCESS_MEMORY_COUNTERS,
    cycle: &workload::Cycle<'_>,
    expected: u64,
) -> bool {
    measured.stop > measured.start
        && measured.cpu_end >= measured.cpu_before
        && measured.cycles_end > measured.cycles_before
        && memory.PeakPagefileUsage != 0
        && memory.PeakWorkingSetSize != 0
        && !cycle.failed
        && cycle.operations == expected
}

fn emit(
    measured: &Loop,
    memory: &PROCESS_MEMORY_COUNTERS,
    cycles: usize,
    operations: u64,
    image: &str,
    after: &[String],
) -> Result<()> {
    let mut out = std::io::stdout().lock();
    write!(
        out,
        "{{\"pid\":{},\"operations\":{operations},\"cycles\":{cycles},\"fingerprint\":\"{:016x}\",\"wall_seconds\":{:.9},\"cpu_seconds\":{:.9},\"loop_cpu_cycles\":{},\"peak_private_commit_bytes\":{},\"peak_working_set_bytes\":{},\"library\":",
        std::process::id(),
        measured.fingerprint,
        (measured.stop - measured.start) as f64 / 1e9,
        (measured.cpu_end - measured.cpu_before) as f64 / 1e7,
        measured.cycles_end - measured.cycles_before,
        memory.PeakPagefileUsage,
        memory.PeakWorkingSetSize
    )?;
    output::string(&mut out, image)?;
    out.write_all(b",\"modules\":")?;
    modules_json(&mut out, after)?;
    writeln!(out, "}}")?;
    Ok(())
}

fn tables(library: &abi::Library) -> Result<(abi::Typing, abi::Guess)> {
    Ok((
        abi::Typing::load(library, "pinyin")?,
        abi::Guess::load(library, "pinyin")?,
    ))
}

fn measure(subject: &Subject, data: &str, manifest: &str) -> Result<()> {
    let cycles = super::strict_cycles()?;
    let (typing, guess) = tables(&subject.library)?;
    let user = abi::UserDir::temporary("bisect-measure-")?;
    let paths = abi::ContextPaths::new(data, user.path())?;
    let ctx = abi::Context::open(&subject.core, &paths)?;
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
    let memory = memory_counters()?;
    let after = modules(subject.product)?;
    dependencies(&after, manifest).map_err(
        |_| "measurement: incomplete workload or broken required capture/dependency identity",
    )?;
    drop(user);
    require(
        complete(&measured, &memory, &cycle, expected),
        "measurement: incomplete workload or broken required capture/dependency identity",
    )?;
    emit(
        &measured,
        &memory,
        cycles,
        cycle.operations,
        &subject.image,
        &after,
    )
}

pub(crate) fn native(args: &[String]) -> Result<()> {
    let subject = open_subject(args)?;
    let before = modules(subject.product)?;
    if std::env::var_os("MEASURE_PREFLIGHT").is_some() {
        return preflight(&subject.image, &before);
    }
    let manifest = manifest()?;
    dependencies(&before, &manifest)
        .map_err(|_| "measurement: dependency identity mismatch before loop")?;
    measure(&subject, &args[4], &manifest)
}

/// Windows runs `--measure` and the two sentence observations, whose results hold there. `--perf`
/// would report per-cycle CPU time quantized to the 1/64 s process tick and no memory readings,
/// so it is refused with the usage status, as the C driver refused every mode but `--measure`.
/// Unknown modes and a missing mode are refused the same way.
pub(crate) fn refusal(mode: Option<&str>) -> Option<&'static str> {
    match mode {
        Some("--measure" | "--perf-sentence" | "--perf-typing-sequence") => None,
        _ => Some("Windows supports --measure, --perf-sentence and --perf-typing-sequence only"),
    }
}

pub(crate) fn temp_root() -> std::path::PathBuf {
    std::env::temp_dir()
}
