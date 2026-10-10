//! PrintDlgEx owns printer choice and confirmation; GDI receives bounded tiles
//! of the same prepared SVG only after the user chooses Print.
use super::{Outcome, Page};
use bevy::window::RawHandleWrapper;
use raw_window_handle::RawWindowHandle;
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{GlobalFree, HGLOBAL, HWND},
        Graphics::{
            Gdi::*,
            Printing::{EnumPrintersW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL},
        },
        Storage::Xps::{AbortDoc, EndDoc, EndPage, StartDocW, StartPage, DOCINFOW},
        System::{
            Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
            Memory::{GlobalLock, GlobalUnlock},
        },
        UI::Controls::Dialogs::*,
    },
};

struct Dialog(PRINTDLGEXW);
impl Drop for Dialog {
    fn drop(&mut self) {
        unsafe {
            if !self.0.hDC.is_invalid() {
                let _ = DeleteDC(self.0.hDC);
            }
            if !self.0.hDevMode.is_invalid() {
                let _ = GlobalFree(Some(self.0.hDevMode));
            }
            if !self.0.hDevNames.is_invalid() {
                let _ = GlobalFree(Some(self.0.hDevNames));
            }
        }
    }
}
struct Com;
impl Drop for Com {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

pub(super) fn print(parent: RawHandleWrapper, page: Page) -> Result<Outcome, String> {
    let RawWindowHandle::Win32(raw) = parent.get_window_handle() else {
        return Err("The native print owner is not a Windows window".into());
    };

    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }.map_err(|e| e.to_string())?;
    let _com = Com;
    if no_installed_printer() {
        return Ok(Outcome::Submitted {
            pdf_path: Some(super::write_retained_pdf(&page)?),
        });
    }
    let mut dialog = Dialog(PRINTDLGEXW {
        lStructSize: std::mem::size_of::<PRINTDLGEXW>() as u32,
        hwndOwner: HWND(raw.hwnd.get() as *mut _),
        Flags: PD_RETURNDEFAULT,
        nMinPage: 1,
        nMaxPage: 1,
        nCopies: 1,
        nStartPage: START_PAGE_GENERAL,
        ..Default::default()
    });
    unsafe {
        if PrintDlgExW(&mut dialog.0).is_ok() && !dialog.0.hDevMode.is_invalid() {
            let mode = GlobalLock(dialog.0.hDevMode).cast::<DEVMODEW>();
            if !mode.is_null() {
                let landscape = page.size_mm[0] > page.size_mm[1];
                (*mode).dmFields |=
                    DM_PAPERWIDTH | DM_PAPERLENGTH | DM_ORIENTATION | DM_PAPERSIZE | DM_SCALE;
                let paper = &mut (*mode).Anonymous1.Anonymous1;
                paper.dmOrientation = if landscape {
                    DMORIENT_LANDSCAPE
                } else {
                    DMORIENT_PORTRAIT
                } as i16;
                paper.dmPaperSize = DMPAPER_USER as i16;
                paper.dmScale = 100;
                paper.dmPaperWidth = (page.size_mm[0].min(page.size_mm[1]) * 10.).round() as i16;
                paper.dmPaperLength = (page.size_mm[0].max(page.size_mm[1]) * 10.).round() as i16;
                let _ = GlobalUnlock(dialog.0.hDevMode);
            }
        }
        dialog.0.Flags = PD_RETURNDC
            | PD_NOSELECTION
            | PD_NOPAGENUMS
            | PD_NOCURRENTPAGE
            | PD_USEDEVMODECOPIESANDCOLLATE;
        PrintDlgExW(&mut dialog.0).map_err(|e| format!("Could not open the print dialog: {e}"))?;
        if dialog.0.dwResultAction != PD_RESULT_PRINT {
            return Ok(Outcome::Cancelled);
        }
        if super::is_pdf_printer(&selected_device_name(dialog.0.hDevNames)) {
            return Ok(Outcome::Submitted {
                pdf_path: Some(super::write_retained_pdf(&page)?),
            });
        }
        if dialog.0.hDC.is_invalid() {
            return Err("The selected printer returned no print context".into());
        }
        submit(dialog.0.hDC, &page)?;
    }
    Ok(Outcome::Submitted { pdf_path: None })
}

fn no_installed_printer() -> bool {
    unsafe {
        let mut needed = 0u32;
        let mut returned = 0u32;
        match EnumPrintersW(
            PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS,
            PCWSTR::null(),
            4,
            None,
            &mut needed,
            &mut returned,
        ) {
            Ok(()) => returned == 0,
            Err(error)
                if needed == 0 && returned == 0 && error.code().0 == 0x8007_007A_u32 as i32 =>
            {
                true
            }
            Err(_) => false,
        }
    }
}

