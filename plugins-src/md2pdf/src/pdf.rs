//! PDF generation pipeline. Same algorithm as the prior in-process
//! src-tauri/src/pdf.rs — offscreen WKWebView, evaluateJavaScript to read
//! scrollHeight, per-page createPDFWithConfiguration, PDFKit merge with
//! A4 + margin expansion.
//!
//! Adapted for a CLI process: NSApp::run / NSApp::stop drive the runloop
//! instead of Tauri's run_on_main_thread; a sync `Rc<RefCell<…>>` replaces
//! tokio::oneshot.

#![cfg(target_os = "macos")]

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{class, define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::{
    MainThreadMarker, NSData, NSDictionary, NSError, NSNumber, NSObject, NSObjectProtocol,
    NSRect, NSString, NSURL,
};
use objc2_pdf_kit::{
    PDFDisplayBox, PDFDocument, PDFDocumentOptimizeImagesForScreenOption,
    PDFDocumentSaveImagesAsJPEGOption, PDFPage,
};
use objc2_web_kit::{
    WKNavigation, WKNavigationDelegate, WKPDFConfiguration, WKWebView, WKWebViewConfiguration,
};

/// A4 page size in PostScript points (1pt = 1/72 inch).
const A4_W: f64 = 595.0;
const A4_H: f64 = 842.0;
/// Visible margins inside each A4 page. ~20mm horizontal, ~25mm vertical.
const MARGIN_H: f64 = 57.0;
const MARGIN_V: f64 = 71.0;
/// Inner content area = A4 minus margins.
const INNER_W: f64 = A4_W - 2.0 * MARGIN_H;
const INNER_H: f64 = A4_H - 2.0 * MARGIN_V;

/// Result is written here by the navigation delegate; main observes it
/// after NSApp::run returns.
pub type ResultCell = Rc<RefCell<Option<Result<(), String>>>>;

pub(super) struct DelegateIvars {
    webview: RefCell<Option<Retained<WKWebView>>>,
    self_ref: RefCell<Option<Retained<NavDelegate>>>,
    output_path: String,
    pages: RefCell<Vec<Retained<NSData>>>,
    num_pages: Cell<usize>,
    current_page: Cell<usize>,
    result: ResultCell,
    app: Retained<NSApplication>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    #[name = "Md2PdfNavDelegate"]
    pub(super) struct NavDelegate;

    unsafe impl NSObjectProtocol for NavDelegate {}

    unsafe impl WKNavigationDelegate for NavDelegate {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _w: &WKWebView, _n: Option<&WKNavigation>) {
            self.start_height_measurement();
        }
        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(&self, _w: &WKWebView, _n: Option<&WKNavigation>, error: &NSError) {
            let msg = error.localizedDescription().to_string();
            self.dispatch_result(Err(format!("WKWebView navigation failed: {msg}")));
        }
        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(
            &self,
            _w: &WKWebView,
            _n: Option<&WKNavigation>,
            error: &NSError,
        ) {
            let msg = error.localizedDescription().to_string();
            self.dispatch_result(Err(format!("WKWebView provisional navigation failed: {msg}")));
        }
    }
);

impl NavDelegate {
    fn new(
        mtm: MainThreadMarker,
        webview: Retained<WKWebView>,
        output_path: String,
        result: ResultCell,
        app: Retained<NSApplication>,
    ) -> Retained<Self> {
        let ivars = DelegateIvars {
            webview: RefCell::new(Some(webview)),
            self_ref: RefCell::new(None),
            output_path,
            pages: RefCell::new(Vec::new()),
            num_pages: Cell::new(0),
            current_page: Cell::new(0),
            result,
            app,
        };
        let this = Self::alloc(mtm).set_ivars(ivars);
        let retained: Retained<Self> = unsafe { msg_send![super(this), init] };
        // Self-retain for the duration of the async chain.
        *retained.ivars().self_ref.borrow_mut() = Some(retained.clone());
        retained
    }

    /// Send result + tear down + stop the runloop. Idempotent.
    fn dispatch_result(&self, result: Result<(), String>) {
        *self.ivars().result.borrow_mut() = Some(result);
        let _ = self.ivars().webview.borrow_mut().take();
        let _ = self.ivars().self_ref.borrow_mut().take();
        self.ivars().app.stop(None);
    }

