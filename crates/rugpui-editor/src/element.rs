//! The custom element: the gutter, the text, and the quads under and over it.
//!
//! # Why an element rather than `uniform_list`
//!
//! gpui's [`uniform_list`](gpui::uniform_list) virtualises a list by building
//! only the rows the viewport can reach, which is what `rugpui`'s tree uses
//! and what the result grid will use. It is the wrong tool here for one reason:
//! a caret. A caret is not a row, a selection is not a row, the composing
//! underline is not a row, and every one of them has to be positioned against
//! the *shaped* text — which means the code that shapes a line and the code
//! that places a quad on it have to be the same code, holding the same
//! [`WrappedLine`]. An element gets that; a list of independently rendered rows
//! does not.
//!
//! The virtualisation is the same trick nonetheless, and it is the load-bearing
//! one: `prepaint` works out which rows the viewport covers from the scroll
//! offset and the line height, and shapes **the lines those rows belong to and
//! no others**. A hundred thousand lines cost what forty cost. `SyntaxCache`'s
//! call counter is what the tests read to hold that down.
//!
//! # Lines and rows
//!
//! With word wrap off they are the same thing and everything below reads as it
//! always did: one row per line, `wrap_width` of [`None`], an empty list of
//! breaks. With it on a line is drawn as several rows — by one call, since a
//! [`WrappedLine`] paints every row it holds — and every quad under it is cut
//! on the row boundaries first. The one thing `prepaint` does that is *not*
//! bounded by the viewport is measuring where the unseen lines break, which is
//! what [`crate::wrap`] exists to do once per line rather than once per frame.
//!
//! # Painting order
//!
//! Back to front, because each layer is drawn over the one before it:
//!
//! 1. the statement under the caret, a wash across its whole extent;
//! 2. the caret's line;
//! 3. the find matches, the current one brighter;
//! 4. the selection;
//! 5. the bracket pair;
//! 6. the text, with the composing run underlined;
//! 7. the caret;
//! 8. the line numbers, in the gutter.
//!
//! Layers 1 to 7 are drawn under a content mask that stops at the gutter's inner
//! edge. Horizontally scrolled text — and a selection over it — runs left past
//! that edge, and the mask is what keeps it out of the gutter. Clipping rather
//! than covering, which the gutter used to do with an opaque quad painted over
//! the text afterwards: a mask says only *where the text may be seen*, and so
//! assumes nothing about what fills the gutter behind the numbers or what colour
//! it is. Covering had to be right about both, and had to be redrawn whenever
//! either changed.

use gpui::{
    App, Bounds, ContentMask, CursorStyle, Element, ElementId, ElementInputHandler, Entity, Font,
    GlobalElementId, Hitbox, HitboxBehavior, InspectorElementId, LayoutId, PaintQuad, Pixels,
    Point, ShapedLine, SharedString, Style, TextAlign, TextRun, UnderlineStyle, Window,
    WindowTextSystem, WrappedLine, black, fill, point, prelude::*, px, relative, size,
};
use rugpui::EditorTheme;

use crate::editor::EditorView;
use crate::highlight::{plain_run, runs_for_spans};

/// Space between the line numbers and the text.
const GUTTER_PADDING: f32 = 12.;

/// Space to the left of the line numbers.
const GUTTER_LEAD: f32 = 8.;

/// Width of the caret.
const CARET_WIDTH: f32 = 2.;

/// Width of the bar a gutter mark draws at the left edge of the gutter.
const MARK_WIDTH: f32 = 3.;

/// How much of the line's height that bar takes, centred on the line.
const MARK_HEIGHT: f32 = 0.7;

/// How strongly a marked line's own row is tinted behind the text.
const MARK_WASH: f32 = 0.12;

/// How far past the viewport lines are shaped, so that a partially visible row
/// at either edge is drawn rather than clipped away.
const OVERSCAN: usize = 1;

/// How much of the text area a wrapped row leaves clear on the right.
///
/// A caret at the end of a full row is drawn *past* the last glyph, and the
/// vertical scrollbar rides that edge; without the margin both would sit on top
/// of the text.
const WRAP_MARGIN: f32 = 10.;

