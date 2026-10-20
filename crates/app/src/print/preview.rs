//! The print preview: the pages as they will print, laid out like the
//! printed ones (`layout` and `PageSetup`), before the Print dialog.

use std::ops::Range;

use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
        h_flex,
        scroll::ScrollableElement,
        v_flex,
    },
    *,
};

use super::{FONT_SIZE, PageSetup, PrintJob, layout};

/// Room around each page, in pixels.
const GAP: f32 = 24.;

type PrintCallback = Box<dyn FnOnce(PrintJob)>;

/// Opens the preview of `job`; its Print button closes it and calls `print`.
pub fn open(job: PrintJob, setup: PageSetup, print: impl FnOnce(PrintJob) + 'static, cx: &mut App) {
    let title = t!("print.preview_title", name = job.title).to_string();
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions { title: Some(title.into()), ..Default::default() }),
        window_bounds: Some(WindowBounds::centered(size(px(760.), px(820.)), cx)),
        window_min_size: Some(size(px(420.), px(360.))),
        ..Default::default()
    };
    let opened = gpui_kit::open_window(options, cx, |window, cx| {
        let preview = cx.new(|cx| PrintPreview::new(job, setup, Box::new(print), cx));
        let focus_handle = preview.read(cx).focus_handle.clone();
        window.focus(&focus_handle, cx);
        preview
    });
    if let Err(err) = opened {
        log::error!("could not open the print preview: {err:#}");
    }
}

/// One printed line: its text, spaces where tabs were, and its colors.
type Line = (SharedString, Vec<(Range<usize>, HighlightStyle)>);

struct PrintPreview {
    job: Option<PrintJob>,
    print: Option<PrintCallback>,
    setup: PageSetup,
    title: SharedString,
    lines: Vec<Line>,
    per_page: usize,
    pages: usize,
    scroll: UniformListScrollHandle,
    focus_handle: FocusHandle,
}

impl PrintPreview {
    fn new(job: PrintJob, setup: PageSetup, print: PrintCallback, cx: &mut Context<Self>) -> Self {
        let lines: Vec<Line> = layout(&job, setup.columns()).into_iter().map(line).collect();
        let per_page = setup.lines_per_page();
        let pages = lines.len().div_ceil(per_page).max(1);
        Self {
            title: job.title.clone().into(),
            job: Some(job),
            print: Some(print),
            setup,
            lines,
            per_page,
            pages,
            scroll: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    fn print(&mut self, window: &mut Window) {
        if let (Some(job), Some(print)) = (self.job.take(), self.print.take()) {
            print(job);
        }
        window.remove_window();
    }

    /// Pixels per point: pages fit the window's width, within limits.
    fn scale(&self, window: &Window) -> f32 {
        let available = window.viewport_size().width.as_f32() - 2. * GAP;
        (available / self.setup.width).clamp(0.5, 1.6)
    }

    fn render_pages(&self, range: Range<usize>, window: &mut Window, cx: &App) -> Vec<AnyElement> {
        let scale = self.scale(window);
        let setup = self.setup;
        let pt = |points: f32| px(points * scale);
        let font = if cfg!(target_os = "windows") { "Consolas".into() } else { cx.theme().mono_font_family.clone() };
        range
            .map(|page| {
                let first = page * self.per_page;
                let lines = self.lines.iter().skip(first).take(self.per_page);
                let header = h_flex()
                    .h(pt(setup.line_height))
                    .justify_between()
                    .child(self.title.clone())
                    .child(format!("{} / {}", page + 1, self.pages));
                let body = v_flex()
                    .absolute()
                    .left(pt(setup.margin_x))
                    .top(pt(setup.margin_y))
                    .w(pt(setup.width - 2. * setup.margin_x))
                    .font_family(font.clone())
                    .text_size(pt(FONT_SIZE as f32))
                    .line_height(pt(setup.line_height))
                    .text_color(black())
                    .whitespace_nowrap()
                    .child(header)
                    .child(div().h(pt(setup.line_height)))
                    .children(lines.map(|(text, colors)| {
                        div().h(pt(setup.line_height)).child(StyledText::new(text.clone()).with_highlights(colors.clone()))
                    }));
                div()
                    .w_full()
                    .h(pt(setup.height) + px(GAP))
                    .flex()
                    .justify_center()
                    .pt(px(GAP))
                    .child(
                        div()
                            .relative()
                            .flex_shrink_0()
                            .w(pt(setup.width))
                            .h(pt(setup.height))
                            .bg(white())
                            .shadow_md()
                            .overflow_hidden()
                            .child(body),
                    )
                    .into_any_element()
            })
            .collect()
    }
}

/// A laid-out line as text with its colors, padding the gaps tabs left.
fn line(pieces: Vec<super::Piece>) -> Line {
    let mut text = String::new();
    let mut colors = Vec::new();
    let mut column = 0;
    for (start, color, piece) in pieces {
        text.extend(std::iter::repeat_n(' ', start.saturating_sub(column)));
        let begin = text.len();
        text.push_str(&piece);
        if let Some(color) = color {
            colors.push((begin..text.len(), HighlightStyle { color: Some(color.into()), ..Default::default() }));
        }
        column = start + piece.chars().count();
    }
    (text.into(), colors)
}

impl Render for PrintPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pages = if self.pages == 1 { t!("print.one_page").to_string() } else { t!("print.pages", count = self.pages).to_string() };
        let toolbar = h_flex()
            .px_3()
            .py_2()
            .gap_2()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().background)
            .child(div().flex_1().text_sm().text_color(cx.theme().muted_foreground).child(pages))
            .child(
                Button::new("preview-close")
                    .small()
                    .label(t!("print.close"))
                    .on_click(|_, window, _| window.remove_window()),
            )
            .child(
                Button::new("preview-print")
                    .small()
                    .primary()
                    .label(t!("print.print_button"))
                    .on_click(cx.listener(|this, _, window, _| this.print(window))),
            );
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().muted)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, _| {
                match event.keystroke.key.as_str() {
                    "escape" => window.remove_window(),
                    "enter" => this.print(window),
                    _ => {}
                }
            }))
            .child(toolbar)
            .child(
                div().relative().flex_1().min_h_0().child(
                    uniform_list("print-pages", self.pages, cx.processor(|this, range, window, cx| this.render_pages(range, window, cx)))
                        .track_scroll(&self.scroll)
                        .size_full(),
                )
                .vertical_scrollbar(&self.scroll),
            )
    }
}
