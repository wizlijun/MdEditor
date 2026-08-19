//! Platform port layer — the single funnel for spawning child processes.
//!
//! Two things must be true on Windows that are free on unix, and both are easy
//! to forget at each individual call site (which is why they live here):
//!
//! 1. **No console flash.** A GUI process spawning a console subprocess (`git`)
//!    pops a black `conhost` window for the life of the child. `git` runs on
//!    every vault-sync tick, so without `CREATE_NO_WINDOW` the app strobes.
//! 2. **A usable child environment.** `env_clear()` on Windows is far more
//!    destructive than on unix: without `SystemRoot` the loader cannot
//!    initialise winsock/crypto and most binaries die before `main`.
//!
//! Migration discipline (docs/2026-08-08-pc-port-refactor-plan.md §1.1): new
//! code MUST NOT call `std::process::Command::new` / `tokio::process::Command::new`
//! directly outside this module.

use std::ffi::OsStr;

/// `CREATE_NO_WINDOW` (winbase.h). Suppresses the console window that a GUI
/// parent would otherwise pop for a console child.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// `std::process::Command`, windowless on Windows.
pub fn command(program: impl AsRef<OsStr>) -> std::process::Command {
    let cmd = std::process::Command::new(program);
    #[cfg(windows)]
    let mut cmd = cmd;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `tokio::process::Command`, windowless on Windows.
#[cfg(not(target_os = "ios"))]
pub fn tokio_command(program: impl AsRef<OsStr>) -> tokio::process::Command {
    let cmd = tokio::process::Command::new(program);
    #[cfg(windows)]
    let mut cmd = cmd;
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Environment variables a plugin subprocess may inherit after `env_clear()`.
///
/// The unix set is the original allowlist from `plugin_runtime::process`: enough
/// for a plugin to find `$HOME`/`$PATH` (openclaw resolves its UDS socket path
/// from them) and nothing that carries a secret.
///
/// The Windows set is the equivalent, and it is not optional the way the unix
/// one is: `SystemRoot` in particular is required by the loader itself — clear
/// it and a child dies before reaching `main` with an opaque failure. `PATHEXT`
/// and `COMSPEC` are needed for command resolution; `APPDATA`/`LOCALAPPDATA`/
/// `USERPROFILE` are the Windows analogue of `$HOME`; `TEMP`/`TMP` of `TMPDIR`.
pub fn plugin_env_allowlist() -> &'static [&'static str] {
    #[cfg(windows)]
    {
        &[
            "SystemRoot", "windir", "SystemDrive", "COMSPEC", "PATHEXT", "PATH",
            "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "APPDATA", "LOCALAPPDATA",
            "PROGRAMDATA", "TEMP", "TMP", "NUMBER_OF_PROCESSORS",
            "PROCESSOR_ARCHITECTURE", "LANG", "LC_ALL", "USERNAME",
        ]
    }
    #[cfg(not(windows))]
    {
        &["HOME", "PATH", "LANG", "LC_ALL", "TERM", "USER", "TMPDIR"]
    }
}

/// Point `link` at the directory `target`.
///
/// unix: an ordinary symlink (the caller decides relative vs absolute).
///
/// Windows: `symlink_dir` first — that is the pre-existing behaviour and what
/// you get with Developer Mode on — and on failure a **directory junction**,
/// which needs no privilege whatsoever. Without the fallback, installing any
/// plugin failed with `os error 1314` (ERROR_PRIVILEGE_NOT_HELD) on a stock
/// Windows account, because the plugin tree's `current` pointer is a directory
/// link.
///
/// A junction is indistinguishable from a symlink to the callers here:
/// `read_link` returns the target, `symlink_metadata().file_type().is_symlink()`
/// is true, and traversal works. The one difference is that a junction must
/// name an absolute local path, which is what the Windows branch already passed.
#[cfg(windows)]
pub fn link_dir(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    let first = match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    // `mklink` is a cmd.exe builtin, so it has to go through the shell. Args are
    // passed as separate values, letting std quote paths containing spaces
    // (`C:\Users\Some Name\...` is entirely ordinary).
    let status = command("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;
    if status.success() && link.exists() {
        Ok(())
    } else {
        Err(first)
    }
}

/// unix counterpart of the Windows [`link_dir`]: a plain symlink.
#[cfg(unix)]
pub fn link_dir(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// Create a symlink in a test, of whichever kind the platform needs.
///
/// Windows distinguishes file from directory links and requires Developer Mode
/// or elevation, so an `Err` here means "this machine cannot make symlinks" —
/// callers must skip rather than fail (docs/2026-08-08-pc-port-refactor-plan.md
/// §9.1). Before this existed, the affected tests called `std::os::unix`
/// directly and the whole lib-test crate failed to compile on Windows.
#[cfg(test)]
pub fn test_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link)
        } else {
            std::os::windows::fs::symlink_file(target, link)
        }
    }
}

