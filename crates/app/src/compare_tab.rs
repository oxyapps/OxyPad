//! Side-by-side comparison of two open tabs.
//!
//! Both columns are cells of the same virtualized rows, so they always
//! scroll together vertically. The divider between them can be dragged
//! (double-click restores 50/50), and each column scrolls horizontally on
//! its own.

use std::{ops::Range, rc::Rc};

use gpui_kit::{
    component::{
        ActiveTheme, IconName, h_flex,
        highlighter::SyntaxHighlighter,
        input::Rope,
        scroll::ScrollableElement,
        v_flex,
    },
    prelude::FluentBuilder,
    *,
};
use oxypad_core::{Alignment, Cell, RowKind, align};

const TAB_WIDTH: usize = 4;
const DIVIDER_WIDTH: Pixels = px(5.);
const MARKER_WIDTH: Pixels = px(14.);
const MIN_RATIO: f32 = 0.1;

/// One side of a comparison: a snapshot of an open tab.
pub struct CompareSource {
    pub tab_id: u64,
    pub title: SharedString,
    pub text: String,
    pub language: SharedString,
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Left,
    Right,
}

/// Display-ready lines of one side.
struct SideLines {
    tab_id: u64,
    title: SharedString,
    /// The text as compared, kept to redo syntax colors on a theme switch.
    text: String,
    /// Lines with tabs expanded.
    lines: Vec<SharedString>,
    /// Syntax highlights per line, in display offsets.
    syntax: Vec<Vec<(Range<usize>, HighlightStyle)>>,
    /// Tab positions per line, to map source offsets to display offsets.
    tabs: Vec<Vec<usize>>,
    widest: usize,
}

impl SideLines {
    fn new(source: CompareSource, language: &str, cx: &App) -> Self {
        let raw = oxypad_core::lines(&source.text);
        let tabs: Vec<Vec<usize>> = raw
            .iter()
            .map(|line| line.match_indices('\t').map(|(i, _)| i).collect())
            .collect();
        let lines: Vec<SharedString> =
            raw.iter().map(|line| line.replace('\t', &" ".repeat(TAB_WIDTH)).into()).collect();
        let widest = lines.iter().map(|line| line.chars().count()).max().unwrap_or(0);
        let mut this = Self {
            tab_id: source.tab_id,
            title: source.title,
            text: String::new(),
            lines,
            syntax: Vec::new(),
            tabs,
            widest,
        };
        this.text = source.text;
        this.highlight(language, cx);
        this
    }

    /// Resolves syntax colors with the current theme.
    fn highlight(&mut self, language: &str, cx: &App) {
        let raw = oxypad_core::lines(&self.text);
        let tabs = &self.tabs;
        self.syntax = syntax_by_line(&self.text, &raw, language, cx)
            .into_iter()
            .zip(tabs)
            .map(|(spans, tabs)| {
                spans
                    .into_iter()
                    .map(|(range, style)| (to_display(tabs, range.start)..to_display(tabs, range.end), style))
                    .collect()
            })
            .collect();
    }
}

/// Source byte offset → display offset, given the tab positions in the line.
fn to_display(tabs: &[usize], offset: usize) -> usize {
    offset + tabs.iter().take_while(|&&tab| tab < offset).count() * (TAB_WIDTH - 1)
}

/// Highlights the whole text once, then splits the spans by line.
fn syntax_by_line(
    text: &str,
    lines: &[&str],
    language: &str,
    cx: &App,
) -> Vec<Vec<(Range<usize>, HighlightStyle)>> {
    let mut per_line = vec![Vec::new(); lines.len()];
    if language == "plaintext" || text.is_empty() {
        return per_line;
    }
    let mut highlighter = SyntaxHighlighter::new(language);
    highlighter.update(None, &Rope::from_str(text), None);
    let spans = highlighter.styles(&(0..text.len()), &*cx.theme().highlight_theme);

    // Start offset of each line in `text`.
    let mut starts = Vec::with_capacity(lines.len());
    let mut at = 0;
    for line in text.split_inclusive('\n').take(lines.len()) {
        starts.push(at);
        at += line.len();
    }
    for (range, style) in spans {
        let mut ix = starts.partition_point(|&start| start <= range.start).saturating_sub(1);
        while ix < lines.len() && starts[ix] < range.end {
            let line_start = starts[ix];
            let line_end = line_start + lines[ix].len();
            let start = range.start.max(line_start);
            let end = range.end.min(line_end);
            if start < end {
                per_line[ix].push((start - line_start..end - line_start, style));
            }
            ix += 1;
        }
    }
    per_line
}

