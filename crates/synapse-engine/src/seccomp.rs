#[cfg(target_os = "linux")]
use std::{
    ffi::{CStr, CString},
    fs::{File, OpenOptions},
    io, mem,
    os::{
        fd::{AsRawFd, RawFd},
        raw::{c_char, c_int, c_uint, c_void},
    },
    path::Path,
    sync::OnceLock,
};

#[cfg(target_os = "linux")]
const SCMP_ACT_ALLOW: u32 = 0x7fff_0000;
#[cfg(target_os = "linux")]
const SCMP_ACT_ERRNO_BASE: u32 = 0x0005_0000;
#[cfg(target_os = "linux")]
const SCMP_ERROR: c_int = -1;
#[cfg(target_os = "linux")]
const BLOCKED_SYSCALLS: &[&str] = &[
    "fork",
    "vfork",
    "clone",
    "clone3",
    "unshare",
    "setns",
    "mount",
    "umount2",
    "ptrace",
    "socket",
    "socketpair",
    "connect",
    "accept",
    "accept4",
    "bind",
    "listen",
    "bpf",
    "finit_module",
    "init_module",
    "delete_module",
    "kexec_load",
    "name_to_handle_at",
    "open_by_handle_at",
    "process_vm_readv",
    "process_vm_writev",
    "userfaultfd",
];

#[cfg(target_os = "linux")]
type SeccompInit = unsafe extern "C" fn(c_uint) -> *mut c_void;
#[cfg(target_os = "linux")]
type SeccompRuleAdd = unsafe extern "C" fn(*mut c_void, c_uint, c_int, c_uint, ...) -> c_int;
#[cfg(target_os = "linux")]
type SeccompLoad = unsafe extern "C" fn(*mut c_void) -> c_int;
#[cfg(target_os = "linux")]
type SeccompRelease = unsafe extern "C" fn(*mut c_void);
#[cfg(target_os = "linux")]
type SeccompExportBpf = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
#[cfg(target_os = "linux")]
type SeccompSyscallResolveName = unsafe extern "C" fn(*const c_char) -> c_int;

#[cfg(target_os = "linux")]
pub struct ExportedSeccompFilter {
    file: File,
}

#[cfg(target_os = "linux")]
impl ExportedSeccompFilter {
    pub fn fd(&self) -> RawFd {
        self.file.as_raw_fd()
    }
}

#[cfg(target_os = "linux")]
pub fn load_blacklist_profile() -> io::Result<()> {
    let ctx = FilterContext::new_blacklist()?;
    map_seccomp_result(unsafe { (ctx.api.seccomp_load)(ctx.raw) })
}

#[cfg(target_os = "linux")]
pub fn export_blacklist_bpf(path: &Path) -> io::Result<ExportedSeccompFilter> {
    let ctx = FilterContext::new_blacklist()?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(path)?;

    let fd = file.as_raw_fd();
    map_seccomp_result(unsafe { (ctx.api.seccomp_export_bpf)(ctx.raw, fd) })?;
    reset_fd_position(fd)?;
    set_inheritable(fd)?;
    file.sync_all()?;

    Ok(ExportedSeccompFilter { file })
}

#[cfg(target_os = "linux")]
fn seccomp_errno_action(errno: u16) -> u32 {
    SCMP_ACT_ERRNO_BASE | u32::from(errno)
}

#[cfg(target_os = "linux")]
fn map_seccomp_result(result: c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(-result))
    }
}

#[cfg(target_os = "linux")]
fn reset_fd_position(fd: RawFd) -> io::Result<()> {
    let result = unsafe { libc::lseek(fd, 0, libc::SEEK_SET) };
    if result >= 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
fn set_inheritable(fd: RawFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }

    let result = unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "linux")]
struct FilterContext {
    api: &'static SeccompApi,
    raw: *mut c_void,
}

