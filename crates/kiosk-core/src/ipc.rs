use serde::{Deserialize, Serialize};

pub const PING_INTERVAL_S: u64 = 5;

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Frame {
    Ready, // main → launcher: webview up + first nav committed (arch-03)
    Ping,  // main → launcher: liveness, every PING_INTERVAL_S
}

#[derive(Debug, thiserror::Error)]
#[error("bad heartbeat frame: {0}")]
pub struct IpcError(String);

/// One '\n'-terminated JSON line.
pub fn encode(frame: &Frame) -> String {
    let mut s = serde_json::to_string(frame).expect("Frame is always serializable");
    s.push('\n');
    s
}

/// One line → Frame. Malformed / unknown-type → Err (never panics; the launcher must
/// survive garbage or a newer main's P2 frame on the pipe).
pub fn decode(line: &str) -> Result<Frame, IpcError> {
    serde_json::from_str(line.trim()).map_err(|e| IpcError(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrips_every_frame() {
        for f in [Frame::Ready, Frame::Ping] {
            assert_eq!(decode(encode(&f).trim()).unwrap(), f);
        }
    }
    #[test]
    fn encode_is_one_newline_terminated_line() {
        let s = encode(&Frame::Ping);
        assert!(s.ends_with('\n'));
        assert_eq!(s.matches('\n').count(), 1);
    }
    #[test]
    fn garbage_is_err_not_panic() {
        assert!(decode("not json").is_err());
        assert!(decode("").is_err());
        assert!(
            decode("{\"type\":\"unknown\"}").is_err(),
            "forward-compat: unknown frame ignored, not a crash"
        );
    }
}
