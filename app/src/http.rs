//! Minimal HTTPS GET on top of WinHTTP.
//!
//! Windows already ships a TLS stack and the system certificate store, so using it avoids
//! bundling rustls plus a root-certificate bundle - that is megabytes of code and data we would
//! otherwise carry in our address space for one request per launch.

use std::ptr::{null, null_mut};

use windows_sys::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Split "https://host/path" into (host, path). Only https is supported.
fn split_url(url: &str) -> Result<(String, String), String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or("only https URLs are supported")?;
    match rest.find('/') {
        Some(i) => Ok((rest[..i].to_string(), rest[i..].to_string())),
        None => Ok((rest.to_string(), "/".to_string())),
    }
}

/// GET a URL, following redirects (WinHTTP does that by default), returning the body.
/// `limit` caps how much we are willing to read.
pub fn get(url: &str, limit: usize) -> Result<Vec<u8>, String> {
    let (host, path) = split_url(url)?;
    let agent = wide("SimDisplayFixer");
    let hostw = wide(&host);
    let pathw = wide(&path);
    let verb = wide("GET");

    unsafe {
        let session = WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        );
        if session.is_null() {
            return Err("could not start a network session".into());
        }
        let guard_session = Handle(session);

        let conn = WinHttpConnect(session, hostw.as_ptr(), 443, 0);
        if conn.is_null() {
            return Err(format!("could not reach {host}"));
        }
        let guard_conn = Handle(conn);

        let req = WinHttpOpenRequest(
            conn,
            verb.as_ptr(),
            pathw.as_ptr(),
            null(),
            null(),
            null_mut(),
            WINHTTP_FLAG_SECURE,
        );
        if req.is_null() {
            return Err("could not create the request".into());
        }
        let guard_req = Handle(req);

        if WinHttpSendRequest(req, null(), 0, null(), 0, 0, 0) == 0 {
            return Err("the request could not be sent".into());
        }
        if WinHttpReceiveResponse(req, null_mut()) == 0 {
            return Err("no response from the server".into());
        }

        let mut out: Vec<u8> = Vec::new();
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(req, &mut avail) == 0 {
                return Err("the download was interrupted".into());
            }
            if avail == 0 {
                break;
            }
            let start = out.len();
            if start + avail as usize > limit {
                return Err("the download was larger than expected".into());
            }
            out.resize(start + avail as usize, 0);
            let mut read: u32 = 0;
            if WinHttpReadData(
                req,
                out.as_mut_ptr().add(start) as *mut core::ffi::c_void,
                avail,
                &mut read,
            ) == 0
            {
                return Err("the download was interrupted".into());
            }
            out.truncate(start + read as usize);
        }

        drop((guard_req, guard_conn, guard_session));
        Ok(out)
    }
}

/// Closes a WinHTTP handle on drop so early returns cannot leak it.
struct Handle(*mut core::ffi::c_void);
impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}