#[cfg(target_os = "linux")]
impl FilterContext {
    fn new_blacklist() -> io::Result<Self> {
        let api = seccomp_api()?;
        let raw = unsafe { (api.seccomp_init)(SCMP_ACT_ALLOW) };
        if raw.is_null() {
            return Err(io::Error::other("seccomp_init returned a null context"));
        }

        let context = Self { api, raw };
        for syscall in BLOCKED_SYSCALLS {
            let name = CString::new(*syscall).expect("syscall names are valid C strings");
            let number = unsafe { (api.seccomp_syscall_resolve_name)(name.as_ptr()) };
            if number == SCMP_ERROR {
                continue;
            }

            map_seccomp_result(unsafe {
                (api.seccomp_rule_add)(
                    context.raw,
                    seccomp_errno_action(libc::EPERM as u16),
                    number,
                    0,
                )
            })?;
        }

        Ok(context)
    }
}

#[cfg(target_os = "linux")]
impl Drop for FilterContext {
    fn drop(&mut self) {
        unsafe {
            (self.api.seccomp_release)(self.raw);
        }
    }
}

#[cfg(target_os = "linux")]
struct SeccompApi {
    _handle: *mut c_void,
    seccomp_init: SeccompInit,
    seccomp_rule_add: SeccompRuleAdd,
    seccomp_load: SeccompLoad,
    seccomp_release: SeccompRelease,
    seccomp_export_bpf: SeccompExportBpf,
    seccomp_syscall_resolve_name: SeccompSyscallResolveName,
}

#[cfg(target_os = "linux")]
unsafe impl Send for SeccompApi {}
#[cfg(target_os = "linux")]
unsafe impl Sync for SeccompApi {}

#[cfg(target_os = "linux")]
fn seccomp_api() -> io::Result<&'static SeccompApi> {
    static API: OnceLock<Result<SeccompApi, String>> = OnceLock::new();
    match API.get_or_init(|| SeccompApi::load().map_err(|error| error.to_string())) {
        Ok(api) => Ok(api),
        Err(message) => Err(io::Error::other(message.clone())),
    }
}

#[cfg(target_os = "linux")]
impl SeccompApi {
    fn load() -> io::Result<Self> {
        let mut last_error = None;
        for library in [c"libseccomp.so.2", c"libseccomp.so"] {
            let handle =
                unsafe { libc::dlopen(library.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
            if handle.is_null() {
                last_error = Some(dlerror_message());
                continue;
            }

            return Ok(Self {
                _handle: handle,
                seccomp_init: unsafe { load_symbol(handle, c"seccomp_init")? },
                seccomp_rule_add: unsafe { load_symbol(handle, c"seccomp_rule_add")? },
                seccomp_load: unsafe { load_symbol(handle, c"seccomp_load")? },
                seccomp_release: unsafe { load_symbol(handle, c"seccomp_release")? },
                seccomp_export_bpf: unsafe { load_symbol(handle, c"seccomp_export_bpf")? },
                seccomp_syscall_resolve_name: unsafe {
                    load_symbol(handle, c"seccomp_syscall_resolve_name")?
                },
            });
        }

        Err(io::Error::other(format!(
            "failed to load libseccomp: {}",
            last_error.unwrap_or_else(|| "library not found".to_string())
        )))
    }
}

#[cfg(target_os = "linux")]
unsafe fn load_symbol<T>(handle: *mut c_void, symbol: &CStr) -> io::Result<T>
where
    T: Copy,
{
    let raw = unsafe { libc::dlsym(handle, symbol.as_ptr()) };
    if raw.is_null() {
        Err(io::Error::other(format!(
            "missing libseccomp symbol {}: {}",
            symbol.to_string_lossy(),
            dlerror_message()
        )))
    } else {
        Ok(unsafe { mem::transmute_copy(&raw) })
    }
}

#[cfg(target_os = "linux")]
fn dlerror_message() -> String {
    let message = unsafe { libc::dlerror() };
    if message.is_null() {
        "unknown dlopen error".to_string()
    } else {
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    }
}
