use std::io::{self, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

pub struct FixtureServer {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<String>>>,
    thread: Option<JoinHandle<()>>,
}

impl FixtureServer {
    pub fn start(root: impl AsRef<Path>) -> io::Result<Self> {
        Self::start_on(root, 0)
    }

    pub fn start_on(root: impl AsRef<Path>, port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind((IpAddr::V4(Ipv4Addr::LOCALHOST), port))?;
        listener.set_nonblocking(true)?;
        let addr = listener.local_addr()?;
        let root = root.as_ref().to_path_buf();
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop_thread = stop.clone();
        let requests_thread = requests.clone();
        let thread = thread::Builder::new()
            .name("kiosk-fixture-httpd".into())
            .spawn(move || {
                while !stop_thread.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = serve(&root, stream, &requests_thread);
                        }
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(std::time::Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            })?;
        Ok(Self {
            addr,
            stop,
            requests,
            thread: Some(thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}:{}{}", self.addr.ip(), self.port(), path)
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("fixture request lock").clone()
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn serve(root: &Path, mut stream: TcpStream, requests: &Arc<Mutex<Vec<String>>>) -> io::Result<()> {
    let mut buffer = [0u8; 8192];
    let count = stream.read(&mut buffer)?;
    let request = String::from_utf8_lossy(&buffer[..count]);
    let target = request
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("GET "))
        .and_then(|line| line.split_whitespace().next())
        .unwrap_or("/");
    eprintln!("GET {target} HTTP/1.1");
    requests
        .lock()
        .expect("fixture request lock")
        .push(target.to_string());

    let path = target.split('?').next().unwrap_or("/");
    let relative = path.trim_start_matches('/');
    let safe = !relative.split('/').any(|component| component == "..");
    let file = safe.then(|| root.join(relative));
    let (status, content_type, disposition, body) = match file {
        Some(path) if path.is_file() => (
            "200 OK",
            content_type(&path),
            (relative == "attachment.bin").then_some("attachment; filename=attachment.bin"),
            std::fs::read(path).unwrap_or_default(),
        ),
        _ => (
            "404 Not Found",
            "text/plain; charset=utf-8",
            None,
            b"not found".to_vec(),
        ),
    };
    let disposition = disposition
        .map(|value| format!("Content-Disposition: {value}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: {content_type}\r\n{disposition}Connection: close\r\n\r\n",
        body.len(),
    )?;
    stream.write_all(&body)
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("json") => "application/json",
        Some("mp4") => "video/mp4",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("kiosk-smoke-httpd-{suffix}"));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn the_fixture_server_serves_a_file_from_a_directory() {
        let dir = temp_dir();
        std::fs::write(dir.join("home.html"), "<h1>home</h1>").unwrap();
        let server = FixtureServer::start(&dir).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
        write!(stream, "GET /home.html HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut body = String::new();
        stream.read_to_string(&mut body).unwrap();
        assert!(body.contains("<h1>home</h1>"));
        assert_eq!(server.requests(), vec!["/home.html"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_attachment_fixture_has_download_headers() {
        let dir = temp_dir();
        std::fs::write(dir.join("attachment.bin"), "download").unwrap();
        let server = FixtureServer::start(&dir).unwrap();
        let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
        write!(
            stream,
            "GET /attachment.bin HTTP/1.1\r\nHost: localhost\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.contains("Content-Disposition: attachment; filename=attachment.bin"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