/// 外壳(`notemd mcp`)与 GUI 主程序之间的那一跳。
///
/// **不开 TCP 端口**:UDS / Named Pipe 都不在网络栈上,于是端口占用、
/// DNS rebinding、CSRF、Origin 校验这一整类问题连同 token 一起消失,
/// 访问控制交给 OS(unix 文件权限 / Windows 管道 ACL)。
///
/// 不用「AF_UNIX 一把梭」:Windows 10 1803+ 虽支持 AF_UNIX,但 tokio 在
/// Windows 上不支持它(`UnixStream` 由 `cfg(unix)` 门死),得另引
/// `uds_windows` 再自建异步桥 —— 所谓统一只是换个地方分叉,还多背一个依赖。
pub mod ipc {
    use std::io;
    use std::path::PathBuf;

    #[cfg(unix)]
    pub type Stream = tokio::net::UnixStream;
    #[cfg(windows)]
    pub type Stream = tokio::net::windows::named_pipe::NamedPipeServer;

    /// unix:socket 文件路径。Linux 用 `$XDG_RUNTIME_DIR`(runtime socket
    /// 不属于 config 目录),macOS 无此变量,回落 App Support。
    #[cfg(unix)]
    pub fn endpoint() -> io::Result<PathBuf> {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .map(|d| d.join("notemd"))
            .or_else(|| dirs::config_dir().map(|d| d.join(crate::app_dirs::BUNDLE_ID)))
            .ok_or_else(|| io::Error::other("no runtime or config dir"))?;
        std::fs::create_dir_all(&base)?;
        Ok(base.join("mcp.sock"))
    }

    #[cfg(windows)]
    pub fn endpoint() -> io::Result<PathBuf> {
        Ok(PathBuf::from(r"\\.\pipe\net.notemd.app.mcp"))
    }

    #[cfg(unix)]
    pub struct Listener(tokio::net::UnixListener);

    #[cfg(unix)]
    impl Listener {
        pub async fn accept(&mut self) -> io::Result<Stream> {
            let (s, _) = self.0.accept().await?;
            Ok(s)
        }
    }