unsafe fn selected_device_name(devnames: HGLOBAL) -> String {
    if devnames.is_invalid() {
        return String::new();
    }
    let locked = GlobalLock(devnames);
    if locked.is_null() {
        return String::new();
    }
    let names = locked.cast::<DEVNAMES>();
    let offset = usize::from((*names).wDeviceOffset);
    if offset > 4096 {
        let _ = GlobalUnlock(devnames);
        return String::new();
    }
    let mut wide = locked.cast::<u16>().add(offset);
    let mut units = Vec::new();
    while *wide != 0 && units.len() < 512 {
        units.push(*wide);
        wide = wide.add(1);
    }
    let _ = GlobalUnlock(devnames);
    String::from_utf16_lossy(&units)
}

unsafe fn submit(dc: HDC, page: &Page) -> Result<(), String> {
    let dpi = [
        GetDeviceCaps(Some(dc), LOGPIXELSX),
        GetDeviceCaps(Some(dc), LOGPIXELSY),
    ];
    let paper = [
        GetDeviceCaps(Some(dc), PHYSICALWIDTH),
        GetDeviceCaps(Some(dc), PHYSICALHEIGHT),
    ];
    if dpi.iter().any(|v| *v <= 0) || paper.iter().any(|v| *v <= 0) {
        return Err("The printer returned invalid page dimensions".into());
    }
    for axis in 0..2 {
        if f64::from(paper[axis]) * 25.4 / f64::from(dpi[axis]) + 1. < page.size_mm[axis] {
            return Err("The selected paper is smaller than this drawing. Choose matching paper to print at 1:1.".into());
        }
    }
    let name: Vec<u16> = page.title.encode_utf16().chain(Some(0)).collect();
    let info = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: PCWSTR(name.as_ptr()),
        ..Default::default()
    };
    if StartDocW(dc, &info) <= 0 {
        return Err("The printer could not start the confirmed job".into());
    }
    let result = (|| {
        if StartPage(dc) <= 0 {
            return Err("The printer could not start the drawing page".into());
        }
        raster_tiles(dc, page, dpi)?;
        if EndPage(dc) <= 0 {
            return Err("The printer could not finish the drawing page".into());
        }
        if EndDoc(dc) <= 0 {
            return Err("The print spooler did not accept the drawing".into());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = AbortDoc(dc);
    }
    result
}

unsafe fn raster_tiles(dc: HDC, page: &Page, dpi: [i32; 2]) -> Result<(), String> {
    let sample = [dpi[0].min(600), dpi[1].min(600)];
    let pixels = [
        (page.size_mm[0] * f64::from(sample[0]) / 25.4).ceil() as u32,
        (page.size_mm[1] * f64::from(sample[1]) / 25.4).ceil() as u32,
    ];
    let offset = [
        GetDeviceCaps(Some(dc), PHYSICALOFFSETX),
        GetDeviceCaps(Some(dc), PHYSICALOFFSETY),
    ];
    let drawable = [
        GetDeviceCaps(Some(dc), HORZRES),
        GetDeviceCaps(Some(dc), VERTRES),
    ];
    let coordinate = |v: u32, axis: usize| {
        (f64::from(v) * f64::from(dpi[axis]) / f64::from(sample[axis])).round() as i32
            - offset[axis]
    };
    for y in (0..pixels[1]).step_by(1024) {
        for x in (0..pixels[0]).step_by(1024) {
            let width = (pixels[0] - x).min(1024);
            let height = (pixels[1] - y).min(1024);
            let (left, top, right, bottom) = (
                coordinate(x, 0),
                coordinate(y, 1),
                coordinate(x + width, 0),
                coordinate(y + height, 1),
            );
            if right <= 0 || bottom <= 0 || left >= drawable[0] || top >= drawable[1] {
                continue;
            }
            let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
                .ok_or("Could not allocate a print tile")?;
            pixmap.fill(resvg::tiny_skia::Color::WHITE);
            resvg::render(
                &page.tree,
                resvg::tiny_skia::Transform::from_row(
                    sample[0] as f32 / 96.,
                    0.,
                    0.,
                    sample[1] as f32 / 96.,
                    -(x as f32),
                    -(y as f32),
                ),
                &mut pixmap.as_mut(),
            );
            for rgba in pixmap.data_mut().as_chunks_mut::<4>().0 {
                rgba.swap(0, 2);
            }
            let bitmap = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width as i32,
                    biHeight: -(height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            if StretchDIBits(
                dc,
                left,
                top,
                right - left,
                bottom - top,
                0,
                0,
                width as i32,
                height as i32,
                Some(pixmap.data().as_ptr().cast()),
                &bitmap,
                DIB_RGB_COLORS,
                SRCCOPY,
            ) <= 0
            {
                return Err("The printer rejected drawing graphics".into());
            }
        }
    }
    Ok(())
}
