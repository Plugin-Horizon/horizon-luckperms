//! `BytebinClient`: the web editor's paste service. POST gzip-compressed JSON to `<url>post`, the
//! key comes back in the `Location` header; GET `<url><key>` returns it. Blocking (ureq): it
//! runs on the worker thread, never on the game's tick. Everything downloaded is capped.

use std::cell::RefCell;
use std::rc::Rc;

/// The biggest document taken from bytebin (an editor reply is a few KiB; this is generous).
pub const MAX_DOWNLOAD: usize = 4 * 1024 * 1024;
/// The biggest payload sent (compressed); bytebin itself refuses roughly 10 MiB.
pub const MAX_UPLOAD: usize = 8 * 1024 * 1024;
const TIMEOUT_MS: u64 = 15_000;

#[derive(Debug, PartialEq, Eq)]
pub enum HttpError {
    /// The service answered with an error status.
    Status(u16, String),
    /// Anything else (no route, TLS, timeout, too big...). Printed to the console only.
    Other(String),
}

pub trait Bytebin {
    /// Uploads gzip-compressed JSON; the key.
    fn post(&self, gz: &[u8]) -> Result<String, HttpError>;
    /// The document under `id` (already checked by [`valid_key`]), decompressed if it came gzipped.
    fn get(&self, id: &str) -> Result<Vec<u8>, HttpError>;
}

/// A key bytebin hands out: letters, digits, `_` and `-`. Anything else would let a code
/// rewrite the request path.
pub fn valid_key(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// gzip, as `WebEditorRequest.encode` writes it.
#[cfg(feature = "net")]
pub fn gzip(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = enc.write_all(data);
    enc.finish().unwrap_or_default()
}

/// Inflates gzip, at most `cap` bytes out.
#[cfg(feature = "net")]
pub fn gunzip(data: &[u8], cap: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data).take(cap as u64 + 1).read_to_end(&mut out).map_err(|e| e.to_string())?;
    if out.len() > cap {
        return Err("decompressed data is too big".into());
    }
    Ok(out)
}

pub struct Http {
    base: String,
}

impl Http {
    pub fn new(url: &str) -> Self {
        Http { base: if url.ends_with('/') { url.to_owned() } else { format!("{url}/") } }
    }
}

#[cfg(feature = "net")]
mod net {
    use super::*;
    use std::io::Read;

    fn agent() -> ureq::Agent {
        ureq::AgentBuilder::new().timeout(std::time::Duration::from_millis(TIMEOUT_MS)).redirects(0).build()
    }

    /// The status text, as OkHttp prints `response.message()`; no control characters.
    fn status_error(code: u16, r: ureq::Response) -> HttpError {
        let text: String = r.status_text().chars().filter(|c| !c.is_control()).take(100).collect();
        HttpError::Status(code, text)
    }

    impl Bytebin for Http {
        fn post(&self, gz: &[u8]) -> Result<String, HttpError> {
            let r = agent()
                .post(&format!("{}post", self.base))
                .set("User-Agent", "luckperms/editor")
                .set("Content-Type", "application/json; charset=utf-8")
                .set("Content-Encoding", "gzip")
                .send_bytes(gz);
            match r {
                Ok(resp) => {
                    let key = resp.header("Location").map(str::to_owned);
                    match key {
                        Some(k) if valid_key(&k) => Ok(k),
                        Some(_) => Err(HttpError::Other("bytebin returned an unusable key".into())),
                        None => Err(HttpError::Other("Key not returned".into())),
                    }
                }
                Err(ureq::Error::Status(code, resp)) => Err(status_error(code, resp)),
                Err(e) => Err(HttpError::Other(e.to_string())),
            }
        }

        fn get(&self, id: &str) -> Result<Vec<u8>, HttpError> {
            let r = agent().get(&format!("{}{id}", self.base)).set("User-Agent", "luckperms").call();
            match r {
                Ok(resp) => {
                    let mut body = Vec::new();
                    resp.into_reader().take(MAX_DOWNLOAD as u64 + 1).read_to_end(&mut body).map_err(|e| HttpError::Other(e.to_string()))?;
                    if body.len() > MAX_DOWNLOAD {
                        return Err(HttpError::Other("response is too big".into()));
                    }
                    if body.starts_with(&[0x1f, 0x8b]) {
                        return gunzip(&body, MAX_DOWNLOAD).map_err(HttpError::Other);
                    }
                    Ok(body)
                }
                Err(ureq::Error::Status(code, resp)) => Err(status_error(code, resp)),
                Err(e) => Err(HttpError::Other(e.to_string())),
            }
        }
    }
}

#[cfg(not(feature = "net"))]
impl Bytebin for Http {
    fn post(&self, _: &[u8]) -> Result<String, HttpError> {
        Err(HttpError::Other("built without the `net` feature".into()))
    }
    fn get(&self, _: &str) -> Result<Vec<u8>, HttpError> {
        Err(HttpError::Other("built without the `net` feature".into()))
    }
}

thread_local! {
    /// Tests put a fake service here (no network in tests).
    static OVERRIDE: RefCell<Option<Rc<dyn Bytebin>>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub fn set_test_client(c: Option<Rc<dyn Bytebin>>) {
    OVERRIDE.with(|o| *o.borrow_mut() = c);
}

/// Runs `f` with the service at `url` (or the test double).
pub fn with_client<R>(url: &str, f: impl FnOnce(&dyn Bytebin) -> R) -> R {
    let fake = OVERRIDE.with(|o| o.borrow().clone());
    match fake {
        Some(c) => f(&*c),
        None => f(&Http::new(url)),
    }
}