/// The narrowest a wrapped row is ever broken at.
///
/// A pane dragged shut, or a frame taken before anything has been laid out, has
/// no width to speak of, and breaking a line every other character there is a
/// great deal of work to draw nothing.
const MIN_WRAP_WIDTH: f32 = 48.;

/// The element that draws one [`EditorView`].
pub struct EditorElement {
    /// The view it draws.
    editor: Entity<EditorView>,
}

impl EditorElement {
    /// An element over `editor`.
    pub const fn new(editor: Entity<EditorView>) -> Self {
        Self { editor }
    }
}

/// Everything [`EditorElement::prepaint`] hands over to `paint`.
pub struct PrepaintState {
    /// The shaped visible lines, as `(line index, top of its first row, shaped
    /// line)`. A line holds every row it was broken into and paints all of them
    /// itself, a row apart.
    lines: Vec<(usize, Pixels, WrappedLine)>,
    /// The line numbers, shaped, as `(top of the line's first row, number)`.
    numbers: Vec<(Pixels, ShapedLine)>,
    /// Quads painted under the text.
    below: Vec<PaintQuad>,
    /// The caret, when it is visible.
    caret: Option<PaintQuad>,
    /// The bars of the gutter marks, which are painted outside the text mask.
    marks: Vec<PaintQuad>,
    /// Lines whose breaks came out of the shaper differently from what the wrap
    /// map holds, so that the map can be put right in `paint`. Empty in every
    /// ordinary frame; see [`EditorElement::prepaint`].
    corrections: Vec<(usize, Vec<u32>)>,
    /// Width of the gutter.
    gutter: Pixels,
    /// Height of one row.
    line_height: Pixels,
    /// The scroll offset the frame was built at.
    scroll: Point<Pixels>,
    /// The widest shaped line, for the horizontal scroll extent. Zero while
    /// lines are wrapped, since then nothing is off to the right.
    content_width: Pixels,
    /// The text area — the body minus the gutter — registered so that the
    /// pointer over it is an I-beam. The gutter is left out: nothing there is
    /// text, and an I-beam over a line number promises a caret it cannot give.
    text_hitbox: Hitbox,
}

