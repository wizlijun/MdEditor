//! Standalone macOS location helper — a faithful port of fulldecent/CoreLocationCLI.
//!
//! WHY a separate process: earlier in-process attempts ran CLLocationManager on
//! Tauri/tao's main event loop, where the authorization prompt and delegate
//! callbacks were never delivered (status stayed NotDetermined, no prompt — five
//! releases in a row). CoreLocationCLI proves the reliable pattern: a process
//! whose main thread runs a CLEAN `RunLoop.main.run()` doing nothing but
//! CoreLocation. This binary does exactly that. note.md.app launches it on
//! demand; macOS attributes the request to the responsible parent (the signed,
//! notarized note.md.app carrying NSLocationUsageDescription), so the prompt
//! reads "note.md wants to use your location".
//!
//! Output: one line of JSON `{country,province,city,poi,latitude,longitude}` on
//! stdout, then exit 0. Exit codes: 1 denied / geocode failure, 2 timeout, 3 no
//! location in update.
#![allow(deprecated)] // CLPlacemark accessors are deprecated in favor of MapKit; still the CLI-friendly API.
use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AllocAnyThread, DefinedClass};
use objc2_core_location::{
    CLAuthorizationStatus, CLGeocoder, CLLocation, CLLocationManager, CLLocationManagerDelegate,
    CLPlacemark,
};
use objc2_foundation::{NSArray, NSError, NSRunLoop, NSString};

fn verbose() -> bool {
    std::env::args().any(|a| a == "-v" || a == "--verbose")
}

struct Ivars {
    manager: Retained<CLLocationManager>,
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
            unsafe { manager.stopUpdatingLocation() };
            let Some(loc) = locations.lastObject() else {
                eprintln!("location: empty update");
                std::process::exit(3);
            };
            let coord = loc.coordinate();
            let (lat, lon) = (coord.latitude, coord.longitude);
            // Reverse geocode; completion lands on this thread's run loop.
            let geocoder = self.ivars().geocoder.clone();
            let block = RcBlock::new(
                move |placemarks: *mut NSArray<CLPlacemark>, error: *mut NSError| {
                    print_and_exit(placemarks, error, lat, lon);
                },
            );
            type GeoBlock = block2::Block<dyn Fn(*mut NSArray<CLPlacemark>, *mut NSError)>;
            let block_ptr: *mut GeoBlock = &*block as *const GeoBlock as *mut GeoBlock;
            unsafe { geocoder.reverseGeocodeLocation_completionHandler(&loc, block_ptr) };
            // Keep the block alive until the completion fires (process exits there).
            std::mem::forget(block);
        }

        #[unsafe(method(locationManager:didFailWithError:))]
        unsafe fn did_fail(&self, _manager: &CLLocationManager, error: &NSError) {
            eprintln!(
                "location: didFailWithError code={} {}",
                error.code(),
                error.localizedDescription()
            );
            // kCLErrorDenied == 1
            std::process::exit(1);
        }

        #[unsafe(method(locationManagerDidChangeAuthorization:))]
        unsafe fn did_change_auth(&self, manager: &CLLocationManager) {
            let status = unsafe { manager.authorizationStatus() };
            if verbose() {
                eprintln!("location: authorization status = {}", status.0);
            }
            if matches!(
                status,
                CLAuthorizationStatus::Denied | CLAuthorizationStatus::Restricted
            ) {
                eprintln!("location: denied — enable note.md in System Settings ▸ Privacy & Security ▸ Location Services");
                std::process::exit(1);
            }
        }
    }
);

impl Delegate {
    fn new() -> Retained<Self> {
        let manager = unsafe { CLLocationManager::new() };
        let geocoder = unsafe { CLGeocoder::new() };
        let this = Self::alloc().set_ivars(Ivars { manager, geocoder });
        unsafe { msg_send![super(this), init] }
    }
}

fn opt(s: Option<Retained<NSString>>) -> String {
    s.map(|v| v.to_string()).unwrap_or_default()
}

fn print_and_exit(placemarks: *mut NSArray<CLPlacemark>, error: *mut NSError, lat: f64, lon: f64) {
    if !placemarks.is_null() {
        let arr = unsafe { &*placemarks };
        if let Some(pm) = arr.firstObject() {
            let poi = unsafe { pm.areasOfInterest() }
                .and_then(|a| a.firstObject())
                .map(|s| s.to_string())
                .or_else(|| unsafe { pm.name() }.map(|s| s.to_string()))
                .unwrap_or_default();
            let country = opt(unsafe { pm.country() });
            let province = opt(unsafe { pm.administrativeArea() });
            let city = opt(unsafe { pm.locality() });
            // Minimal hand-escaped JSON (fields are place names — no control chars).
            let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
            println!(
                "{{\"country\":\"{}\",\"province\":\"{}\",\"city\":\"{}\",\"poi\":\"{}\",\"latitude\":{:.6},\"longitude\":{:.6}}}",
                esc(&country), esc(&province), esc(&city), esc(&poi), lat, lon
            );
            std::process::exit(0);
        }
    }
    let msg = if error.is_null() {
        "no placemark".to_string()
    } else {
        unsafe { (*error).localizedDescription() }.to_string()
    };
    eprintln!("location: reverse geocode failed: {msg}");
    std::process::exit(1);
}

fn main() {
    // Timeout guard (CoreLocationCLI uses 10s; allow a little more for geocode).
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(20));
        eprintln!("location: timed out waiting for a fix");
        std::process::exit(2);
    });

    let delegate = Delegate::new();
    unsafe {
        let m = &delegate.ivars().manager;
        m.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        // Match CoreLocationCLI: just startUpdatingLocation. On macOS this
        // triggers the authorization prompt when status is NotDetermined.
        m.startUpdatingLocation();
    }
    // Keep the delegate (and its manager/geocoder) alive for the process life.
    std::mem::forget(delegate);

    // Clean main run loop — the whole point of the separate-process design.
    NSRunLoop::mainRunLoop().run();
}
