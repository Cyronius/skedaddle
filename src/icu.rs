use std::ffi::c_void;
use std::rc::Rc;
use std::sync::OnceLock;

use chrono::NaiveDateTime;
use windows::core::{s, w};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use crate::tz::{OffsetSource, Zone};

type Status = i32;
type Open = unsafe extern "C" fn(*const u16, i32, *const u8, i32, *mut Status) -> *mut c_void;
type Close = unsafe extern "C" fn(*mut c_void);
type SetMillis = unsafe extern "C" fn(*mut c_void, f64, *mut Status);
type Get = unsafe extern "C" fn(*const c_void, i32, *mut Status) -> i32;
type Canonical = unsafe extern "C" fn(*const u16, i32, *mut u16, i32, *mut i8, *mut Status) -> i32;

const UCAL_GREGORIAN: i32 = 1;
const UCAL_ZONE_OFFSET: i32 = 15;
const UCAL_DST_OFFSET: i32 = 16;

struct Api {
    open: Open,
    close: Close,
    set_millis: SetMillis,
    get: Get,
    canonical: Canonical,
}

// Windows 10 1903+ ships ICU with the IANA zone database as icu.dll; load it lazily so older systems still start.
fn api() -> Option<&'static Api> {
    static API: OnceLock<Option<Api>> = OnceLock::new();
    API.get_or_init(|| unsafe {
        let lib = LoadLibraryW(w!("icu.dll")).ok()?;
        Some(Api {
            open: std::mem::transmute::<_, Open>(GetProcAddress(lib, s!("ucal_open"))?),
            close: std::mem::transmute::<_, Close>(GetProcAddress(lib, s!("ucal_close"))?),
            set_millis: std::mem::transmute::<_, SetMillis>(GetProcAddress(lib, s!("ucal_setMillis"))?),
            get: std::mem::transmute::<_, Get>(GetProcAddress(lib, s!("ucal_get"))?),
            canonical: std::mem::transmute::<_, Canonical>(GetProcAddress(lib, s!("ucal_getCanonicalTimeZoneID"))?),
        })
    })
    .as_ref()
}

struct IcuZone {
    api: &'static Api,
    cal: *mut c_void,
}

impl Drop for IcuZone {
    fn drop(&mut self) {
        unsafe { (self.api.close)(self.cal) }
    }
}

impl OffsetSource for IcuZone {
    fn offset_at(&self, utc: NaiveDateTime) -> i32 {
        let ms = utc.and_utc().timestamp_millis() as f64;
        let mut st: Status = 0;
        unsafe {
            (self.api.set_millis)(self.cal, ms, &mut st);
            let zone = (self.api.get)(self.cal, UCAL_ZONE_OFFSET, &mut st);
            let dst = (self.api.get)(self.cal, UCAL_DST_OFFSET, &mut st);
            if st > 0 {
                0
            } else {
                (zone + dst) / 1000
            }
        }
    }
}

/// An IANA zone from the Windows ICU database, or `None` if ICU or the zone is unknown.
pub fn zone(name: &str) -> Option<Zone> {
    let api = api()?;
    let id: Vec<u16> = name.encode_utf16().collect();
    let mut buf = [0u16; 128];
    let mut system: i8 = 0;
    let mut st: Status = 0;
    unsafe {
        (api.canonical)(id.as_ptr(), id.len() as i32, buf.as_mut_ptr(), buf.len() as i32, &mut system, &mut st);
        if st > 0 || system == 0 {
            return None;
        }
        let cal = (api.open)(id.as_ptr(), id.len() as i32, std::ptr::null(), UCAL_GREGORIAN, &mut st);
        if st > 0 || cal.is_null() {
            return None;
        }
        Some(Zone::Rules(Rc::new(IcuZone { api, cal })))
    }
}
