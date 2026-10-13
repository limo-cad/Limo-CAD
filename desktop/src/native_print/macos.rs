//! PDFKit supplies the native print operation; its document-modal sheet keeps
//! the existing Bevy window as owner and never creates a web application.
use super::{Outcome, Page};
use bevy::window::RawHandleWrapper;
use objc2::{
    define_class, msg_send, rc::Retained, sel, AnyThread, DefinedClass, MainThreadMarker,
    MainThreadOnly,
};
use objc2_app_kit::{NSPaperOrientation, NSPrintInfo, NSPrintOperation, NSPrinter, NSView};
use objc2_foundation::{NSData, NSObject, NSObjectProtocol, NSSize, NSString};
use objc2_pdf_kit::{PDFDocument, PDFPrintScalingMode};
use raw_window_handle::RawWindowHandle;
use std::{cell::RefCell, ffi::c_void};

type Callback = Box<dyn FnOnce(Result<Outcome, String>) + Send>;
struct CompletionState {
    callback: RefCell<Option<Callback>>,
}
define_class!(


    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = CompletionState]
    struct Completion;
    unsafe impl NSObjectProtocol for Completion {}
    impl Completion {
        #[unsafe(method(printOperation:didRun:contextInfo:))]
        fn completed(&self, _operation: &NSPrintOperation, success: bool, _context: *mut c_void) {
            if let Some(done) = self.ivars().callback.borrow_mut().take() {
                done(if success {
                    Ok(Outcome::Submitted { pdf_path: None })
                } else {
                    Ok(Outcome::Cancelled)
                });
            }
        }
    }
);
struct Active {
    _parent: RawHandleWrapper,
    _document: Retained<PDFDocument>,
    _operation: Retained<NSPrintOperation>,
    _delegate: Retained<Completion>,
}
thread_local! { static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) }; }

pub(super) fn start(
    parent: RawHandleWrapper,
    page: Page,
    done: impl FnOnce(Result<Outcome, String>) + Send + 'static,
) -> Result<(), String> {
    let mtm =
        MainThreadMarker::new().ok_or("Native printing must start on the AppKit main thread")?;
    if ACTIVE.with_borrow(|active| active.is_some()) {
        return Err("Finish the open print dialog first".into());
    }
    let RawWindowHandle::AppKit(raw) = parent.get_window_handle() else {
        return Err("The native print owner is not an AppKit window".into());
    };
    if NSPrinter::printerNames().count() == 0 {
        let pdf_path = super::write_retained_pdf(&page)?;
        done(Ok(Outcome::Submitted {
            pdf_path: Some(pdf_path),
        }));
        return Ok(());
    }
    unsafe {
        let view = &*raw.ns_view.as_ptr().cast::<NSView>();
        let window = view.window().ok_or("The print owner window was closed")?;
        let data = NSData::from_vec(page.pdf);
        let document = PDFDocument::initWithData(PDFDocument::alloc(), &data)
            .ok_or("PDFKit could not read the prepared drawing")?;
        let info = NSPrintInfo::new();
        info.setPaperSize(NSSize::new(
            page.size_mm[0].min(page.size_mm[1]) * 72. / 25.4,
            page.size_mm[0].max(page.size_mm[1]) * 72. / 25.4,
        ));
        info.setOrientation(if page.size_mm[0] > page.size_mm[1] {
            NSPaperOrientation::Landscape
        } else {
            NSPaperOrientation::Portrait
        });
        info.setScalingFactor(1.);
        info.setTopMargin(0.);
        info.setBottomMargin(0.);
        info.setLeftMargin(0.);
        info.setRightMargin(0.);
        info.setHorizontallyCentered(false);
        info.setVerticallyCentered(false);
        let operation = document
            .printOperationForPrintInfo_scalingMode_autoRotate(
                Some(&info),
                PDFPrintScalingMode::PageScaleNone,
                false,
                mtm,
            )
            .ok_or("PDFKit could not prepare the native print operation")?;
        operation.setJobTitle(Some(&NSString::from_str(&page.title)));
        operation.setShowsPrintPanel(true);
        operation.setShowsProgressPanel(true);
        let allocated = Completion::alloc(mtm).set_ivars(CompletionState {
            callback: RefCell::new(Some(Box::new(done))),
        });
        let delegate: Retained<Completion> = msg_send![super(allocated), init];
        ACTIVE.with_borrow_mut(|active| {
            *active = Some(Active {
                _parent: parent,
                _document: document,
                _operation: operation.clone(),
                _delegate: delegate.clone(),
            })
        });
        operation.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
            &window,
            Some(&delegate),
            Some(sel!(printOperation:didRun:contextInfo:)),
            std::ptr::null_mut(),
        );
    }
    Ok(())
}

pub(super) fn retire() {
    if MainThreadMarker::new().is_some() {
        ACTIVE.with_borrow_mut(|active| {
            active.take();
        });
    }
}