    /// Step 1: ask the page how tall its content is.
    fn start_height_measurement(&self) {
        let webview = match self.ivars().webview.borrow().as_ref() {
            Some(w) => w.clone(),
            None => {
                self.dispatch_result(Err("WKWebView dropped before height read".into()));
                return;
            }
        };
        let self_for_block: Retained<NavDelegate> = self
            .ivars()
            .self_ref
            .borrow()
            .as_ref()
            .expect("self_ref set before nav finish")
            .clone();

        let block = RcBlock::new(move |result: *mut AnyObject, err: *mut NSError| {
            if !err.is_null() {
                let err_obj = unsafe { &*err };
                let msg = err_obj.localizedDescription().to_string();
                self_for_block.dispatch_result(Err(format!("evaluateJavaScript failed: {msg}")));
                return;
            }
            if result.is_null() {
                self_for_block
                    .dispatch_result(Err("evaluateJavaScript returned no result".into()));
                return;
            }
            let number = unsafe { &*(result as *mut NSNumber) };
            let height: f64 = number.doubleValue();

            // Compute number of pages by INNER content height — PDFKit pads
            // each captured page to A4 with margins later.
            let num_pages = ((height / INNER_H as f64).ceil() as usize).max(1);
            self_for_block.ivars().num_pages.set(num_pages);
            self_for_block.ivars().current_page.set(0);
            self_for_block.capture_next_page();
        });
        let js = NSString::from_str("document.documentElement.scrollHeight");
        unsafe {
            webview.evaluateJavaScript_completionHandler(&js, Some(&block));
        }
    }

    /// Step 2 (loop): capture the current page slice as a single-page PDF.
    fn capture_next_page(&self) {
        let i = self.ivars().current_page.get();
        let n = self.ivars().num_pages.get();
        if i >= n {
            self.finalize();
            return;
        }
        let webview = match self.ivars().webview.borrow().as_ref() {
            Some(w) => w.clone(),
            None => {
                self.dispatch_result(Err("WKWebView dropped during page capture".into()));
                return;
            }
        };
        let rect = NSRect::new(
            objc2_foundation::NSPoint::new(0.0, i as f64 * INNER_H),
            objc2_foundation::NSSize::new(INNER_W, INNER_H),
        );
        let self_for_block: Retained<NavDelegate> = self
            .ivars()
            .self_ref
            .borrow()
            .as_ref()
            .expect("self_ref set during page capture")
            .clone();
        let block = RcBlock::new(move |data: *mut NSData, err: *mut NSError| {
            if !err.is_null() {
                let err_obj = unsafe { &*err };
                let msg = err_obj.localizedDescription().to_string();
                self_for_block.dispatch_result(Err(format!("createPDF page failed: {msg}")));
                return;
            }
            if data.is_null() {
                self_for_block.dispatch_result(Err("createPDF returned null data".into()));
                return;
            }
            let nsdata = unsafe { Retained::retain(data).expect("non-null data") };
            self_for_block.ivars().pages.borrow_mut().push(nsdata);
            self_for_block.ivars().current_page.set(i + 1);
            self_for_block.capture_next_page();
        });
        unsafe {
            let config = WKPDFConfiguration::new(self.mtm());
            config.setRect(rect);
            webview.createPDFWithConfiguration_completionHandler(Some(&config), &block);
        }
    }

    /// Step 3: merge per-page PDFs into one multi-page PDF + write to disk.
    fn finalize(&self) {
        let pieces: Vec<Retained<NSData>> = self.ivars().pages.borrow_mut().drain(..).collect();
        let output_path = self.ivars().output_path.clone();
        let merged = match merge_page_pdfs(&pieces) {
            Ok(d) => d,
            Err(e) => {
                self.dispatch_result(Err(e));
                return;
            }
        };
        let bytes: Vec<u8> = merged.to_vec();
        if let Err(e) = std::fs::write(&output_path, bytes) {
            self.dispatch_result(Err(format!("Failed to write PDF: {e}")));
            return;
        }
        if !Path::new(&output_path).exists() {
            self.dispatch_result(Err(format!(
                "PDF reportedly written but no file at {output_path}"
            )));
            return;
        }
        self.dispatch_result(Ok(()));
    }
}

/// Expand a captured single-page PDF's page boxes so the page becomes A4
/// with the original content offset by (MARGIN_H, MARGIN_V). Sets every
/// box (Media/Crop/Bleed/Trim/Art) — viewers may clip to CropBox.
fn expand_to_a4_with_margins(page: &PDFPage) {
    let new_box = NSRect::new(
        objc2_foundation::NSPoint::new(-MARGIN_H, -MARGIN_V),
        objc2_foundation::NSSize::new(A4_W, A4_H),
    );
    unsafe {
        page.setBounds_forBox(new_box, PDFDisplayBox::MediaBox);
        page.setBounds_forBox(new_box, PDFDisplayBox::CropBox);
        page.setBounds_forBox(new_box, PDFDisplayBox::BleedBox);
        page.setBounds_forBox(new_box, PDFDisplayBox::TrimBox);
        page.setBounds_forBox(new_box, PDFDisplayBox::ArtBox);
    }
}