impl IntoElement for EditorElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for EditorElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        // The font is the window's text style unless the host has pushed one
        // in. Both halves have to come from the same place: a caret placed
        // against text shaped in one font and drawn beside another is exactly
        // the bug an `EditorView::set_font` that reached only half of this
        // would produce.
        //
        // Read first and on its own, because the wrap width is measured in this
        // font and the measuring has to happen before anything asks which row a
        // line begins on.
        let (font, font_size, line_height, palette, digits, wrapping) = {
            let editor = self.editor.read(cx);
            let palette = editor.palette(cx);
            let (font, font_size, line_height) = match editor.font_override() {
                Some(pushed) => (pushed.font.clone(), pushed.size, pushed.line_height),
                None => {
                    let style = window.text_style();
                    (
                        style.font(),
                        style.font_size.to_pixels(window.rem_size()),
                        window.line_height(),
                    )
                }
            };
            (
                font,
                font_size,
                line_height,
                palette,
                digit_count(editor.buffer().line_count()),
                editor.is_word_wrap(),
            )
        };

        // The gutter is as wide as the largest line number, so it does not
        // twitch as the view scrolls past a power of ten.
        let digit_width = window
            .text_system()
            .shape_line(
                SharedString::from("0".repeat(digits)),
                font_size,
                &[plain_run(digits, palette.gutter, &font)],
                None,
            )
            .width;
        let gutter = digit_width + px(GUTTER_PADDING + GUTTER_LEAD);

        // The width a row is broken at, and the one thing that says whether a
        // line is one row or five. `None` is the unwrapped shape: one row per
        // line, however far it runs off to the right.
        let wrap_width = wrapping
            .then(|| (bounds.size.width - gutter - px(WRAP_MARGIN)).max(px(MIN_WRAP_WIDTH)));

        // Every line that has changed since it was last measured, measured. The
        // one pass over the whole buffer in this file, and it happens when word
        // wrap is switched on, when the width or the font changes, and for the
        // handful of lines an edit touched. See `crate::wrap`.
        if let Some(width) = wrap_width {
            let text_system = window.text_system().clone();
            self.editor.update(cx, |editor, _cx| {
                measure_wrap(editor, &text_system, width, &font, font_size);
            });
        }

        let editor = self.editor.read(cx);
        let buffer = editor.buffer();

        let scroll = editor.scroll_offset();
        // *The* virtualisation. Everything below shapes exactly these lines.
        // Counted in rows, because that is what the viewport is divided into;
        // the lines they belong to are what gets shaped.
        let total_rows = editor.wrap.total_rows(buffer.line_count());
        let first_row = ((f32::from(scroll.y) / f32::from(line_height)) as usize)
            .saturating_sub(OVERSCAN)
            .min(total_rows.saturating_sub(1));
        let rows = (f32::from(bounds.size.height) / f32::from(line_height)).ceil() as usize;
        let last_row = (first_row + rows + 2 * OVERSCAN).min(total_rows - 1);
        let last_of = |line: usize| line.min(buffer.line_count() - 1);
        let first_line = last_of(editor.wrap.row_at(first_row).0);
        let last_line = last_of(editor.wrap.row_at(last_row).0);

        let text_left = bounds.left() + gutter - scroll.x;
        let top_of = |line: usize| {
            bounds.top() + line_height * (editor.wrap.first_row(line) as f32) - scroll.y
        };

        let selection = editor.selection();
        let caret_offset = editor.caret();
        let caret_line = buffer.line_of(caret_offset);
        let brackets = editor.brackets();
        let statement = editor
            .statement_at_caret()
            .map(|span| span.range())
            .unwrap_or(0..0);
        let current_match = editor.current_match();

        let mut lines = Vec::with_capacity(last_line - first_line + 1);
        let mut numbers = Vec::with_capacity(last_line - first_line + 1);
        let mut below = Vec::new();
        let mut marks = Vec::new();
        let mut corrections = Vec::new();
        let mut caret = None;
        let mut content_width = px(0.);

        for line in first_line..=last_line {
            let start = buffer.line_start(line);
            let text = buffer.line_text(line).into_owned();
            let end = start + text.len();
            let top = top_of(line);

            let runs = runs_for(editor, line, &text, &palette, &font);
            let shaped = shape_wrapped(
                window.text_system(),
                SharedString::from(text.clone()),
                font_size,
                &runs,
                wrap_width,
            );
            content_width = content_width.max(shaped.width());

            // Where this line broke, according to the shaper that is about to
            // draw it. It is what the wrap map was measured with and so what
            // the map already holds; when the two disagree — a line edited and
            // drawn inside the same frame, before a measuring pass reached it —
            // what is drawn wins, and the map is put right in `paint`.
            let breaks = breaks_of(&shaped);
            if wrapping && editor.wrap.breaks(line) != breaks.as_slice() {
                corrections.push((line, breaks.clone()));
            }
            let row_count = breaks.len() + 1;
            let row_start = |row: usize| match row.checked_sub(1) {
                None => 0,
                Some(before) => breaks[before] as usize,
            };
            let row_end = |row: usize| breaks.get(row).map_or(text.len(), |at| *at as usize);
            let row_of = |column: usize| breaks.partition_point(|at| (*at as usize) <= column);
            // `column` is a byte offset into the line, `row` the row it is to
            // be measured on: a line drawn in rows is drawn shifted left by
            // where each row began.
            let x_at = |column: usize, row: usize| {
                text_left
                    + shaped
                        .unwrapped_layout
                        .x_for_index(column.min(shaped.len()))
                    - row_x_offset(&shaped, row_start(row))
            };
            // Every row the line takes, gutter to right edge, which is what a
            // wash across a line covers.
            let whole_line = Bounds::from_corners(
                point(bounds.left() + gutter, top),
                point(bounds.right(), top + line_height * (row_count as f32)),
            );

            // 1. the statement the caret is in, and 2. the caret's own line.
            if statement.start <= end && statement.end >= start && statement.end > statement.start {
                below.push(fill(whole_line, palette.line_highlight.opacity(0.5)));
            }
            if line == caret_line && selection.is_empty() {
                below.push(fill(whole_line, palette.line_highlight));
            }

            // 3. find matches, 4. the selection: both are byte ranges cut on
            // the row boundaries, one quad per row they touch.
            for found in editor.find_matches() {
                if found.end < start || found.start > end {
                    continue;
                }
                let color = if current_match.as_ref() == Some(found) {
                    palette.warning.opacity(0.45)
                } else {
                    palette.warning.opacity(0.2)
                };
                for row in 0..row_count {
                    let from = found.start.saturating_sub(start).max(row_start(row));
                    let to = (found.end - start).min(row_end(row));
                    if to <= from {
                        continue;
                    }
                    below.push(fill(
                        Bounds::from_corners(
                            point(x_at(from, row), top + line_height * (row as f32)),
                            point(x_at(to, row), top + line_height * ((row + 1) as f32)),
                        ),
                        color,
                    ));
                }
            }

            if !selection.is_empty() && selection.end >= start && selection.start <= end {
                for row in 0..row_count {
                    let from = selection.start.saturating_sub(start).max(row_start(row));
                    let to = (selection.end - start).min(row_end(row));
                    if to < from || (to == from && selection.end <= start + row_end(row)) {
                        continue;
                    }
                    let left = x_at(from, row);
                    // A selection that runs past the end of this row covers the
                    // break — or the line break — too, so a selection over more
                    // than one row reads as a block.
                    let right = if selection.end > start + row_end(row) {
                        bounds.right()
                    } else {
                        x_at(to, row)
                    };
                    below.push(fill(
                        Bounds::from_corners(
                            point(left, top + line_height * (row as f32)),
                            point(right, top + line_height * ((row + 1) as f32)),
                        ),
                        palette.selection,
                    ));
                }
            }

            // 5. the bracket pair.
            if let Some((left, right)) = brackets {
                for at in [left, right] {
                    if at < start || at >= end {
                        continue;
                    }
                    let to = buffer.next_grapheme(at);
                    let row = row_of(at - start);
                    below.push(fill(
                        Bounds::from_corners(
                            point(x_at(at - start, row), top + line_height * (row as f32)),
                            point(
                                x_at(to - start, row),
                                top + line_height * ((row + 1) as f32),
                            ),
                        ),
                        palette.bracket_match.opacity(0.35),
                    ));
                }
            }

            // 7. the caret, once the line it is on has been shaped.
            if line == caret_line && selection.is_empty() {
                let column = caret_offset - start;
                let row = row_of(column);
                caret = Some(fill(
                    Bounds::new(
                        point(x_at(column, row), top + line_height * (row as f32)),
                        size(px(CARET_WIDTH), line_height),
                    ),
                    palette.cursor,
                ));
            }

            // 8a. the diagnostic mark, when the host put one on this line: a
            // bar at the left edge of the gutter and a wash across the row, so
            // that a warning is visible both where the line numbers are looked
            // at and where the text is read.
            if let Some(kind) = editor.mark_on(line) {
                let color = match kind {
                    crate::editor::MarkKind::Error => palette.error,
                    crate::editor::MarkKind::Warning => palette.warning,
                };
                below.push(fill(whole_line, color.opacity(MARK_WASH)));
                let inset = line_height * ((1. - MARK_HEIGHT) / 2.);
                marks.push(fill(
                    Bounds::from_corners(
                        point(bounds.left(), top + inset),
                        point(bounds.left() + px(MARK_WIDTH), top + line_height - inset),
                    ),
                    color,
                ));
            }

            // 8. the line number, against the first row of the line: the rows
            // under it are the same line and are not numbered again.
            let number = format!("{}", line + 1);
            let color = if line == caret_line {
                palette.gutter_active
            } else {
                palette.gutter
            };
            numbers.push((
                top,
                window.text_system().shape_line(
                    SharedString::from(number.clone()),
                    font_size,
                    &[plain_run(number.len(), color, &font)],
                    None,
                ),
            ));

            lines.push((line, top, shaped));
        }

        let text_hitbox = window.insert_hitbox(
            Bounds::from_corners(
                point(bounds.left() + gutter, bounds.top()),
                bounds.bottom_right(),
            ),
            HitboxBehavior::Normal,
        );

        PrepaintState {
            lines,
            numbers,
            below,
            marks,
            corrections,
            caret,
            gutter,
            line_height,
            scroll,
            content_width: if wrapping { px(0.) } else { content_width },
            text_hitbox,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.editor.read(cx).input_focus();
        let read_only = self.editor.read(cx).is_read_only();
        let focused = self.editor.read(cx).is_focused(window);

        // Even a read-only editor takes the handler: without it the platform
        // has no way to report the selection, and copy stops working.
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.editor.clone()),
            cx,
        );

        // A read-only editor still selects and copies, so it still gets the
        // I-beam; `TextInput` shows an arrow only when it is *disabled*, which
        // an editor cannot be.
        window.set_cursor_style(CursorStyle::IBeam, &prepaint.text_hitbox);

        let line_height = prepaint.line_height;
        let scroll = prepaint.scroll;
        let gutter = prepaint.gutter;

        // The body minus the gutter. Everything that belongs to the text is
        // clipped to it, because a horizontally scrolled line — and a selection
        // over it — extends to the left of it, and the gutter is not where it may
        // be seen. gpui intersects a nested mask with the one it sits inside, so
        // the body mask around it still holds the other three edges.
        let text_area = Bounds::from_corners(
            point(bounds.left() + gutter, bounds.top()),
            bounds.bottom_right(),
        );

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.with_content_mask(Some(ContentMask { bounds: text_area }), |window| {
                for quad in prepaint.below.drain(..) {
                    window.paint_quad(quad);
                }

                let text_left = bounds.left() + gutter - scroll.x;
                for (_, top, shaped) in &prepaint.lines {
                    // One call draws every row the line was broken into, a row
                    // apart, which is the whole reason the shaped line rather
                    // than the row is what this element holds.
                    shaped
                        .paint(
                            point(text_left, *top),
                            line_height,
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        )
                        .ok();
                }

                if focused
                    && !read_only
                    && let Some(caret) = prepaint.caret.take()
                {
                    window.paint_quad(caret);
                }
            });

            // The diagnostic bars ride the gutter's left edge, outside the
            // text mask with the numbers.
            for quad in prepaint.marks.drain(..) {
                window.paint_quad(quad);
            }

            // The line numbers are the only thing drawn in the gutter, and so the
            // only thing outside the mask above. Nothing here fills the gutter
            // behind them — the editor's own surface has already done that — which
            // is the whole economy of clipping instead of covering.
            for (top, number) in &prepaint.numbers {
                let left = bounds.left() + gutter - px(GUTTER_PADDING) - number.width;
                number
                    .paint(
                        point(left, *top),
                        line_height,
                        TextAlign::Left,
                        None,
                        window,
                        cx,
                    )
                    .ok();
            }
        });

        let lines = std::mem::take(&mut prepaint.lines);
        let corrections = std::mem::take(&mut prepaint.corrections);
        let content_width = prepaint.content_width;
        self.editor.update(cx, |editor, _cx| {
            editor.layout.bounds = Some(bounds);
            editor.layout.gutter = gutter;
            editor.layout.line_height = line_height;
            editor.layout.content_width = editor.layout.content_width.max(content_width);
            editor.layout.lines = lines
                .into_iter()
                .map(|(line, _, shaped)| (line, shaped))
                .collect();
            for (line, breaks) in corrections {
                editor.wrap.measured(line, breaks);
            }
        });
    }
}

