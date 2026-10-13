//! The desktop Print portal owns both the dialog and confirmed submission.
use super::{Outcome, Page};
use ashpd::desktop::{
    print::{
        Orientation, OutputFileFormat, PageSetup, PreparePrintOptions, PrintOptions, PrintProxy,
        Settings,
    },
    ResponseError,
};
use bevy::window::RawHandleWrapper;
use std::{fs::File, os::unix::fs::DirBuilderExt, path::PathBuf};

struct SpoolFile {
    directory: PathBuf,
    path: PathBuf,
}
impl Drop for SpoolFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}
pub(super) fn print(parent: RawHandleWrapper, page: Page) -> Result<Outcome, String> {
    if cups_has_printer() == Some(false) {
        return Ok(Outcome::Submitted {
            pdf_path: Some(super::write_retained_pdf(&page)?),
        });
    }
    let directory = std::env::temp_dir().join(format!("Limo-CAD-print-{}", uuid::Uuid::new_v4()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&directory)
        .map_err(|e| e.to_string())?;
    let file = SpoolFile {
        path: directory.join("sheet.pdf"),
        directory,
    };
    limo_cad_project_file::write_binary_file_new(&file.path, &page.pdf)
        .map_err(|e| e.to_string())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let result = runtime.block_on(async {
        let window = ashpd::WindowIdentifier::from_raw_handle(
            &parent.get_window_handle(),
            Some(&parent.get_display_handle()),
        )
        .await;
        let proxy = PrintProxy::new().await?;
        let orientation = if page.size_mm[0] > page.size_mm[1] {
            Orientation::Landscape
        } else {
            Orientation::Portrait
        };
        let setup = PageSetup::default()
            .set_width(page.size_mm[0].min(page.size_mm[1]))
            .set_height(page.size_mm[0].max(page.size_mm[1]))
            .set_orientation(orientation)
            .set_margin_top(0.)
            .set_margin_bottom(0.)
            .set_margin_left(0.)
            .set_margin_right(0.);
        let prepared = proxy
            .prepare_print(
                window.as_ref(),
                &page.title,
                Settings::default()
                    .set_orientation(orientation)
                    .set_scale(100),
                setup,
                PreparePrintOptions::default()
                    .set_modal(true)
                    .set_supported_output_file_formats([OutputFileFormat::Pdf]),
            )
            .await?
            .response()?;
        let input = File::open(&file.path)?;
        proxy
            .print(
                window.as_ref(),
                &page.title,
                &input,
                PrintOptions::default()
                    .set_token(prepared.token)
                    .set_modal(true)
                    .set_supported_output_file_formats([OutputFileFormat::Pdf]),
            )
            .await?
            .response()?;
        Ok::<_, ashpd::Error>(Outcome::Submitted { pdf_path: None })
    });
    match result {
        Ok(outcome) => Ok(outcome),
        Err(ashpd::Error::Response(ResponseError::Cancelled)) => Ok(Outcome::Cancelled),
        Err(error) if portal_unavailable(&error) => Ok(Outcome::Submitted {
            pdf_path: Some(super::write_retained_pdf(&page)?),
        }),
        Err(error) => Err(format!("Native print portal: {error}")),
    }
}

fn cups_has_printer() -> Option<bool> {
    let output = std::process::Command::new("lpstat")
        .arg("-p")
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    Some(text.lines().any(|line| line.starts_with("printer ")))
}

fn portal_unavailable(error: &ashpd::Error) -> bool {
    matches!(
        error,
        ashpd::Error::PortalNotFound(_) | ashpd::Error::Zbus(_) | ashpd::Error::NoResponse
    )
}
