//! Printing a tab's text with the system's print dialog.
//!
//! GPUI has no printing, so each platform has its own:
//!
//! - macOS: AppKit's print panel, as a sheet on the window, printing an
//!   NSTextView (with Preview and Save as PDF).
//! - Windows: the Print dialog, then GDI pages laid out here, with the file
//!   name and page number on each. Runs on a thread of its own, since the
//!   dialog has its own message loop.
//! - Linux and the BSDs: the default printer through CUPS' `lp`, after a
//!   confirmation, without colors.
//!
//! Colors come from the light syntax theme: paper is white whatever the
//! window looks like.

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
            FIXED_PITCH, FW_NORMAL, GetDeviceCaps, GetTextMetricsW, HDC, HORZRES, LOGPIXELSX, LOGPIXELSY,
            OUT_DEFAULT_PRECIS, SelectObject, SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT, TextOutW, VERTRES,
        },
        Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage},
        UI::Controls::Dialogs::{
            PD_NOPAGENUMS, PD_NOSELECTION, PD_RETURNDC, PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGW, PrintDlgW,
        },
    };

    use super::{FONT_SIZE, PrintJob, window_handle};

    /// Columns of a tab stop.
    const TAB_WIDTH: usize = 4;
    /// Black, for text without a color.
    const BLACK: u32 = 0;

    pub fn print(job: PrintJob, window: &mut Window, _: &mut App) -> Result<()> {
        let RawWindowHandle::Win32(handle) = window_handle(window)?.as_raw() else {
            bail!("not a Win32 window");
        };
        let owner = handle.hwnd.get();
        // The dialog runs a message loop of its own: keep it off GPUI's thread.
        std::thread::spawn(move || {
            // SAFETY: the window outlives the dialog it owns, or the dialog
            // closes with it; everything else is created and freed here.
            unsafe { print_on_this_thread(job, owner as HWND) };
        });
        Ok(())
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
            if !dialog.hDevMode.is_null() {
                GlobalFree(dialog.hDevMode);
            }
            if !dialog.hDevNames.is_null() {
                GlobalFree(dialog.hDevNames);
            }
        }
    }

    unsafe fn print_pages(job: &PrintJob, hdc: HDC) {
        let (dpi_x, dpi_y) = unsafe { (GetDeviceCaps(hdc, LOGPIXELSX), GetDeviceCaps(hdc, LOGPIXELSY)) };
        let (page_width, page_height) = unsafe { (GetDeviceCaps(hdc, HORZRES), GetDeviceCaps(hdc, VERTRES)) };
        // Half an inch inside the printable area.
        let (margin_x, margin_y) = (dpi_x / 2, dpi_y / 2);

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
        let previous_font = unsafe { SelectObject(hdc, font) };
        let mut metrics: TEXTMETRICW = unsafe { mem::zeroed() };
        unsafe {
            GetTextMetricsW(hdc, &mut metrics);
            SetBkMode(hdc, TRANSPARENT as i32);
        }
        let line_height = (metrics.tmHeight + metrics.tmExternalLeading).max(1);
        let char_width = metrics.tmAveCharWidth.max(1);
        let columns = (((page_width - 2 * margin_x) / char_width).max(20)) as usize;
        // A header line and a blank one above the text.
        let lines_per_page = (((page_height - 2 * margin_y) / line_height) - 2).max(1) as usize;

        let lines = layout(job, columns);
        let pages = lines.len().div_ceil(lines_per_page).max(1);

        let title: Vec<u16> = job.title.encode_utf16().chain(Some(0)).collect();
        let info = DOCINFOW {
            cbSize: mem::size_of::<DOCINFOW>() as i32,
            lpszDocName: title.as_ptr(),
            lpszOutput: ptr::null(),
            lpszDatatype: ptr::null(),
            fwType: 0,
        };
        if unsafe { StartDocW(hdc, &info) } <= 0 {
            unsafe {
                SelectObject(hdc, previous_font);
                DeleteObject(font);
            }
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
                text_out(hdc, margin_x, margin_y, &job.title);
                let right = page_width - margin_x - char_width * number.chars().count() as i32;
                text_out(hdc, right, margin_y, &number);
            }
            let first = page * lines_per_page;
            for (row, line) in lines.iter().skip(first).take(lines_per_page).enumerate() {
                let y = margin_y + line_height * (row as i32 + 2);
                for (column, color, text) in line {
                    unsafe {
                        SetTextColor(hdc, *color);
                        text_out(hdc, margin_x + char_width * *column as i32, y, text);
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
            SelectObject(hdc, previous_font);
            DeleteObject(font);
        }
    }

    unsafe fn text_out(hdc: HDC, x: i32, y: i32, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe { TextOutW(hdc, x, y, wide.as_ptr(), wide.len() as i32) };
    }

    /// The text cut into printed lines of at most `columns` characters, tabs
    /// expanded; each line is a list of (column, color, text) pieces.
    fn layout(job: &PrintJob, columns: usize) -> Vec<Vec<(usize, u32, String)>> {
        let mut lines = vec![Vec::new()];
        let mut column = 0;
        let mut runs = job.colors.iter().peekable();
        let push = |lines: &mut Vec<Vec<(usize, u32, String)>>, column: usize, color: u32, text: &str| {
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
                Some((range, rgba)) if range.start <= at => colorref(rgba),
                _ => BLACK,
            };
            match ch {
                '\n' => {
                    lines.push(Vec::new());
                    column = 0;
                }
                '\r' => {}
                '\t' => {
                    let spaces = TAB_WIDTH - column % TAB_WIDTH;
                    column += spaces;
                }
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
                eprintln!("oxypad: printing failed: {err:#}");
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