pub struct CompareTab {
    id: u64,
    language: SharedString,
    left: SideLines,
    right: SideLines,
    alignment: Rc<Alignment>,
    /// Theme the syntax colors were resolved with.
    theme_name: SharedString,
    /// Width of the left column as a share of the available width.
    ratio: f32,
    dragging: bool,
    /// Last laid-out bounds of the comparison body.
    bounds: Bounds<Pixels>,
    h_offset: [Pixels; 2],
    scroll: UniformListScrollHandle,
    /// Index into `alignment.changes` of the change last navigated to.
    current_change: Option<usize>,
    /// Keeps keyboard focus (F7, shortcuts) inside the window while shown.
    focus_handle: FocusHandle,
}

impl CompareTab {
    pub fn new(id: u64, left: CompareSource, right: CompareSource, cx: &mut Context<Self>) -> Self {
        // One language colors both columns; prefer whichever side has one.
        let language = if left.language == "plaintext" { right.language.clone() } else { left.language.clone() };
        let alignment = Rc::new(align(&left.text, &right.text));
        Self {
            id,
            left: SideLines::new(left, &language, cx),
            right: SideLines::new(right, &language, cx),
            language,
            alignment,
            theme_name: cx.theme().theme_name().clone(),
            ratio: 0.5,
            dragging: false,
            bounds: Bounds::default(),
            h_offset: [px(0.); 2],
            scroll: UniformListScrollHandle::new(),
            current_change: None,
            focus_handle: cx.focus_handle(),
        }
    }

