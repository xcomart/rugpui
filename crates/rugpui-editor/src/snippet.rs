//! A few lines of highlighted code that are not an editor.
//!
//! [`EditorView`](crate::EditorView) is an entity: it has a caret, a history, a
//! scroll offset and an input handler, because someone is going to type into
//! it. A completion popup's documentation box, a tooltip over a saved query, a
//! preview beside a file list — none of those are typed into, and paying for an
//! editor to draw four read-only lines is the wrong trade. [`CodeSnippet`] is
//! the other end: a stateless element, rebuilt on every render of its parent,
//! that lexes its text and hands gpui one [`StyledText`] per line.
//!
//! It shares the editor's colours exactly, because it shares the editor's
//! machinery exactly: the same [`Highlighter`], the same
//! [`runs_for_spans`] gap filling, the same
//! [`EditorTheme`](rugpui::EditorTheme). What it does not share is everything that makes an editor
//! an editor — no gutter, no caret, no selection, no virtualisation. A snippet
//! is expected to be a handful of lines, and [`CodeSnippet::max_lines`] is
//! there to keep it one when the text is not.
//!
//! [`tooltip_code`] is the one-liner for the case that prompted the module: the
//! same snippet, inside a [`rugpui::tooltip_frame`], as the callback gpui's
//! `.tooltip(..)` takes.

use std::sync::Arc;

use gpui::{
    AnyElement, AnyView, App, Pixels, SharedString, StyledText, Window, div, prelude::*, px,
};
use rugpui::editor_theme;

use crate::highlight::{Highlighter, LineState, plain_run, runs_for_spans};

/// Horizontal padding of the code block, in pixels.
const PADDING_X: f32 = 8.;

/// Vertical padding of the code block, in pixels.
const PADDING_Y: f32 = 6.;

/// The type size a snippet draws at when the host does not say.
///
/// Smaller than body text and smaller than the editor's usual 12.5: a snippet
/// is a quotation, and the surfaces it turns up on — a tooltip at 11 px above
/// all — are already small. A host putting one on a full-size panel raises it
/// with [`CodeSnippet::text_size`].
const DEFAULT_TEXT_SIZE: f32 = 11.5;

/// What an empty line is drawn as, so that it still occupies one.
///
/// gpui shapes a line from its text, and a [`StyledText`] over `""` has nothing
/// to take a height from. A single space has the line's full height and no ink,
/// which is what a blank line in a listing should be.
const BLANK: &str = " ";

/// A read-only block of highlighted code.
///
/// Stateless, like `rugpui`'s own widgets: it is rebuilt on every render of
/// whatever holds it, and holds nothing between frames.
///
/// ```ignore
/// CodeSnippet::new(query, highlighter_for_extension("sql").expect("sql"))
///     .font_family(self.mono.clone())
///     .max_lines(4)
/// ```
///
/// # The font is the host's to name
///
/// By default a snippet draws in whatever family the window's text style is in,
/// which for most hosts is a proportional face — and code in a proportional
/// face does not line up. So either put a fixed-pitch family on the snippet
/// with [`CodeSnippet::font_family`], or put one on a container above it the
/// way the gallery does for the editors; both reach the runs, because the
/// family named here is written into every [`TextRun`](gpui::TextRun) and the
/// one named above is what `window.text_style()` answers with.
///
/// Name a family that exists. The literal `"monospace"` is a *fontconfig*
/// alias: it resolves on Linux and nowhere else, so a host that runs on more
/// than one platform asks `cx.text_system().all_font_names()` which of its
/// candidates is installed — `monospace(cx)` in `rugpui-gallery`'s `main.rs`
/// is that loop, and is four lines long.
///
/// # Colours
///
/// [`EditorTheme`](rugpui::EditorTheme), read fresh on every render through
/// [`editor_theme`](fn@editor_theme) — the *code* palette, not the chrome
/// one, so a snippet in a tooltip matches the editor two panels over rather
/// than the tooltip's own surface. The code-block background is
/// [`EditorTheme::background`](rugpui::EditorTheme#structfield.background), which is what makes the block read as a
/// quotation; [`CodeSnippet::bare`] turns that off for a host that has already
/// drawn a container.
#[derive(IntoElement)]
pub struct CodeSnippet {
    /// The code, newline-separated. Never edited, so a [`SharedString`] rather
    /// than a rope.
    text: SharedString,
    /// The lexer, the same kind the editor takes.
    highlighter: Arc<dyn Highlighter>,
    /// The family to shape in, or the window's if the host said nothing.
    font_family: Option<SharedString>,
    /// The type size, or [`DEFAULT_TEXT_SIZE`].
    text_size: Option<Pixels>,
    /// How many lines to draw before giving up and showing an ellipsis.
    max_lines: Option<usize>,
    /// Whether to skip the code-block background and padding.
    bare: bool,
}