    /// unix 的僵尸 socket:主程序崩溃后 `.sock` 残留,再 `bind()` 得
    /// `EADDRINUSE`。**先 connect 探活,被拒才 unlink** —— 无脑删会踢掉一个
    /// 正在健康运行的实例(spec §3.4)。
    #[cfg(unix)]
    pub async fn listen() -> io::Result<Listener> {
        use std::os::unix::fs::PermissionsExt;
        let path = endpoint()?;
        if path.exists() {
            match tokio::net::UnixStream::connect(&path).await {
                Ok(_) => return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another note.md instance is already serving MCP",
                )),
                Err(_) => { let _ = std::fs::remove_file(&path); }
            }
        }
        let l = tokio::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener(l))
    }

    #[cfg(unix)]
    pub async fn connect() -> io::Result<tokio::net::UnixStream> {
        tokio::net::UnixStream::connect(endpoint()?).await
    }

    /// 建一个只有当前用户能碰的管道实例。
    ///
    /// `ServerOptions::create` 传的是 NULL `lpSecurityAttributes`,而
    /// `CreateNamedPipe` 文档写明:NULL 时的默认安全描述符对 Everyone 组和
    /// 匿名账户授予**读权限** —— 同机的另一个本地账户就能读到 MCP 流量(vault
    /// 搜索结果)。这与 unix 侧 `0600` 想达到的效果不对等,也是 spec 明写的
    /// 「Windows 建管道时挂 SECURITY_DESCRIPTOR 限本用户」。
    ///
    /// SDDL `"D:P(A;;GA;;;OW)"`:受保护的 DACL(`P`,不继承)、只有一条 ACE
    /// 把 Generic-All 授给 owner(`OW`),别的主体不出现 —— 是
    /// `ConvertStringSecurityDescriptorToSecurityDescriptorW` 里最不容易出错的
    /// 构造方式,免得手搭 ACL/SID 的字节布局。
    #[cfg(windows)]
    fn create_owner_only_pipe(
        name: &str,
        first_instance: bool,
    ) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStrExt;
        use tokio::net::windows::named_pipe::ServerOptions;
        use windows_sys::Win32::Foundation::LocalFree;
        use windows_sys::Win32::Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        };
        use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};

        const SDDL: &str = "D:P(A;;GA;;;OW)";
        let wide: Vec<u16> = std::ffi::OsStr::new(SDDL)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // Safety: `wide` is a live null-terminated UTF-16 buffer for the
        // duration of this call; `sd` is a valid out-pointer the API fills in
        // with a heap allocation (owned by us afterwards, freed below).
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide.as_ptr(),
                SDDL_REVISION_1,
                &mut sd,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }

        let mut attrs = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };

        // Safety: `attrs` is a live, correctly-sized SECURITY_ATTRIBUTES for
        // the duration of this synchronous call; `create_with_security_attributes_raw`
        // only reads it while `CreateNamedPipe` runs.
        let result = unsafe {
            ServerOptions::new()
                .first_pipe_instance(first_instance)
                .create_with_security_attributes_raw(name, &mut attrs as *mut _ as *mut c_void)
        };

        // Safety: `sd` was allocated by the Convert…W call above and is used
        // nowhere after this point in either branch.
        unsafe { LocalFree(sd as _) };

        result
    }

    #[cfg(windows)]
    pub struct Listener {
        name: String,
        next: Option<tokio::net::windows::named_pipe::NamedPipeServer>,
    }

    #[cfg(windows)]
    impl Listener {
        pub async fn accept(&mut self) -> io::Result<Stream> {
            let server = self.next.take().ok_or_else(|| io::Error::other("listener closed"))?;
            server.connect().await?;
            // 下一个实例必须在把当前这个交出去之前建好,否则客户端会在
            // 两次 accept 之间撞上 ERROR_FILE_NOT_FOUND。
            self.next = Some(create_owner_only_pipe(&self.name, false)?);
            Ok(server)
        }
    }

    #[cfg(windows)]
    pub async fn listen() -> io::Result<Listener> {
        let name = endpoint()?.to_string_lossy().to_string();
        let first = create_owner_only_pipe(&name, true)?;
        Ok(Listener { name, next: Some(first) })
    }

    #[cfg(windows)]
    pub async fn connect() -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
        use tokio::net::windows::named_pipe::ClientOptions;
        let name = endpoint()?.to_string_lossy().to_string();
        ClientOptions::new().open(&name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every IPC test below binds the same fixed endpoint (the unix socket
    /// path / Windows pipe name is a single well-known constant, not
    /// per-test), so running them concurrently makes them fight each other —
    /// one test's `remove_file`/rebind lands mid-flight of another. This
    /// guard makes that safe under a plain `cargo test`, not just under
    /// `--test-threads=1`: correctness must not depend on a CLI flag a CI
    /// script, an IDE's "run all", or a dev typing the obvious command could
    /// omit.
    ///
    /// Poisoning is tolerated (`unwrap_or_else(PoisonError::into_inner)`):
    /// one panicking IPC test must not cascade into every other IPC test
    /// failing with a poisoned-lock error instead of its own assertion.
    static IPC_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn ipc_test_guard() -> std::sync::MutexGuard<'static, ()> {
        IPC_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn allowlist_carries_no_secrets() {
        // A regression guard on the intent of the list, not its exact contents:
        // nothing key/token/secret-shaped may be inherited by a plugin.
        for k in plugin_env_allowlist() {
            let lower = k.to_ascii_lowercase();
            assert!(
                !lower.contains("key")
                    && !lower.contains("token")
                    && !lower.contains("secret")
                    && !lower.contains("password"),
                "secret-shaped var in plugin env allowlist: {k}"
            );
        }
    }

    /// `SystemRoot` is load-bearing on Windows — a child without it fails to
    /// start at all, so this is not a "nice to have" entry.
    #[cfg(windows)]
    #[test]
    fn windows_allowlist_has_systemroot() {
        assert!(plugin_env_allowlist().contains(&"SystemRoot"));
    }

    #[test]
    fn command_builds() {
        // Smoke: the builder must compile and be usable on every platform.
        let c = command("git");
        assert_eq!(c.get_program(), std::ffi::OsStr::new("git"));
    }

    /// 端点路径必须在 `sun_path` 上限之内 —— macOS 104 / Linux 108 字节。
    /// 用户名可以很长,这里断言而不是假设(spec §3.4)。
    #[cfg(unix)]
    #[test]
    fn ipc_endpoint_fits_sun_path() {
        let _guard = ipc_test_guard();
        let p = super::ipc::endpoint().expect("endpoint resolvable");
        let len = p.as_os_str().len();
        assert!(len < 104, "socket path too long ({len}): {}", p.display());
    }

    /// 一个往返:listen → connect → 写一帧 → 读回来。两个平台各自的分支都要过。
    #[tokio::test]
    async fn ipc_round_trip() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let _guard = ipc_test_guard();
        let mut listener = super::ipc::listen().await.expect("listen");
        let server = tokio::spawn(async move {
            let stream = listener.accept().await.expect("accept");
            let (r, mut w) = tokio::io::split(stream);
            let mut lines = BufReader::new(r).lines();
            let line = lines.next_line().await.unwrap().unwrap();
            w.write_all(format!("echo:{line}\n").as_bytes()).await.unwrap();
        });
        let stream = super::ipc::connect().await.expect("connect");
        let (r, mut w) = tokio::io::split(stream);
        w.write_all(b"hello\n").await.unwrap();
        let mut lines = BufReader::new(r).lines();
        assert_eq!(lines.next_line().await.unwrap().unwrap(), "echo:hello");
        server.await.unwrap();
    }

    /// 僵尸 socket:主程序崩溃后文件残留,再 listen 必须能重建(spec §3.4、§8.6)。
    #[cfg(unix)]
    #[tokio::test]
    async fn stale_socket_file_is_reclaimed() {
        let _guard = ipc_test_guard();
        let path = super::ipc::endpoint().unwrap();
        let _ = std::fs::remove_file(&path);
        // 造一个「有文件但没人监听」的现场 —— 正是崩溃后留下的样子。
        std::fs::write(&path, b"").unwrap();
        assert!(path.exists());
        let _l = super::ipc::listen().await.expect("必须能回收僵尸 socket");
        let _ = std::fs::remove_file(&path);
    }

    /// 反过来:已有实例在健康监听时,第二次 listen 必须**失败**而不是把
    /// 对方的 socket 删掉。无脑 unlink 会踢掉一个正在服务的实例。
    #[cfg(unix)]
    #[tokio::test]
    async fn live_listener_is_not_evicted() {
        let _guard = ipc_test_guard();
        let path = super::ipc::endpoint().unwrap();
        let _ = std::fs::remove_file(&path);
        let _first = super::ipc::listen().await.expect("first listen");
        let second = super::ipc::listen().await;
        assert!(second.is_err(), "健康实例不得被顶掉");
        assert!(path.exists(), "对方的 socket 文件必须还在");
        let _ = std::fs::remove_file(&path);
    }
}
