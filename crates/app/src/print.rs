//! Printing a tab's text with the system's print dialog.
//!
//! GPUI has no printing, so each platform has its own:
//!
//! - macOS: AppKit's print panel, as a sheet on the window, printing an
//!   NSTextView (with Preview and Save as PDF).
//! - Windows: a preview of the pages (`preview.rs`), whose Print button
//!   opens the Print dialog; then GDI pages, laid out like the preview, with
//!   the file name and page number on each. The dialog runs on a thread of
//!   its own, since it has its own message loop.
//! - Linux and the BSDs: the default printer through CUPS' `lp`, after a
//!   confirmation, without colors.
//!
//! Colors come from the light syntax theme: paper is white whatever the
//! window looks like.

// The page layout is used by the Windows printing and preview only.
#![cfg_attr(not(target_os = "windows"), allow(dead_code))]

mod preview;

use std::ops::Range;

use anyhow::Result;
use gpui_kit::*;

/// What to print.
pub struct PrintJob {
    /// The document's name, for the print queue and the page headers.
    pub title: String,
    pub text: String,
    /// Colored byte ranges of `text`, in order.
    pub colors: Vec<(Range<usize>, Rgba)>,
}

/// Point size of the printed text.
const FONT_SIZE: f64 = 9.;
/// Columns of a tab stop.
const TAB_WIDTH: usize = 4;

/// A page's measurements, in points (1/72 inch), and those of the text on it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageSetup {
    /// The printable area.
    pub width: f32,
    pub height: f32,
    /// Between the printable area and the text.
    pub margin_x: f32,
    pub margin_y: f32,
    /// The monospaced font's advance and line height.
    pub char_width: f32,
    pub line_height: f32,
}

impl PageSetup {
    /// An A4 page with Consolas' proportions, when no printer can tell.
    pub fn fallback() -> Self {
        let size = FONT_SIZE as f32;
        Self { width: 595., height: 842., margin_x: 36., margin_y: 36., char_width: size * 0.55, line_height: size * 1.17 }
    }

    /// Characters on a line.
    pub fn columns(&self) -> usize {
        // The small allowance keeps rounding from losing a column the
        // device-unit arithmetic would have.
        (((self.width - 2. * self.margin_x) / self.char_width + 1e-3).floor() as usize).max(20)
    }

    /// Text lines on a page, below the header line and a blank one.
    pub fn lines_per_page(&self) -> usize {
        (((self.height - 2. * self.margin_y) / self.line_height + 1e-3).floor() as usize).saturating_sub(2).max(1)
    }
}

/// A piece of a printed line: its first column, its color (black when
/// `None`) and its text.
pub type Piece = (usize, Option<Rgba>, String);

/// The text cut into printed lines of at most `columns` characters, tabs
/// expanded.
pub fn layout(job: &PrintJob, columns: usize) -> Vec<Vec<Piece>> {
    let mut lines = vec![Vec::new()];
    let mut column = 0;
    let mut runs = job.colors.iter().peekable();
    let push = |lines: &mut Vec<Vec<Piece>>, column: usize, color: Option<Rgba>, text: &str| {
        let line = lines.last_mut().expect("never empty");
        match line.last_mut() {
            Some((start, last_color, last)) if *last_color == color && *start + last.chars().count() == column => {
                last.push_str(text);
            }
            _ => line.push((column, color, text.to_owned())),
        }
    };
    for (at, ch) in job.text.char_indices() {
        while runs.peek().is_some_and(|(range, _)| range.end <= at) {
            runs.next();
        }
        let color = match runs.peek() {
            Some((range, rgba)) if range.start <= at => Some(*rgba),
            _ => None,
        };
        match ch {
            '\n' => {
                lines.push(Vec::new());
                column = 0;
            }
            '\r' => {}
            '\t' => column += TAB_WIDTH - column % TAB_WIDTH,
            _ => {
                if column >= columns {
                    lines.push(Vec::new());
                    column = 0;
                }
                let mut buffer = [0; 4];
                push(&mut lines, column, color, ch.encode_utf8(&mut buffer));
                column += 1;
            }
        }
    }
    lines
}