/// Measures every line whose breaks are not known, and no others.
///
/// Called from `prepaint` before anything reads a row, and the only writer of
/// [`crate::wrap::WrapMap`]'s measurements apart from the corrections `paint`
/// applies. The runs are plain rather than the highlighter's: a colour moves no
/// glyph, so the breaks are the same either way, and lexing a whole buffer to
/// find out where it wraps would be work for nothing.
fn measure_wrap(
    editor: &mut EditorView,
    text_system: &WindowTextSystem,
    width: Pixels,
    font: &Font,
    font_size: Pixels,
) {
    let line_count = editor.buffer().line_count();
    if !editor.wrap.begin(width, font_size, font, line_count) {
        return;
    }
    for line in 0..line_count {
        if !editor.wrap.unmeasured(line) {
            continue;
        }
        let text = editor.buffer().line_text(line).into_owned();
        let breaks = if text.is_empty() {
            Vec::new()
        } else {
            let runs = [plain_run(text.len(), black(), font)];
            let shaped = shape_wrapped(
                text_system,
                SharedString::from(text),
                font_size,
                &runs,
                Some(width),
            );
            breaks_of(&shaped)
        };
        editor.wrap.measured(line, breaks);
    }
    editor.wrap.finish();
}

/// One line, shaped, broken at `wrap_width` when there is one.
///
/// `shape_text` splits on newlines and answers with a line per piece; the text
/// handed to it here never has one, so the answer is always the single line
/// asked for.
fn shape_wrapped(
    text_system: &WindowTextSystem,
    text: SharedString,
    font_size: Pixels,
    runs: &[TextRun],
    wrap_width: Option<Pixels>,
) -> WrappedLine {
    text_system
        .shape_text(text, font_size, runs, wrap_width, None)
        .ok()
        .and_then(|lines| lines.into_iter().next())
        .unwrap_or_default()
}