    /// Compares fresh snapshots of the same two tabs, keeping the layout.
    pub fn refresh(&mut self, left: CompareSource, right: CompareSource, cx: &mut Context<Self>) {
        self.alignment = Rc::new(align(&left.text, &right.text));
        self.left = SideLines::new(left, &self.language, cx);
        self.right = SideLines::new(right, &self.language, cx);
        self.theme_name = cx.theme().theme_name().clone();
        self.current_change = None;
        cx.notify();
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// Tab ids of the two compared tabs, left first.
    pub fn sources(&self) -> (u64, u64) {
        (self.left.tab_id, self.right.tab_id)
    }

    pub fn title(&self) -> SharedString {
        format!("{} ↔ {}", self.left.title, self.right.title).into()
    }

    pub fn has_changes(&self) -> bool {
        !self.alignment.is_identical()
    }

    pub fn next_change(&mut self, cx: &mut Context<Self>) {
        let count = self.alignment.changes.len();
        if count == 0 {
            return;
        }
        let next = self.current_change.map_or(0, |ix| (ix + 1) % count);
        self.go_to_change(next, cx);
    }

    pub fn previous_change(&mut self, cx: &mut Context<Self>) {
        let count = self.alignment.changes.len();
        if count == 0 {
            return;
        }
        let previous = self.current_change.map_or(count - 1, |ix| (ix + count - 1) % count);
        self.go_to_change(previous, cx);
    }

    fn go_to_change(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.current_change = Some(ix);
        self.scroll.scroll_to_item(self.alignment.changes[ix], ScrollStrategy::Center);
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.focus_handle.focus(window, cx);
    }

    /// Summary for the status bar.
    pub fn summary(&self) -> SharedString {
        let alignment = &self.alignment;
        if alignment.is_identical() {
            return t!("compare.identical").into();
        }
        let position = match self.current_change {
            Some(ix) => t!("compare.position", current = ix + 1, total = alignment.changes.len()).to_string(),
            None => t!("compare.count", count = alignment.changes.len()).to_string(),
        };
        t!(
            "compare.summary",
            position = position,
            changed = alignment.changed,
            removed = alignment.removed,
            added = alignment.added
        )
        .into()
    }

    fn side(&self, side: Side) -> &SideLines {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    // ---- geometry ----------------------------------------------------------

    fn char_width(&self, cx: &App) -> Pixels {
        // Monospace advance; precise enough for scroll limits and gutters.
        cx.theme().mono_font_size * 0.6
    }

    fn row_height(&self, cx: &App) -> Pixels {
        (cx.theme().mono_font_size * 1.5).round()
    }

    fn gutter_width(&self, cx: &App) -> Pixels {
        let digits = self.left.lines.len().max(self.right.lines.len()).max(1).to_string().len();
        self.char_width(cx) * (digits as f32) + px(16.)
    }

    /// Width of each column.
    fn column_widths(&self) -> (Pixels, Pixels) {
        let available = (self.bounds.size.width - DIVIDER_WIDTH).max(px(0.));
        let left = (available * self.ratio).round();
        (left, available - left)
    }

    fn max_h_offset(&self, side: Side, cx: &App) -> Pixels {
        let (left, right) = self.column_widths();
        let column = if side == Side::Left { left } else { right };
        let content = self.char_width(cx) * (self.side(side).widest as f32 + 2.)
            + self.gutter_width(cx)
            + MARKER_WIDTH;
        (content - column).max(px(0.))
    }

    // ---- mouse ---------------------------------------------------------------

    fn on_divider_down(&mut self, event: &MouseDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        cx.stop_propagation();
        if event.click_count >= 2 {
            self.ratio = 0.5;
            self.dragging = false;
        } else {
            self.dragging = true;
        }
        cx.notify();
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.dragging {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.dragging = false;
            cx.notify();
            return;
        }
        let width = self.bounds.size.width - DIVIDER_WIDTH;
        if width <= px(0.) {
            return;
        }
        let x = event.position.x - self.bounds.origin.x - DIVIDER_WIDTH / 2.;
        self.ratio = (x / width).clamp(MIN_RATIO, 1. - MIN_RATIO);
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.dragging {
            self.dragging = false;
            cx.notify();
        }
    }

    fn on_scroll(&mut self, side: Side, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = event.delta.pixel_delta(self.row_height(cx));
        if delta.x.abs() <= delta.y.abs() {
            return;
        }
        // Horizontal gestures belong to this column only; the list would
        // otherwise turn them into vertical scrolling.
        cx.stop_propagation();
        let ix = side as usize;
        let max = self.max_h_offset(side, cx);
        self.h_offset[ix] = (self.h_offset[ix] - delta.x).clamp(px(0.), max);
        cx.notify();
    }

    // ---- rendering -------------------------------------------------------------

    fn render_divider(&self, id: impl Into<ElementId>, cx: &mut Context<Self>) -> impl IntoElement {
        let color = if self.dragging { cx.theme().primary } else { cx.theme().border };
        div()
            .id(id)
            .w(DIVIDER_WIDTH)
            .h_full()
            .flex_shrink_0()
            .cursor_col_resize()
            .child(div().mx_auto().w(px(1.)).h_full().bg(color))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_divider_down))
    }

