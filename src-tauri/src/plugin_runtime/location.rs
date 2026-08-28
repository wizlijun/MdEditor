//! Host-side CoreLocation (macOS), exposed to plugins via `host.location.get`
//! (capability `location`).
//!
//! WHY in-process (not a sidecar): TCC attributes a location request to the
//! requesting process's identity. Running CLLocationManager INSIDE note.md.app
//! makes the TCC subject `net.notemd.app` — the entry the user grants as
//! "note.md". A separate signed helper binary carries its OWN bundle id
//! (a distinct TCC subject) and gets denied, which is exactly what happened.
//!
//! WHY a dedicated thread: earlier in-process attempts ran CLLocationManager on
//! Tauri/tao's MAIN event loop, where its delegate callbacks were not delivered
//! (status stuck at NotDetermined). Here we create the manager on a DEDICATED
//! thread and pump that thread's OWN run loop with `CFRunLoop::run_in_mode`
//! (kCFRunLoopDefaultMode) — a clean run loop that reliably delivers the
//! CLLocationManager callbacks, mirroring how CoreLocationCLI drives
//! `RunLoop.main.run()`. Reverse-geocode completions land on the main queue
//! (drained by tao) and write the shared result, which this thread polls.

use serde_json::Value;

/// `kCLErrorDenied` / `CLError::Denied` raw value.
const CL_ERROR_DENIED_CODE: isize = 1;
const LOCATION_DENIED_ERROR: &str =
    "location: denied — enable note.md in System Settings ▸ Privacy & Security ▸ Location Services";

fn core_location_failure(code: isize, description: &str) -> String {
    if code == CL_ERROR_DENIED_CODE {
        LOCATION_DENIED_ERROR.into()
    } else {
        format!("location: didFailWithError code={code} {description}")
    }
}

/// Blocking one-shot location read → `{country, province, city, poi, latitude, longitude}`.
#[cfg(target_os = "macos")]
pub fn fetch_once<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Result<Value, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("notemd-location".into())
        .spawn(move || {
            let _ = tx.send(mac::run_on_thread());
        })
        .map_err(|e| format!("spawn location thread: {e}"))?;
    // Slightly longer than the in-thread deadline so the thread reports first.
    match rx.recv_timeout(std::time::Duration::from_secs(25)) {
        Ok(res) => res,
        Err(_) => Err("location: timed out (no response from location thread)".into()),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn fetch_once<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) -> Result<Value, String> {
    Err("location is only supported on macOS".into())
}

/// No-op: the request is made on demand (Location Log / the 30-min round);
/// since it runs in-process as note.md.app, an authorized user needs no prompt.
pub fn init_at_startup<R: tauri::Runtime>(_app: &tauri::AppHandle<R>) {}

#[cfg(target_os = "macos")]
#[allow(deprecated)] // CLPlacemark accessors are deprecated in favor of MapKit; still the API here.
mod mac {
    use block2::RcBlock;
    use core_foundation::runloop::{kCFRunLoopDefaultMode, CFRunLoop};
    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
    use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
    use objc2_core_location::{
        CLAuthorizationStatus, CLGeocoder, CLLocation, CLLocationManager, CLLocationManagerDelegate,
        CLPlacemark,
    };
    use objc2_foundation::{NSArray, NSError, NSString};
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const WAIT_SECS: u64 = 20;

    /// The result slot is the only thing shared across threads: the delegate
    /// (dedicated thread) and the reverse-geocode completion (main queue) both
    /// write it; the dedicated thread polls it. `Value`/`String` are `Send`.
    type Slot = Arc<Mutex<Option<Result<Value, String>>>>;

    fn set_result(slot: &Slot, res: Result<Value, String>) {
        let mut g = slot.lock().unwrap();
        if g.is_none() {
            *g = Some(res);
        }
    }

    struct Ivars {
        slot: Slot,
        geocoder: Retained<CLGeocoder>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "NotemdLocationDelegate"]
        #[ivars = Ivars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl CLLocationManagerDelegate for Delegate {
            #[unsafe(method(locationManager:didUpdateLocations:))]
            unsafe fn did_update(&self, manager: &CLLocationManager, locations: &NSArray<CLLocation>) {
                if self.ivars().slot.lock().unwrap().is_some() {
                    return;
                }
                unsafe { manager.stopUpdatingLocation() };
                let Some(loc) = locations.lastObject() else { return };
                let coord = loc.coordinate();
                let (lat, lon) = (coord.latitude, coord.longitude);
                // Reverse geocode; the completion runs on the main queue (drained
                // by tao) and writes the shared slot the dedicated thread polls.
                let slot = self.ivars().slot.clone();
                let block = RcBlock::new(
                    move |placemarks: *mut NSArray<CLPlacemark>, error: *mut NSError| {
                        set_result(&slot, placemark_result(placemarks, error, lat, lon));
                    },
                );
                type GeoBlock = block2::Block<dyn Fn(*mut NSArray<CLPlacemark>, *mut NSError)>;
                let block_ptr: *mut GeoBlock = &*block as *const GeoBlock as *mut GeoBlock;
                unsafe { self.ivars().geocoder.reverseGeocodeLocation_completionHandler(&loc, block_ptr) };
                std::mem::forget(block); // released after the completion fires
            }

