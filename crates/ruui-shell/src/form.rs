//! Small pieces every settings form is built out of.
//!
//! Nothing here knows what is being configured. The *body* of a settings form —
//! which rows there are, what they mean, and how they turn into a settings
//! struct — is the application's, and stays there; these are the parts that
//! were being written out identically in three of them: a titled card, a muted
//! hint, a unit suffix beside a narrow control, and the four lines that turn a
//! text field into a number field.

use gpui::{App, Context, Entity, IntoElement, SharedString, div, prelude::*, px};
use ruui::{TextInput, theme};

/// Wraps `body` in a titled card.
pub fn section<E: IntoElement>(
    title: SharedString,
    cx: &App,
    body: E,
) -> impl IntoElement + use<E> {
    let chrome = theme(cx);
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .p(px(12.))
        .rounded_lg()
        .border_1()
        .border_color(chrome.border)
        .bg(chrome.surface)
        .child(
            div()
                .text_size(px(11.))
                .text_color(chrome.text_muted)
                .child(title),
        )
        .child(body)
}

/// A muted paragraph explaining something a form row cannot say on its own.
pub fn hint(words: SharedString, cx: &App) -> impl IntoElement + use<> {
    let chrome = theme(cx);
    div()
        .text_size(px(11.))
        .text_color(chrome.text_muted)
        .child(words)
}

/// Lays a short unit hint out to the right of a narrow control.
pub fn suffixed<E: IntoElement>(
    control: E,
    words: SharedString,
    cx: &App,
) -> impl IntoElement + use<E> {
    let chrome = theme(cx);
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.))
        .w_full()
        .child(div().flex_none().w(px(96.)).child(control))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(px(11.))
                .text_color(chrome.text_muted)
                .child(words),
        )
}

/// The font families the platform offers, in the order gpui reports them —
/// sorted and deduplicated already.
///
/// Names starting with a dot are dropped: those are the platform's private
/// aliases, such as `.SystemUIFont` on macOS, which are not meant to be chosen
/// by name.
pub fn installed_fonts(cx: &App) -> Vec<SharedString> {
    cx.text_system()
        .all_font_names()
        .into_iter()
        .filter(|name| !name.starts_with('.'))
        .map(SharedString::from)
        .collect()
}

/// Trimmed content of `input`.
pub fn text(input: &Entity<TextInput>, cx: &App) -> String {
    input.read(cx).content().trim().to_owned()
}

/// Parses `input` into `T`, or `None` when it is blank or malformed.
pub fn parse_number<T: std::str::FromStr>(input: &Entity<TextInput>, cx: &App) -> Option<T> {
    text(input, cx).parse::<T>().ok()
}

/// Replaces the contents of `input`.
pub fn set_text(input: &Entity<TextInput>, value: impl Into<SharedString>, cx: &mut App) {
    input.update(cx, |input, cx| input.set_content(value, cx));
}

/// Renders `value` without a trailing `.0`, so 14.0 shows as "14".
pub fn format_number(value: f32) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

/// Installs an observer that keeps `input` numeric.
///
/// The text field has no input filter, so the content is rewritten after every
/// edit. Rewriting only when the text actually changes stops the observer from
/// re-triggering itself.
///
/// Generic over the view that owns the field, because the observer is
/// registered on that view's context and every dialog is a different type.
pub fn restrict_to_number<V: 'static>(
    cx: &mut Context<V>,
    input: &Entity<TextInput>,
    decimals: bool,
    max_len: usize,
) {
    cx.observe(input, move |_this, input, cx| {
        let content = input.read(cx).content().to_owned();
        let filtered = numeric(&content, decimals, max_len);
        if filtered != content {
            input.update(cx, |input, cx| input.set_content(filtered, cx));
        }
    })
    .detach();
}

/// `value` with everything that is not part of a number taken out.
///
/// At most one decimal point, and only when `decimals` allows one at all; at
/// most `max_len` characters, counted after the filtering so that a paste of
/// letters does not eat the budget.
fn numeric(value: &str, decimals: bool, max_len: usize) -> String {
    let mut seen_dot = false;
    value
        .chars()
        .filter(|c| {
            if c.is_ascii_digit() {
                true
            } else if decimals && *c == '.' && !seen_dot {
                seen_dot = true;
                true
            } else {
                false
            }
        })
        .take(max_len)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_number_loses_its_trailing_zero() {
        assert_eq!(format_number(14.0), "14");
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(13.5), "13.5");
        assert_eq!(format_number(0.75), "0.75");
    }

    #[test]
    fn filtering_keeps_the_digits_and_nothing_else() {
        assert_eq!(numeric("12ab3", false, 8), "123");
        assert_eq!(numeric("-12", false, 8), "12");
        assert_eq!(numeric("  4 5 ", false, 8), "45");
        assert_eq!(numeric("", false, 8), "");
    }

    #[test]
    fn a_decimal_point_is_kept_once_and_only_where_it_is_allowed() {
        assert_eq!(numeric("13.5", true, 8), "13.5");
        assert_eq!(numeric("13.5.7", true, 8), "13.57");
        assert_eq!(numeric("13.5", false, 8), "135");
        assert_eq!(numeric(".5", true, 8), ".5");
    }

    #[test]
    fn the_length_is_counted_after_the_filtering() {
        // A paste of letters must not use up the budget the digits need.
        assert_eq!(numeric("aaaa1234", false, 4), "1234");
        assert_eq!(numeric("123456", false, 4), "1234");
    }
}