/// Merge a sequence of single-page PDF datas into one multi-page PDF.
/// Each captured page (INNER_W × INNER_H) gets its boxes expanded to A4
/// with the content offset by the margin amounts.
fn merge_page_pdfs(pieces: &[Retained<NSData>]) -> Result<Retained<NSData>, String> {
    if pieces.is_empty() {
        return Err("no pages to merge".into());
    }
    let combined = unsafe { PDFDocument::new() };
    for (idx, piece) in pieces.iter().enumerate() {
        let alloc = PDFDocument::alloc();
        let single = unsafe { PDFDocument::initWithData(alloc, piece) }
            .ok_or_else(|| format!("PDFDocument::initWithData failed for page {idx}"))?;
        let page = unsafe { single.pageAtIndex(0) }
            .ok_or_else(|| format!("page {idx} missing in single-page PDF"))?;
        expand_to_a4_with_margins(&page);
        let insert_at = unsafe { combined.pageCount() };
        unsafe { combined.insertPage_atIndex(&page, insert_at) };
    }
    // Re-encode embedded images at screen resolution + JPEG. Shrinks PDFs
    // with many SVG/raster images dramatically. Vector content is unaffected.
    let data = unsafe {
        let yes_obj: Retained<NSNumber> = NSNumber::numberWithBool(true);
        let dict_cls = class!(NSMutableDictionary);
        let dict: *mut AnyObject = msg_send![dict_cls, dictionary];
        let key1: &NSString = PDFDocumentOptimizeImagesForScreenOption;
        let key2: &NSString = PDFDocumentSaveImagesAsJPEGOption;
        let _: () = msg_send![dict, setObject: &*yes_obj, forKey: key1];
        let _: () = msg_send![dict, setObject: &*yes_obj, forKey: key2];
        let dict_ref = &*(dict as *const NSDictionary);
        combined
            .dataRepresentationWithOptions(dict_ref)
            .or_else(|| combined.dataRepresentation())
    }
    .ok_or_else(|| "merged PDF dataRepresentation returned nil".to_string())?;
    Ok(data)
}

trait NSDataToVec {
    fn to_vec(&self) -> Vec<u8>;
}
impl NSDataToVec for Retained<NSData> {
    fn to_vec(&self) -> Vec<u8> {
        (**self).to_vec()
    }
}

/// Render `html` to a PDF at `output_path`, blocking until done. Must be
/// called on the macOS main thread (the CLI's `main` qualifies).
pub fn render_to_path(html: &str, output_path: &str) -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "render_to_path must run on the main thread".to_string())?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);

    let result: ResultCell = Rc::new(RefCell::new(None));
    let frame = NSRect::new(
        objc2_foundation::NSPoint::new(0.0, 0.0),
        objc2_foundation::NSSize::new(INNER_W, INNER_H),
    );

    unsafe {
        let config = WKWebViewConfiguration::new(mtm);
        let webview: Retained<WKWebView> = WKWebView::initWithFrame_configuration(
            WKWebView::alloc(mtm),
            frame,
            &config,
        );
        let delegate = NavDelegate::new(
            mtm,
            webview.clone(),
            output_path.to_string(),
            result.clone(),
            app.clone(),
        );
        let proto = ProtocolObject::from_ref(&*delegate);
        webview.setNavigationDelegate(Some(proto));

        let html_ns = NSString::from_str(html);
        // Base URL is irrelevant — images are already inlined as data: URLs
        // by the host-render pipeline before the request reaches us.
        let base_ns = NSString::from_str("file:///");
        let base_url_obj = NSURL::URLWithString(&base_ns);
        let _ = webview.loadHTMLString_baseURL(&html_ns, base_url_obj.as_deref());

        // The navigation delegate self-retains; drop our local refs.
        drop(webview);
        drop(delegate);
    }

    app.run();

    let outcome = result.borrow_mut().take();
    match outcome {
        Some(Ok(())) => Ok(()),
        Some(Err(e)) => Err(e),
        None => Err("PDF generation completed without setting a result".into()),
    }
}