            #[unsafe(method(locationManager:didFailWithError:))]
            unsafe fn did_fail(&self, _manager: &CLLocationManager, error: &NSError) {
                let description = error.localizedDescription().to_string();
                set_result(
                    &self.ivars().slot,
                    Err(super::core_location_failure(error.code(), &description)),
                );
            }

            #[unsafe(method(locationManagerDidChangeAuthorization:))]
            unsafe fn did_change_auth(&self, manager: &CLLocationManager) {
                let status = unsafe { manager.authorizationStatus() };
                match status {
                    CLAuthorizationStatus::Denied | CLAuthorizationStatus::Restricted => set_result(
                        &self.ivars().slot,
                        Err(super::LOCATION_DENIED_ERROR.into()),
                    ),
                    CLAuthorizationStatus::AuthorizedAlways
                    | CLAuthorizationStatus::AuthorizedWhenInUse => unsafe {
                        manager.startUpdatingLocation()
                    },
                    _ => {}
                }
            }
        }
    );

    impl Delegate {
        fn new(ivars: Ivars) -> Retained<Self> {
            let this = Self::alloc().set_ivars(ivars);
            unsafe { msg_send![super(this), init] }
        }
    }

    /// Runs entirely on the dedicated "notemd-location" thread.
    pub(super) fn run_on_thread() -> Result<Value, String> {
        let slot: Slot = Arc::new(Mutex::new(None));
        let geocoder = unsafe { CLGeocoder::new() };
        let manager = unsafe { CLLocationManager::new() };
        let delegate = Delegate::new(Ivars { slot: slot.clone(), geocoder });
        unsafe {
            manager.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            match manager.authorizationStatus() {
                CLAuthorizationStatus::Denied | CLAuthorizationStatus::Restricted => set_result(
                    &slot,
                    Err(super::LOCATION_DENIED_ERROR.into()),
                ),
                CLAuthorizationStatus::NotDetermined => {
                    // Fresh user: request (best effort) AND start — an authorized
                    // grant during the wait is picked up by didChangeAuthorization.
                    manager.requestWhenInUseAuthorization();
                    manager.startUpdatingLocation();
                }
                _ => manager.startUpdatingLocation(), // already authorized → just fix
            }
        }

        // Pump THIS thread's run loop (delivers the CLLocationManager callbacks)
        // and poll the shared slot (written by callbacks / the main-queue geocode).
        let deadline = Instant::now() + Duration::from_secs(WAIT_SECS);
        loop {
            if slot.lock().unwrap().is_some() {
                break;
            }
            if Instant::now() >= deadline {
                set_result(&slot, Err("location: timed out waiting for a fix".into()));
                break;
            }
            CFRunLoop::run_in_mode(
                unsafe { kCFRunLoopDefaultMode },
                Duration::from_millis(200),
                false,
            );
            std::thread::sleep(Duration::from_millis(20)); // avoid spin once CL source is gone
        }

        unsafe { manager.stopUpdatingLocation() };
        drop(delegate);
        drop(manager);
        let out = slot.lock().unwrap().take();
        out.unwrap_or_else(|| Err("location: no result".into()))
    }

    fn opt(s: Option<Retained<NSString>>) -> String {
        s.map(|v| v.to_string()).unwrap_or_default()
    }

    fn placemark_result(
        placemarks: *mut NSArray<CLPlacemark>,
        error: *mut NSError,
        lat: f64,
        lon: f64,
    ) -> Result<Value, String> {
        if !placemarks.is_null() {
            let arr = unsafe { &*placemarks };
            if let Some(pm) = arr.firstObject() {
                let poi = unsafe { pm.areasOfInterest() }
                    .and_then(|a| a.firstObject())
                    .map(|s| s.to_string())
                    .or_else(|| unsafe { pm.name() }.map(|s| s.to_string()))
                    .unwrap_or_default();
                return Ok(json!({
                    "country": opt(unsafe { pm.country() }),
                    "province": opt(unsafe { pm.administrativeArea() }),
                    "city": opt(unsafe { pm.locality() }),
                    "poi": poi,
                    "latitude": lat,
                    "longitude": lon,
                }));
            }
        }
        let msg = if error.is_null() {
            "no placemark".to_string()
        } else {
            unsafe { (*error).localizedDescription() }.to_string()
        };
        Err(format!("location: reverse geocode failed: {msg}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denied_core_location_error_is_actionable() {
        assert_eq!(
            core_location_failure(1, "kCLErrorDomain error 1"),
            LOCATION_DENIED_ERROR
        );
    }

    #[test]
    fn other_core_location_errors_keep_diagnostics() {
        assert_eq!(
            core_location_failure(2, "network unavailable"),
            "location: didFailWithError code=2 network unavailable"
        );
    }

    #[test]
    fn mac_bundle_config_includes_location_entitlement() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(
            config["bundle"]["macOS"]["entitlements"],
            "Entitlements.plist"
        );

        let entitlements = include_str!("../../Entitlements.plist");
        assert!(entitlements
            .contains("<key>com.apple.security.personal-information.location</key>\n  <true/>"));
    }
}