    fn render_cell(
        &self,
        row: usize,
        side: Side,
        cell: Option<&Cell>,
        kind: RowKind,
        width: Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let (color, marker) = match side {
            Side::Left => (theme.red, "−"),
            Side::Right => (theme.green, "+"),
        };
        let changed = match side {
            Side::Left => matches!(kind, RowKind::Changed | RowKind::Removed),
            Side::Right => matches!(kind, RowKind::Changed | RowKind::Added),
        };
        let background = match cell {
            None => theme.muted.opacity(0.35),
            Some(_) if changed => color.opacity(0.12),
            Some(_) => transparent_black(),
        };
        let muted = theme.muted_foreground;
        let offset = self.h_offset[side as usize];
        let gutter = self.gutter_width(cx);
        let lines = self.side(side);

        let content = cell.map(|cell| {
            let text = lines.lines[cell.line].clone();
            let tabs = &lines.tabs[cell.line];
            let emphasis = cell.emphasis.iter().map(|range| {
                (
                    to_display(tabs, range.start)..to_display(tabs, range.end),
                    HighlightStyle { background_color: Some(color.opacity(0.35)), ..Default::default() },
                )
            });
            let highlights: Vec<_> =
                combine_highlights(lines.syntax[cell.line].iter().cloned(), emphasis).collect();
            (cell.line + 1, StyledText::new(text).with_highlights(highlights))
        });

        h_flex()
            .id((if side == Side::Left { "left" } else { "right" }, row))
            .w(width)
            .h_full()
            .flex_shrink_0()
            .overflow_hidden()
            .bg(background)
            .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
                this.on_scroll(side, event, cx)
            }))
            .when_some(content, |this, (number, text)| {
                this.child(
                    div()
                        .w(gutter)
                        .flex_shrink_0()
                        .pr_2()
                        .text_right()
                        .text_color(muted)
                        .child(number.to_string()),
                )
                .child(
                    div()
                        .w(MARKER_WIDTH)
                        .flex_shrink_0()
                        .text_color(color)
                        .when(changed, |this| this.child(marker)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .overflow_hidden()
                        .child(div().ml(-offset).whitespace_nowrap().child(text)),
                )
            })
    }

    fn render_rows(&mut self, range: Range<usize>, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let (left_width, right_width) = self.column_widths();
        let row_height = self.row_height(cx);
        let alignment = self.alignment.clone();
        range
            .map(|ix| {
                let row = &alignment.rows[ix];
                h_flex()
                    .h(row_height)
                    .w_full()
                    .child(self.render_cell(ix, Side::Left, row.left.as_ref(), row.kind, left_width, cx))
                    .child(self.render_divider(("divider", ix), cx))
                    .child(self.render_cell(ix, Side::Right, row.right.as_ref(), row.kind, right_width, cx))
                    .into_any_element()
            })
            .collect()
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let (left_width, right_width) = self.column_widths();
        let (border, background) = (cx.theme().border, cx.theme().secondary);
        let title = |title: &SharedString, width: Pixels| {
            h_flex()
                .w(width)
                .flex_shrink_0()
                .overflow_hidden()
                .gap_2()
                .px_3()
                .py_1()
                .child(IconName::File)
                .child(div().truncate().child(title.clone()))
        };
        h_flex()
            .text_sm()
            .border_b_1()
            .border_color(border)
            .bg(background)
            .child(title(&self.left.title, left_width))
            .child(self.render_divider("header-divider", cx))
            .child(title(&self.right.title, right_width))
    }
}

impl Render for CompareTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Syntax colors are resolved against the theme; redo them after a
        // theme change.
        if &self.theme_name != cx.theme().theme_name() {
            self.theme_name = cx.theme().theme_name().clone();
            let language = self.language.clone();
            self.left.highlight(&language, cx);
            self.right.highlight(&language, cx);
        }

        let row_count = self.alignment.rows.len();
        let entity = cx.entity();
        let (font, font_size) = (cx.theme().mono_font_family.clone(), cx.theme().mono_font_size);
        let (success, success_foreground) = (cx.theme().success, cx.theme().success_foreground);
        v_flex()
            .track_focus(&self.focus_handle)
            .size_full()
            .font_family(font)
            .text_size(font_size)
            .when(self.dragging, |this| this.cursor_col_resize())
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .child(self.render_header(cx))
            .when(self.alignment.is_identical(), |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .px_3()
                        .py_1()
                        .text_sm()
                        .text_color(success_foreground)
                        .bg(success)
                        .child(IconName::CircleCheck)
                        .child(t!("compare.identical_sentence").to_string()),
                )
            })
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(
                        canvas(
                            move |bounds, _, cx| {
                                entity.update(cx, |this, cx| {
                                    if this.bounds != bounds {
                                        this.bounds = bounds;
                                        cx.notify();
                                    }
                                })
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        uniform_list(
                            "compare-rows",
                            row_count,
                            cx.processor(|this, range, _, cx| this.render_rows(range, cx)),
                        )
                        .track_scroll(&self.scroll)
                        .size_full(),
                    )
                    .vertical_scrollbar(&self.scroll),
            )
    }
}