/// The byte offsets, relative to the start of the line, at which each row after
/// the first begins.
pub(crate) fn breaks_of(shaped: &WrappedLine) -> Vec<u32> {
    shaped
        .wrap_boundaries()
        .iter()
        .filter_map(|boundary| {
            let run = shaped.unwrapped_layout.runs.get(boundary.run_ix)?;
            Some(run.glyphs.get(boundary.glyph_ix)?.index as u32)
        })
        .collect()
}

/// How far left a row is drawn from where its glyphs sit in the unwrapped
/// layout, given the byte offset the row begins at.
///
/// Every x inside a wrapped row is an x in the one long shaped line minus this.
pub(crate) fn row_x_offset(shaped: &WrappedLine, row_start: usize) -> Pixels {
    shaped
        .unwrapped_layout
        .x_for_index(row_start.min(shaped.len()))
}

/// The colored runs of one line: the highlighter's spans, plus the composing
/// underline laid over them.
///
/// The gap filling is [`runs_for_spans`]; what is left here is the split the
/// IME needs, which is the editor's alone -- nothing else in this crate draws
/// text that is halfway through being composed.
fn runs_for(
    editor: &EditorView,
    line: usize,
    text: &str,
    palette: &EditorTheme,
    font: &Font,
) -> Vec<TextRun> {
    if text.is_empty() {
        return Vec::new();
    }
    let start = editor.buffer().line_start(line);
    let spans = editor.syntax().spans(editor.buffer(), line);
    let runs = runs_for_spans(text, &spans, palette, font);

    let Some(marked) = editor.marked() else {
        return runs;
    };
    let end = start + text.len();
    if marked.end < start || marked.start > end {
        return runs;
    }

    // Split the runs at the composition's edges and underline what is between
    // them. The underline is the only signal that a syllable is still being
    // composed, and it has to survive whatever color the lexer gave the run.
    let from = marked.start.max(start) - start;
    let to = marked.end.min(end) - start;
    let mut split = Vec::with_capacity(runs.len() + 2);
    let mut at = 0;
    for run in runs {
        let run_end = at + run.len;
        for (piece_start, piece_end) in [
            (at, run_end.min(from)),
            (at.max(from), run_end.min(to)),
            (at.max(to), run_end),
        ] {
            if piece_end <= piece_start {
                continue;
            }
            let underlined = piece_start >= from && piece_end <= to;
            split.push(TextRun {
                len: piece_end - piece_start,
                underline: underlined.then(|| UnderlineStyle {
                    color: Some(run.color),
                    thickness: px(1.),
                    wavy: false,
                }),
                ..run.clone()
            });
        }
        at = run_end;
    }
    split
}

/// How many decimal digits `n` needs, at least two.
const fn digit_count(n: usize) -> usize {
    let mut digits = 1;
    let mut left = n;
    while left >= 10 {
        left /= 10;
        digits += 1;
    }
    if digits < 2 { 2 } else { digits }
}