/// The window's native handle.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn window_handle(window: &Window) -> Result<raw_window_handle::WindowHandle<'_>> {
    use raw_window_handle::HasWindowHandle;
    HasWindowHandle::window_handle(window).map_err(|err| anyhow::anyhow!("no native window: {err:?}"))
}

/// Shows the print dialog for `job`, attached to `window`.
pub fn print(job: PrintJob, window: &mut Window, cx: &mut App) -> Result<()> {
    platform::print(job, window, cx)
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{cell::RefCell, ptr};

    use anyhow::{Context as _, Result, bail};
    use gpui_kit::{App, Window};
    use objc2::{AnyThread, MainThreadMarker, MainThreadOnly, rc::Retained};
    use objc2_app_kit::{
        NSColor, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSPrintInfo, NSPrintOperation,
        NSPrintingPaginationMode, NSTextView, NSView,
    };
    use objc2_foundation::{NSMutableAttributedString, NSPoint, NSRange, NSRect, NSSize, NSString};
    use raw_window_handle::RawWindowHandle;

    use super::{FONT_SIZE, PrintJob, window_handle};

    thread_local! {
        /// The last print operation and its view, kept alive while its sheet is up.
        static RUNNING: RefCell<Option<(Retained<NSPrintOperation>, Retained<NSTextView>)>> =
            const { RefCell::new(None) };
    }

    pub fn print(job: PrintJob, window: &mut Window, _: &mut App) -> Result<()> {
        let mtm = MainThreadMarker::new().context("printing must start on the main thread")?;
        let RawWindowHandle::AppKit(handle) = window_handle(window)?.as_raw() else {
            bail!("not an AppKit window");
        };
        // SAFETY: GPUI's window handle points to its live NSView.
        let view: &NSView = unsafe { handle.ns_view.cast().as_ref() };
        let ns_window = view.window().context("the view has no window")?;

        let print_info = NSPrintInfo::sharedPrintInfo();
        print_info.setHorizontalPagination(NSPrintingPaginationMode::Fit);
        print_info.setHorizontallyCentered(false);
        print_info.setVerticallyCentered(false);
        let width = print_info.paperSize().width - print_info.leftMargin() - print_info.rightMargin();

        let text_view = NSTextView::initWithFrame(
            NSTextView::alloc(mtm),
            NSRect::new(NSPoint::new(0., 0.), NSSize::new(width, 1.)),
        );
        text_view.setVerticallyResizable(true);
        text_view.setHorizontallyResizable(false);
        text_view.setMaxSize(NSSize::new(width, f64::MAX));
        // SAFETY: a new text view always has its text storage.
        let storage = unsafe { text_view.textStorage() }.context("no text storage")?;
        storage.setAttributedString(&attributed(&job));
        text_view.sizeToFit();

        let operation = NSPrintOperation::printOperationWithView_printInfo(&text_view, &print_info);
        operation.setJobTitle(Some(&NSString::from_str(&job.title)));
        operation.setShowsPrintPanel(true);
        operation.setShowsProgressPanel(true);
        // A sheet: returns at once, unlike `runOperation`, whose modal loop
        // would run GPUI's event handling inside this call.
        // SAFETY: no delegate, so no selector or context.
        unsafe {
            operation.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
                &ns_window,
                None,
                None,
                ptr::null_mut(),
            );
        }
        RUNNING.with(|running| *running.borrow_mut() = Some((operation, text_view)));
        Ok(())
    }

    /// The text in a monospaced font, with its colors.
    fn attributed(job: &PrintJob) -> Retained<NSMutableAttributedString> {
        let string = NSMutableAttributedString::initWithString(
            NSMutableAttributedString::alloc(),
            &NSString::from_str(&job.text),
        );
        let font = NSFont::fontWithName_size(&NSString::from_str("Menlo"), FONT_SIZE)
            .or_else(|| NSFont::userFixedPitchFontOfSize(FONT_SIZE));
        let utf16_len = job.text.encode_utf16().count();
        if let Some(font) = font {
            // SAFETY: a font is the value NSFontAttributeName takes.
            unsafe { string.addAttribute_value_range(NSFontAttributeName, &font, NSRange::new(0, utf16_len)) };
        }

        // Byte offsets to UTF-16 offsets, walking the text once.
        let mut chars = job.text.char_indices().peekable();
        let mut utf16 = 0;
        let mut to_utf16 = |byte: usize| {
            while let Some(&(at, ch)) = chars.peek() {
                if at >= byte {
                    break;
                }
                utf16 += ch.len_utf16();
                chars.next();
            }
            utf16
        };
        for (range, rgba) in &job.colors {
            let (start, end) = (to_utf16(range.start), to_utf16(range.end));
            let color =
                NSColor::colorWithSRGBRed_green_blue_alpha(rgba.r.into(), rgba.g.into(), rgba.b.into(), 1.);
            // SAFETY: a color is the value NSForegroundColorAttributeName takes.
            unsafe {
                string.addAttribute_value_range(NSForegroundColorAttributeName, &color, NSRange::new(start, end - start));
            }
        }
        string
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{mem, ptr};

    use anyhow::{Result, bail};
    use gpui_kit::{App, Window};
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HWND},
        Graphics::Gdi::{
            CLIP_DEFAULT_PRECIS, CreateFontW, DEFAULT_CHARSET, DEFAULT_QUALITY, DeleteDC, DeleteObject, FF_MODERN,
            FIXED_PITCH, FW_NORMAL, GetDeviceCaps, GetTextMetricsW, HDC, HFONT, HGDIOBJ, HORZRES, LOGPIXELSX,
            LOGPIXELSY, OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT,
            TextOutW, VERTRES,
        },
        Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage},
        UI::Controls::Dialogs::{
            PD_NOPAGENUMS, PD_NOSELECTION, PD_RETURNDC, PD_RETURNDEFAULT, PD_RETURNIC, PD_USEDEVMODECOPIESANDCOLLATE,
            PRINTDLGW, PrintDlgW,
        },
    };

    use super::{FONT_SIZE, PageSetup, PrintJob, layout, window_handle};

    /// Black, for text without a color.
    const BLACK: u32 = 0;

    /// Shows the preview, laid out for the default printer; its Print
    /// button opens the Print dialog.
    pub fn print(job: PrintJob, window: &mut Window, cx: &mut App) -> Result<()> {
        let RawWindowHandle::Win32(handle) = window_handle(window)?.as_raw() else {
            bail!("not a Win32 window");
        };
        let owner = handle.hwnd.get();
        // SAFETY: the information context is created and freed here.
        let setup = unsafe { default_setup() }.unwrap_or_else(PageSetup::fallback);
        super::preview::open(job, setup, move |job| show_dialog(job, owner), cx);
        Ok(())
    }

    /// The Print dialog, owned by the editor window, then the pages. On a
    /// thread of its own: the dialog runs a message loop.
    fn show_dialog(job: PrintJob, owner: isize) {
        std::thread::spawn(move || {
            // SAFETY: the window outlives the dialog it owns, or the dialog
            // closes with it; everything else is created and freed here.
            unsafe { print_on_this_thread(job, owner as HWND) };
        });
    }

    /// The default printer's page, measured without asking anything.
    unsafe fn default_setup() -> Option<PageSetup> {
        let mut dialog: PRINTDLGW = unsafe { mem::zeroed() };
        dialog.lStructSize = mem::size_of::<PRINTDLGW>() as u32;
        dialog.Flags = PD_RETURNDEFAULT | PD_RETURNIC;
        if unsafe { PrintDlgW(&mut dialog) } == 0 || dialog.hDC.is_null() {
            return None; // No printer.
        }
        let hdc = dialog.hDC;
        let font = unsafe { Font::select(hdc) };
        let setup = font.setup;
        unsafe {
            font.release(hdc);
            DeleteDC(hdc);
            free_dialog(&dialog);
        }
        Some(setup)
    }

    unsafe fn free_dialog(dialog: &PRINTDLGW) {
        unsafe {
            if !dialog.hDevMode.is_null() {
                GlobalFree(dialog.hDevMode);
            }
            if !dialog.hDevNames.is_null() {
                GlobalFree(dialog.hDevNames);
            }
        }
    }

    unsafe fn print_on_this_thread(job: PrintJob, owner: HWND) {
        let mut dialog: PRINTDLGW = unsafe { mem::zeroed() };
        dialog.lStructSize = mem::size_of::<PRINTDLGW>() as u32;
        dialog.hwndOwner = owner;
        dialog.Flags = PD_RETURNDC | PD_NOSELECTION | PD_NOPAGENUMS | PD_USEDEVMODECOPIESANDCOLLATE;
        if unsafe { PrintDlgW(&mut dialog) } == 0 || dialog.hDC.is_null() {
            return; // Canceled.
        }
        let hdc = dialog.hDC;
        unsafe {
            print_pages(&job, hdc);
            DeleteDC(hdc);
            free_dialog(&dialog);
        }
    }

    /// The printing font, selected into a printer's device context, and the
    /// page measured with it.
    struct Font {
        font: HFONT,
        previous: HGDIOBJ,
        setup: PageSetup,
        /// Device units per point, across and down.
        scale_x: f32,
        scale_y: f32,
    }

    impl Font {
        unsafe fn select(hdc: HDC) -> Self {
            let caps = |index: u32| unsafe { GetDeviceCaps(hdc, index as i32) };
            let (dpi_x, dpi_y) = (caps(LOGPIXELSX).max(1), caps(LOGPIXELSY).max(1));
            let face: Vec<u16> = "Consolas\0".encode_utf16().collect();
            let height = -((FONT_SIZE * f64::from(dpi_y) / 72.).round() as i32);
            let font = unsafe {
                CreateFontW(
                    height,
                    0,
                    0,
                    0,
                    FW_NORMAL as i32,
                    0,
                    0,
                    0,
                    DEFAULT_CHARSET as u32,
                    OUT_DEFAULT_PRECIS as u32,
                    CLIP_DEFAULT_PRECIS as u32,
                    DEFAULT_QUALITY as u32,
                    (FIXED_PITCH | FF_MODERN) as u32,
                    face.as_ptr(),
                )
            };
            let previous = unsafe { SelectObject(hdc, font) };
            let mut metrics: TEXTMETRICW = unsafe { mem::zeroed() };
            unsafe { GetTextMetricsW(hdc, &mut metrics) };
            let (scale_x, scale_y) = (dpi_x as f32 / 72., dpi_y as f32 / 72.);
            let setup = PageSetup {
                width: caps(HORZRES) as f32 / scale_x,
                height: caps(VERTRES) as f32 / scale_y,
                // Half an inch inside the printable area.
                margin_x: 36.,
                margin_y: 36.,
                char_width: metrics.tmAveCharWidth.max(1) as f32 / scale_x,
                line_height: (metrics.tmHeight + metrics.tmExternalLeading).max(1) as f32 / scale_y,
            };
            Self { font, previous, setup, scale_x, scale_y }
        }

        unsafe fn release(self, hdc: HDC) {
            unsafe {
                SelectObject(hdc, self.previous);
                DeleteObject(self.font);
            }
        }
    }

    unsafe fn print_pages(job: &PrintJob, hdc: HDC) {
        let font = unsafe { Font::select(hdc) };
        unsafe { SetBkMode(hdc, TRANSPARENT as i32) };
        let setup = font.setup;
        let x = |points: f32| (points * font.scale_x).round() as i32;
        let y = |points: f32| (points * font.scale_y).round() as i32;
        // The same layout as the preview's.
        let lines = layout(job, setup.columns());
        let per_page = setup.lines_per_page();
        let pages = lines.len().div_ceil(per_page).max(1);

        let title: Vec<u16> = job.title.encode_utf16().chain(Some(0)).collect();
        let info = DOCINFOW {
            cbSize: mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: title.as_ptr(),
            lpszOutput: ptr::null(),
            lpszDatatype: ptr::null(),
            fwType: 0,
        };
        if unsafe { StartDocW(hdc, &info) } <= 0 {
            unsafe { font.release(hdc) };
            return;
        }
        for page in 0..pages {
            if unsafe { StartPage(hdc) } <= 0 {
                unsafe { AbortDoc(hdc) };
                break;
            }
            // Header: the document's name on the left, the page on the right.
            let number = format!("{} / {pages}", page + 1);
            unsafe {
                SetTextColor(hdc, BLACK);
                text_out(hdc, x(setup.margin_x), y(setup.margin_y), &job.title);
                let right = setup.width - setup.margin_x - setup.char_width * number.chars().count() as f32;
                text_out(hdc, x(right), y(setup.margin_y), &number);
            }
            for (row, line) in lines.iter().skip(page * per_page).take(per_page).enumerate() {
                let top = y(setup.margin_y + setup.line_height * (row as f32 + 2.));
                for (column, color, text) in line {
                    unsafe {
                        SetTextColor(hdc, color.as_ref().map_or(BLACK, colorref));
                        text_out(hdc, x(setup.margin_x + setup.char_width * *column as f32), top, text);
                    }
                }
            }
            if unsafe { EndPage(hdc) } <= 0 {
                unsafe { AbortDoc(hdc) };
                break;
            }
        }
        unsafe {
            EndDoc(hdc);
            font.release(hdc);
        }
    }

    unsafe fn text_out(hdc: HDC, x: i32, y: i32, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe { TextOutW(hdc, x, y, wide.as_ptr(), wide.len() as i32) };
    }

    fn colorref(rgba: &gpui_kit::Rgba) -> u32 {
        let channel = |value: f32| (value.clamp(0., 1.) * 255.).round() as u32;
        channel(rgba.r) | channel(rgba.g) << 8 | channel(rgba.b) << 16
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    use anyhow::Result;
    use gpui_kit::{App, AppContext as _, PromptButton, PromptLevel, Window};

    use super::PrintJob;

    pub fn print(job: PrintJob, window: &mut Window, cx: &mut App) -> Result<()> {
        let message = t!("print.confirm", name = job.title).to_string();
        let answer = window.prompt(
            PromptLevel::Info,
            &message,
            Some(&t!("print.confirm_detail")),
            &[PromptButton::new(t!("print.print")), PromptButton::cancel(t!("dialog.cancel"))],
            cx,
        );
        cx.background_spawn(async move {
            if answer.await == Ok(0)
                && let Err(err) = lp(&job)
            {
                log::error!("printing failed: {err:#}");
            }
        })
        .detach();
        Ok(())
    }

    fn lp(job: &PrintJob) -> Result<()> {
        let mut child = Command::new("lp").arg("-t").arg(&job.title).stdin(Stdio::piped()).spawn()?;
        child.stdin.take().expect("piped").write_all(job.text.as_bytes())?;
        let status = child.wait()?;
        anyhow::ensure!(status.success(), "lp exited with {status}");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui_kit's `test` attribute would replace the standard one.
    use gpui_kit::Rgba;

    use super::{PageSetup, PrintJob, layout};

    fn job(text: &str) -> PrintJob {
        PrintJob { title: "a.txt".into(), text: text.into(), colors: Vec::new() }
    }

    #[test]
    fn long_lines_wrap_and_tabs_expand() {
        let lines = layout(&job("abcdefghij\n\tx"), 8);
        let texts: Vec<Vec<(usize, &str)>> =
            lines.iter().map(|line| line.iter().map(|(column, _, text)| (*column, text.as_str())).collect()).collect();
        assert_eq!(texts, vec![vec![(0, "abcdefgh")], vec![(0, "ij")], vec![(4, "x")]]);
    }

    #[test]
    fn colors_follow_their_ranges() {
        let red = Rgba { r: 1., g: 0., b: 0., a: 1. };
        let job = PrintJob { title: String::new(), text: "fn main".into(), colors: vec![(0..2, red)] };
        let lines = layout(&job, 80);
        assert_eq!(lines[0], vec![(0, Some(red), "fn".to_owned()), (2, None, " main".to_owned())]);
    }

    #[test]
    fn a4_fits_eighty_columns() {
        let setup = PageSetup::fallback();
        assert!(setup.columns() >= 80, "{}", setup.columns());
        assert!(setup.lines_per_page() > 50, "{}", setup.lines_per_page());
    }
}
