//! Prepared shared drawing output and platform print dialogs. No web host,
//! external document viewer, or second drawing model participates in printing.
use crate::native_viewport::interface_shell::NativeInterfaceHandle;
use bevy::window::RawHandleWrapper;
use std::{
    path::PathBuf,
    sync::{mpsc, Arc, Mutex, OnceLock},
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

pub(crate) struct Page {
    pub title: String,
    pub size_mm: [f64; 2],
    #[cfg(target_os = "windows")]
    pub tree: resvg::usvg::Tree,
    pub pdf: Vec<u8>,
}

impl Page {
    /// SVG came from drawing_export after its topology/annotation validation.
    /// Its root millimetre size owns page dimensions, exactly as in SVG export.
    pub(crate) fn prepare(title: String, svg: &str) -> Result<Self, String> {
        if svg.len() > 32 * 1024 * 1024 || !svg.starts_with("<svg ") {
            return Err("Drawing exceeds the native print preparation limit".into());
        }
        let title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .take(512)
            .collect();
        static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
        let fonts = FONTS.get_or_init(|| {
            Arc::new(limo_cad_occt::drawing_export::load_outline_fonts(Some(
                bevy::text::DEFAULT_FONT_DATA,
            )))
        });
        let svg = limo_cad_occt::drawing_export::resolve_svg_text(svg, fonts)?;
        let options = resvg::usvg::Options {
            fontdb: fonts.clone(),
            font_family: "Fira Mono".into(),
            resources_dir: None,
            image_href_resolver: resvg::usvg::ImageHrefResolver {
                resolve_data: Box::new(|_, _, _| None),
                resolve_string: Box::new(|_, _| None),
            },
            ..Default::default()
        };
        let tree = resvg::usvg::Tree::from_str(&svg, &options).map_err(|e| e.to_string())?;
        let size_mm = [
            f64::from(tree.size().width()) * 25.4 / 96.,
            f64::from(tree.size().height()) * 25.4 / 96.,
        ];
        if size_mm
            .iter()
            .any(|n| !n.is_finite() || *n < 1. || *n > 2000.)
        {
            return Err("Drawing paper size is outside the supported print range".into());
        }
        let pdf = svg2pdf::to_pdf(
            &tree,
            svg2pdf::ConversionOptions {
                embed_text: false,
                ..Default::default()
            },
            svg2pdf::PageOptions::default(),
        )
        .map_err(|e| e.to_string())?;
        if pdf.len() > 64 * 1024 * 1024 {
            return Err("Prepared drawing exceeds the print spool limit".into());
        }
        Ok(Self {
            title,
            size_mm,
            #[cfg(target_os = "windows")]
            tree,
            pdf,
        })
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Outcome {
    Cancelled,
    /// `pdf_path` is set when the drawing was written to a PDF instead of a
    /// physical printer. The CAD document is not part of this value.
    Submitted {
        pdf_path: Option<PathBuf>,
    },
    Failed(String),
}
pub(crate) struct Running {
    receive: Mutex<mpsc::Receiver<Outcome>>,
}
impl Running {
    pub fn poll(&self) -> Option<Outcome> {
        match self
            .receive
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .try_recv()
        {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Outcome::Failed(
                "The native print operation stopped without a result".into(),
            )),
        }
    }
}

pub(crate) fn start(
    parent: RawHandleWrapper,
    page: Page,
    wake: NativeInterfaceHandle,
) -> Result<Running, String> {
    let (send, receive) = mpsc::channel();
    let finished = move |result: Result<Outcome, String>| {
        let _ = send.send(result.unwrap_or_else(Outcome::Failed));
        wake.request_redraw();
    };
    #[cfg(target_os = "macos")]
    macos::start(parent, page, finished)?;
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    std::thread::Builder::new()
        .name("cad-native-print".into())
        .spawn(move || {
            #[cfg(target_os = "windows")]
            let result = windows::print(parent, page);
            #[cfg(target_os = "linux")]
            let result = linux::print(parent, page);
            finished(result);
        })
        .map_err(|e| format!("Could not start native printing: {e}"))?;
    Ok(Running {
        receive: Mutex::new(receive),
    })
}

pub(crate) fn retire() {
    #[cfg(target_os = "macos")]
    macos::retire();
}

#[cfg(target_os = "windows")]
pub(crate) fn is_pdf_printer(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && (name.to_ascii_lowercase().contains("pdf") || name.eq_ignore_ascii_case("Print to File"))
}

/// Keep a prepared PDF where a print test can read it. Callers own the file.
pub(crate) fn write_retained_pdf(page: &Page) -> Result<PathBuf, String> {
    let path = retained_pdf_destination()?;
    limo_cad_project_file::write_binary_file_new(&path, &page.pdf).map_err(|e| e.to_string())?;
    Ok(path)
}

fn retained_pdf_destination() -> Result<PathBuf, String> {
    let directory = std::env::temp_dir().join(format!("Limo-CAD-print-{}", uuid::Uuid::new_v4()));
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .map_err(|e| e.to_string())?;
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
    }
    Ok(directory.join("sheet.pdf"))
}

#[cfg(test)]
mod tests;