impl CodeSnippet {
    /// A snippet of `text`, lexed with `highlighter`.
    ///
    /// `highlighter` is whatever the editor would take —
    /// [`highlighter_for_extension`](crate::highlighter_for_extension) is the
    /// usual way to get one, and the `Arc` means a host can hand the same lexer
    /// to an editor and to every snippet beside it.
    pub fn new(text: impl Into<SharedString>, highlighter: Arc<dyn Highlighter>) -> Self {
        Self {
            text: text.into(),
            highlighter,
            font_family: None,
            text_size: None,
            max_lines: None,
            bare: false,
        }
    }

    /// Shapes the code in `family` rather than in the window's family.
    ///
    /// Default: the window's, which is almost never what a listing wants. See
    /// the type's own documentation for why the literal `"monospace"` is not a
    /// portable answer.
    pub fn font_family(mut self, family: impl Into<SharedString>) -> Self {
        self.font_family = Some(family.into());
        self
    }

    /// Draws the code at `size` rather than at 11.5 px.
    pub fn text_size(mut self, size: Pixels) -> Self {
        self.text_size = Some(size);
        self
    }

    /// Draws at most `lines` lines, then one more holding an ellipsis.
    ///
    /// The ellipsis is drawn in [`EditorTheme::comment`](rugpui::EditorTheme#structfield.comment), which is the
    /// palette's quietest slot: it is the snippet talking about itself rather
    /// than part of the code. Off by default — a host that knows its text is
    /// short does not need it, and a host that does not know should set it,
    /// because nothing else here bounds the height.
    pub fn max_lines(mut self, lines: usize) -> Self {
        self.max_lines = Some(lines);
        self
    }

    /// Drops the code-block background, padding and corner.
    ///
    /// For a host that has already drawn the box the snippet sits in — a
    /// documentation panel with its own surface, say. The lines and their
    /// colours are unchanged; only the chrome around them goes.
    pub fn bare(mut self) -> Self {
        self.bare = true;
        self
    }
}

impl RenderOnce for CodeSnippet {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let palette = editor_theme(cx);

        // The font the runs are written in. Taken from the window so that
        // weight, features and fallbacks survive, with only the family
        // replaced -- a host that named a family did not mean to reset the
        // rest of its text style.
        let mut font = window.text_style().font();
        if let Some(family) = self.font_family.clone() {
            font.family = family;
        }

        let mut lines: Vec<AnyElement> = Vec::new();
        let mut state = LineState::START;
        let mut truncated = false;
        for (index, line) in self.text.lines().enumerate() {
            if self.max_lines.is_some_and(|max| index >= max) {
                truncated = true;
                break;
            }
            let (spans, next) = self.highlighter.line(line, state);
            state = next;

            let element = if line.is_empty() {
                // No runs: `StyledText::with_runs` asserts that the runs tile
                // the text, and there is nothing to tile.
                StyledText::new(BLANK).into_any_element()
            } else {
                StyledText::new(line.to_string())
                    .with_runs(runs_for_spans(line, &spans, &palette, &font))
                    .into_any_element()
            };
            lines.push(element);
        }
        if truncated {
            lines.push(
                StyledText::new("…")
                    .with_runs(vec![plain_run("…".len(), palette.comment, &font)])
                    .into_any_element(),
            );
        }

        div()
            .flex()
            .flex_col()
            .flex_none()
            // A listing never wraps: a wrapped line of code is a line the
            // reader has to reassemble, and the surfaces this turns up on are
            // read at a glance.
            .whitespace_nowrap()
            .text_size(self.text_size.unwrap_or(px(DEFAULT_TEXT_SIZE)))
            .text_color(palette.foreground)
            .when_some(self.font_family, |this, family| this.font_family(family))
            .when(!self.bare, |this| {
                this.bg(palette.background)
                    .px(px(PADDING_X))
                    .py(px(PADDING_Y))
                    .rounded_sm()
            })
            .children(lines)
    }
}

