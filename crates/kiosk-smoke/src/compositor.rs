use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub struct Compositor {
    child: Child,
    runtime_dir: String,
    wayland_display: String,
}

impl Compositor {
    pub fn weston_headless() -> io::Result<Self> {
        let runtime_dir = unique_runtime_dir();
        std::fs::create_dir_all(&runtime_dir)?;
        let wayland_display = "wayland-kiosk-smoke".to_string();
        let log = std::fs::File::create(runtime_dir.join("weston.log"))?;
        let child = Command::new("weston")
            .args([
                "--backend=headless-backend.so",
                "--idle-time=0",
                "--width=1280",
                "--height=720",
                &format!("--socket={wayland_display}"),
            ])
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()?;
        wait_for_socket(&runtime_dir, &wayland_display, child.id())?;
        Ok(Self {
            child,
            runtime_dir: runtime_dir.to_string_lossy().into_owned(),
            wayland_display,
        })
    }

    pub fn cage(command: &Path, args: &[&str]) -> io::Result<Self> {
        let runtime_dir = unique_runtime_dir();
        std::fs::create_dir_all(&runtime_dir)?;
        let wayland_display = "wayland-kiosk-smoke".to_string();
        let log = std::fs::File::create(runtime_dir.join("cage.log"))?;
        let mut child_command = Command::new("cage");
        child_command
            .env("XDG_RUNTIME_DIR", &runtime_dir)
            .env("WAYLAND_DISPLAY", &wayland_display)
            .env("WLR_BACKENDS", "headless")
            .arg("--")
            .arg(command)
            .args(args)
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log));
        let child = child_command.spawn()?;
        wait_for_socket(&runtime_dir, &wayland_display, child.id())?;
        Ok(Self {
            child,
            runtime_dir: runtime_dir.to_string_lossy().into_owned(),
            wayland_display,
        })
    }

    pub fn env(&self) -> (&str, &str) {
        (&self.runtime_dir, &self.wayland_display)
    }

    pub fn log_path(&self) -> std::path::PathBuf {
        Path::new(&self.runtime_dir).join("weston.log")
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.runtime_dir);
    }
}

fn unique_runtime_dir() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("kiosk-smoke-runtime-{}", std::process::id()))
}

fn wait_for_socket(runtime_dir: &Path, display: &str, pid: u32) -> io::Result<()> {
    let socket = runtime_dir.join(display);
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if socket.exists() {
            return Ok(());
        }
        if let Ok(Some(status)) = wait_status(pid) {
            return Err(io::Error::other(format!("compositor exited with {status}")));
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        format!("compositor did not create {}", socket.display()),
    ))
}

#[cfg(unix)]
fn wait_status(pid: u32) -> io::Result<Option<i32>> {
    let status = Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()?;
    Ok((!status.success()).then_some(-1))
}

#[cfg(not(unix))]
fn wait_status(_pid: u32) -> io::Result<Option<i32>> {
    Ok(None)
}
