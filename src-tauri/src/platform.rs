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
        listen_at(&endpoint()?).await
    }

    /// `listen()`'s actual logic, with the path parameterized out — purely so
    /// a test can point this at a scratch file instead of the machine's real,
    /// shared MCP socket (mirrors `mcp::gate::remove_socket_file`'s reasoning).
    /// No test may call `endpoint()` and pass its result here; that would
    /// bind/unlink the one socket path every note.md instance on the machine
    /// shares, including a real GUI that may be serving MCP while `cargo
    /// test` runs.
    #[cfg(unix)]
    async fn listen_at(path: &std::path::Path) -> io::Result<Listener> {
        use std::os::unix::fs::PermissionsExt;
        if path.exists() {
            match tokio::net::UnixStream::connect(path).await {
                Ok(_) => return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    "another note.md instance is already serving MCP",
                )),
                Err(_) => { let _ = std::fs::remove_file(path); }
            }
        }
        let l = tokio::net::UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Listener(l))
    }

    #[cfg(unix)]
    pub async fn connect() -> io::Result<tokio::net::UnixStream> {
        connect_at(&endpoint()?).await
    }

    /// `connect()`'s actual logic, path parameterized for the same reason as
    /// `listen_at`.
    #[cfg(unix)]
    async fn connect_at(path: &std::path::Path) -> io::Result<tokio::net::UnixStream> {
        tokio::net::UnixStream::connect(path).await
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
            // `self.next` reads `None` in two cases: this is the very first
            // call (never true — `listen()` always seeds it), or a *previous*
            // call left it empty because preparing the spare instance failed
            // below. Retrying the creation here — instead of treating `None`
            // as "listener permanently closed" — is what makes a transient
            // failure self-healing rather than fatal: without this, one
            // failed `CreateNamedPipe` bricks every later `accept()` forever
            // (each returns `Err` instantly, with no I/O wait), spinning the
            // caller's loop at 100% CPU with MCP silently dead.
            let server = match self.next.take() {
                Some(s) => s,
                None => create_owner_only_pipe(&self.name, false)?,
            };
            server.connect().await?;
            // 下一个实例必须在把当前这个交出去之前建好,否则客户端会在
            // 两次 accept 之间撞上 ERROR_FILE_NOT_FOUND。
            //
            // If THIS creation fails, do not let it discard the connection
            // we just successfully accepted above (`server.connect()` already
            // succeeded — a real client is on the other end of `server`).
            // Leave `self.next` as `None` instead: the next `accept()` call
            // retries creation via the branch above. The failure still
            // surfaces to the caller — on the *next* call, if the retry also
            // fails — so it is never silently lost, just not blamed on the
            // connection that already succeeded.
            self.next = create_owner_only_pipe(&self.name, false).ok();
            Ok(server)
        }
    }

    #[cfg(windows)]
    pub async fn listen() -> io::Result<Listener> {
        listen_at(&endpoint()?.to_string_lossy()).await
    }

    /// `listen()`'s actual logic, with the pipe name parameterized out —
    /// same reasoning as the unix `listen_at`: no test may resolve
    /// `endpoint()` and pass its result here, since that names the one pipe
    /// every note.md instance on the machine shares.
    #[cfg(windows)]
    async fn listen_at(name: &str) -> io::Result<Listener> {
        let first = create_owner_only_pipe(name, true)?;
        Ok(Listener { name: name.to_string(), next: Some(first) })
    }

    #[cfg(windows)]
    pub async fn connect() -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
        connect_at(&endpoint()?.to_string_lossy()).await
    }

    /// `connect()`'s actual logic, name parameterized for the same reason as
    /// `listen_at`.
    #[cfg(windows)]
    async fn connect_at(name: &str) -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
        use tokio::net::windows::named_pipe::ClientOptions;
        ClientOptions::new().open(name)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// 端点路径必须在 `sun_path` 上限之内 —— macOS 104 / Linux 108 字节。
        /// 用户名可以很长,这里断言而不是假设(spec §3.4)。这是本模块里
        /// 唯一还解析真实 `endpoint()` 的测试 —— 它只读长度,不 bind、不
        /// unlink,所以不需要下面几条测试用的 scratch 端点隔离,也不需要锁。
        #[cfg(unix)]
        #[test]
        fn ipc_endpoint_fits_sun_path() {
            let p = endpoint().expect("endpoint resolvable");
            let len = p.as_os_str().len();
            assert!(len < 104, "socket path too long ({len}): {}", p.display());
        }

        /// 每条测试各自专属的 scratch 端点 —— **绝不能**解析真实
        /// `endpoint()`:那是这台机器上每个 note.md 实例共享的唯一 socket
        /// 路径,`cargo test` 跑的时候真实 GUI 可能正在上面服务 MCP
        /// (mirrors `mcp::gate::remove_socket_file`'s reasoning, and is the
        /// fix for the bug that reasoning warns about: these tests used to
        /// call `endpoint()` directly and could delete/rebind a live
        /// instance's real socket). 用测试函数名当后缀,配合 pid,保证这几
        /// 条测试并发跑也不会互相踩文件 —— 因此不再需要旧版靠一把全局锁
        /// 序列化它们的 `IPC_TEST_LOCK`。
        #[cfg(unix)]
        fn scratch_path(tag: &str) -> std::path::PathBuf {
            std::env::temp_dir()
                .join(format!("notemd-platform-ipc-test-{}-{tag}.sock", std::process::id()))
        }

        #[cfg(windows)]
        fn scratch_name(tag: &str) -> String {
            format!(r"\\.\pipe\notemd-platform-ipc-test-{}-{tag}", std::process::id())
        }

        /// 一个往返:listen → connect → 写一帧 → 读回来。
        #[cfg(unix)]
        #[tokio::test]
        async fn ipc_round_trip() {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            let path = scratch_path("round-trip");
            let _ = std::fs::remove_file(&path);
            let mut listener = listen_at(&path).await.expect("listen");
            let server = tokio::spawn(async move {
                let stream = listener.accept().await.expect("accept");
                let (r, mut w) = tokio::io::split(stream);
                let mut lines = BufReader::new(r).lines();
                let line = lines.next_line().await.unwrap().unwrap();
                w.write_all(format!("echo:{line}\n").as_bytes()).await.unwrap();
            });
            let stream = connect_at(&path).await.expect("connect");
            let (r, mut w) = tokio::io::split(stream);
            w.write_all(b"hello\n").await.unwrap();
            let mut lines = BufReader::new(r).lines();
            assert_eq!(lines.next_line().await.unwrap().unwrap(), "echo:hello");
            server.await.unwrap();
            let _ = std::fs::remove_file(&path);
        }

        /// Windows 镜像:同样的往返,走命名管道分支。未在本仓库验证过 ——
        /// 见任务报告里的说明(交叉依赖在这台机器上没有可用的 Windows
        /// 工具链)。
        #[cfg(windows)]
        #[tokio::test]
        async fn ipc_round_trip() {
            use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
            let name = scratch_name("round-trip");
            let mut listener = listen_at(&name).await.expect("listen");
            let server = tokio::spawn(async move {
                let stream = listener.accept().await.expect("accept");
                let (r, mut w) = tokio::io::split(stream);
                let mut lines = BufReader::new(r).lines();
                let line = lines.next_line().await.unwrap().unwrap();
                w.write_all(format!("echo:{line}\n").as_bytes()).await.unwrap();
            });
            let stream = connect_at(&name).await.expect("connect");
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
            let path = scratch_path("stale-reclaim");
            let _ = std::fs::remove_file(&path);
            // 造一个「有文件但没人监听」的现场 —— 正是崩溃后留下的样子。
            std::fs::write(&path, b"").unwrap();
            assert!(path.exists());
            let _l = listen_at(&path).await.expect("必须能回收僵尸 socket");
            let _ = std::fs::remove_file(&path);
        }

        /// 反过来:已有实例在健康监听时,第二次 listen 必须**失败**而不是把
        /// 对方的 socket 删掉。无脑 unlink 会踢掉一个正在服务的实例。
        #[cfg(unix)]
        #[tokio::test]
        async fn live_listener_is_not_evicted() {
            let path = scratch_path("live-not-evicted");
            let _ = std::fs::remove_file(&path);
            let _first = listen_at(&path).await.expect("first listen");
            let second = listen_at(&path).await;
            assert!(second.is_err(), "健康实例不得被顶掉");
            assert!(path.exists(), "对方的 socket 文件必须还在");
            let _ = std::fs::remove_file(&path);
        }

        /// finding 7: `listen_at` sets `0o600` on the socket file right
        /// after `bind()`, and until now nothing checked the result — that
        /// one line is the entire enforcement of spec §7's "access control
        /// is the OS" argument (unix: file permissions). If it silently
        /// stopped firing, every local account on the machine could connect
        /// and read vault search results over the pipe. Trivial and
        /// isolated now that `listen_at` takes a path.
        #[cfg(unix)]
        #[tokio::test]
        async fn socket_file_is_owner_only() {
            use std::os::unix::fs::PermissionsExt;
            let path = scratch_path("owner-only-perms");
            let _ = std::fs::remove_file(&path);
            let _l = listen_at(&path).await.expect("listen");
            let mode = std::fs::metadata(&path).expect("socket file must exist").permissions().mode();
            assert_eq!(
                mode & 0o777, 0o600,
                "socket file must be 0600 (owner read/write only), got {:o}", mode & 0o777
            );
            let _ = std::fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

}