/// Builds the callback gpui's `.tooltip(..)` takes, showing highlighted code.
///
/// [`rugpui::tooltip_with`] over a [`CodeSnippet`], which is to say: the box
/// every other tooltip in the application is drawn in, with a listing inside
/// it.
///
/// ```ignore
/// div()
///     .id("saved-query")
///     .tooltip(tooltip_code(
///         query.clone(),
///         highlighter_for_extension("sql").expect("sql"),
///         Some(self.mono.clone()),
///     ))
///     .child(name)
/// ```
///
/// `font_family` is `None` for "whatever the window is in", which is only the
/// right answer when the tooltip is opened from a subtree that already has a
/// fixed-pitch family on it. See [`CodeSnippet`] for why naming one is usually
/// the host's job, and why the name has to be a family that exists.
///
/// # Code beside other things
///
/// A tooltip that is *only* code is this function. A tooltip that is code plus
/// a caption plus a thumbnail is a [`rugpui::Tooltip`], with the snippet handed
/// in through [`Tooltip::element`](rugpui::Tooltip::element):
///
/// ```ignore
/// let sql = highlighter_for_extension("sql").expect("sql");
/// let mono = self.mono.clone();
///
/// Tooltip::new()
///     .image("icons/preview.svg", px(96.))
///     .note("public.orders — 12 rows")
///     .element(move |_window, _cx| {
///         CodeSnippet::new("select * from orders;", sql.clone())
///             .font_family(mono.clone())
///             .max_lines(4)
///             .into_any_element()
///     })
///     .build()
/// ```
///
/// The closure is called once per hover, so everything it needs — the lexer,
/// the family, the text — is cloned into it rather than borrowed.
pub fn tooltip_code(
    text: impl Into<SharedString>,
    highlighter: Arc<dyn Highlighter>,
    font_family: Option<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text = text.into();
    rugpui::tooltip_with(move |_window, _cx| {
        let mut snippet = CodeSnippet::new(text.clone(), highlighter.clone());
        if let Some(family) = font_family.clone() {
            snippet = snippet.font_family(family);
        }
        snippet.into_any_element()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ops::Deref;

    use gpui::{Entity, TestAppContext, VisualTestContext};

    use crate::lang::highlighter_for_extension;

    /// Three lines with a blank one in the middle, which is the case the
    /// [`BLANK`] substitution exists for.
    const SAMPLE: &str = "select id, name\n\nfrom public.orders;";

    /// A window with one snippet in it.
    struct Harness {
        /// Rebuilt on every render, the way a stateless element is.
        snippet: Entity<Holder>,
    }

    impl Render for Harness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            div().size_full().child(self.snippet.clone())
        }
    }

    /// The entity that rebuilds the snippet, since `CodeSnippet` is an element
    /// and an element cannot be a window's root.
    struct Holder {
        /// The code under test.
        text: SharedString,
        /// How many lines to allow, if the test is bounding them.
        max_lines: Option<usize>,
    }

    impl Render for Holder {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let mut snippet = CodeSnippet::new(
                self.text.clone(),
                highlighter_for_extension("sql").expect("sql ships with the crate"),
            );
            if let Some(max) = self.max_lines {
                snippet = snippet.max_lines(max);
            }
            snippet
        }
    }

    /// Draws `text` in a window and returns once two frames have gone by.
    fn draw(text: &str, max_lines: Option<usize>, cx: &mut TestAppContext) {
        cx.update(rugpui::init);
        cx.update(crate::init);
        let text = SharedString::from(text.to_string());
        let window = cx.add_window(move |_window, cx| Harness {
            snippet: cx.new(|_| Holder { text, max_lines }),
        });
        let mut cx = VisualTestContext::from_window(*window.deref(), cx);
        cx.run_until_parked();
        cx.refresh().expect("the window redraws");
        cx.run_until_parked();
    }

    #[gpui::test]
    fn a_snippet_with_a_blank_line_draws(cx: &mut TestAppContext) {
        draw(SAMPLE, None, cx);
    }

    #[gpui::test]
    fn a_truncated_snippet_draws(cx: &mut TestAppContext) {
        draw(SAMPLE, Some(1), cx);
    }

    #[gpui::test]
    fn an_empty_snippet_draws(cx: &mut TestAppContext) {
        draw("", None, cx);
    }
}
