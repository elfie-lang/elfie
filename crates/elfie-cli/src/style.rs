//! Compiled from `def/cli/style.lfy`: how the command line paints what it prints.
//!
//! Color is decided once per stream, standard output and standard error apart, and every
//! piece of text is painted by what it means rather than by a color named where it is
//! printed, so the whole CLI reads as one palette and a person scanning a long compile finds
//! the failures by their color alone. Color never reaches a file, a pipe, JSON, or
//! compile.log, so what a script or an agent reads is the same text it read before color
//! existed; only a person at a terminal sees escapes.

use std::io::IsTerminal as _;

use elfie_core::generation::{Reason, ReviewStatus};
use elfie_core::query::Severity;

use crate::data::Step;

/// When the CLI colors what it prints, as `--color` names it.
// @lfy def/cli/style.lfy:ColorChoice
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorChoice {
    Auto,   // @lfy def/cli/style.lfy:ColorChoice.auto
    Always, // @lfy def/cli/style.lfy:ColorChoice.always
    Never,  // @lfy def/cli/style.lfy:ColorChoice.never
}

impl ColorChoice {
    /// Every choice, in the order the enum lists them.
    // @lfy def/cli/style.lfy:ColorChoice
    pub const ALL: [ColorChoice; 3] = [ColorChoice::Auto, ColorChoice::Always, ColorChoice::Never];

    /// The value of the enum member: what is typed after `--color`.
    // @lfy def/cli/style.lfy:ColorChoice
    pub fn value(self) -> &'static str {
        match self {
            ColorChoice::Auto => "auto",
            ColorChoice::Always => "always",
            ColorChoice::Never => "never",
        }
    }

    /// The choice a text names, when it names one.
    // @lfy def/cli/style.lfy:ColorChoice
    pub fn lookup(text: &str) -> Option<ColorChoice> {
        ColorChoice::ALL.into_iter().find(|choice| choice.value() == text)
    }
}

/// What a piece of printed text means; its value is the ANSI SGR parameters that color it.
// @lfy def/cli/style.lfy:Tone
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// Something done or satisfied, in green.
    Success, // @lfy def/cli/style.lfy:Tone.success
    /// An error, a rejection, or a violation, in red.
    Failure, // @lfy def/cli/style.lfy:Tone.failure
    /// Something a person must look at that is not an error, in yellow.
    Warning, // @lfy def/cli/style.lfy:Tone.warning
    /// Work under way or a name to act on, in cyan.
    Active, // @lfy def/cli/style.lfy:Tone.active
    /// What a line is about: a file, a batch, a unit, in bold.
    Subject, // @lfy def/cli/style.lfy:Tone.subject
    /// Detail read only when wanted: counters, times, prefixes, dependencies, in dim.
    Muted, // @lfy def/cli/style.lfy:Tone.muted
}

impl Tone {
    /// The value of the enum member: the ANSI SGR parameters that color the text.
    // @lfy def/cli/style.lfy:Tone
    pub fn value(self) -> &'static str {
        match self {
            Tone::Success => "32",
            Tone::Failure => "31",
            Tone::Warning => "33",
            Tone::Active => "36",
            Tone::Subject => "1",
            Tone::Muted => "2",
        }
    }
}

/// The character every escape sequence begins with.
// @lfy def/cli/style.lfy:paint
const ESCAPE: char = '\u{1b}';

/// Whether text printed to one stream is colored.
///
/// `choice` is what `--color` asked for; `is_error` is true for standard error and false for
/// standard output, so the two streams are decided apart.
// @lfy def/cli/style.lfy:colorsOn
pub fn colors_on(choice: ColorChoice, is_error: bool) -> bool {
    let variable = |name: &str| std::env::var(name).ok();
    // @lfy def/cli/style.lfy:colorsOn
    colored(
        choice,
        variable("NO_COLOR").as_deref(),
        variable("TERM").as_deref(),
        variable("CLICOLOR_FORCE").as_deref(),
        // @lfy def/cli/style.lfy:colorsOn
        || {
            if is_error {
                std::io::stderr().is_terminal()
            } else {
                std::io::stdout().is_terminal()
            }
        },
    )
}

/// Whether text is colored, given what the environment says: `NO_COLOR`, `TERM`, and
/// `CLICOLOR_FORCE` as `Process.environment` gives them, each `None` when it is not set, and
/// whether the stream is a terminal, asked only when nothing above it decided.
// @lfy def/cli/style.lfy:colorsOn
fn colored(
    choice: ColorChoice,
    no_color: Option<&str>,
    term: Option<&str>,
    force: Option<&str>,
    is_terminal: impl Fn() -> bool,
) -> bool {
    match choice {
        // Always, whatever the environment says. @lfy def/cli/style.lfy:colorsOn
        ColorChoice::Always => true,
        // @lfy def/cli/style.lfy:colorsOn
        ColorChoice::Never => false,
        ColorChoice::Auto => {
            // Any text but the empty one turns color off. @lfy def/cli/style.lfy:colorsOn
            if no_color.is_some_and(|value| !value.is_empty()) {
                return false;
            }
            // @lfy def/cli/style.lfy:colorsOn
            if term == Some("dumb") {
                return false;
            }
            // Any text but the empty one and 0 keeps color through a pager.
            // @lfy def/cli/style.lfy:colorsOn
            if force.is_some_and(|value| !value.is_empty() && value != "0") {
                return true;
            }
            // @lfy def/cli/style.lfy:colorsOn
            is_terminal()
        }
    }
}

/// Text wrapped in the escapes that color it by its tone, or left as it is.
///
/// `on` is whether the stream it goes to is colored, as [`colors_on`] decides. The reset
/// always follows, so no color runs past the text, and [`width`] of what is returned is
/// [`width`] of the text, so a painted column lines up as its plain text would.
// @lfy def/cli/style.lfy:paint
pub fn paint(text: &str, tone: Tone, on: bool) -> String {
    // Nothing to color, or a stream that is not colored, is the text unchanged.
    // @lfy def/cli/style.lfy:paint
    if !on || text.is_empty() {
        return text.to_string();
    }
    // @lfy def/cli/style.lfy:paint
    format!("{ESCAPE}[{}m{text}{ESCAPE}[0m", tone.value())
}

/// How many columns text takes in a terminal, painted or not.
///
/// Characters are counted, not bytes, and every escape sequence that begins with the escape
/// character and `[` and ends with `m` is skipped, so painted and plain text measure the
/// same.
// @lfy def/cli/style.lfy:width
pub fn width(text: &str) -> usize {
    let mut columns = 0;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        // @lfy def/cli/style.lfy:width
        if character == ESCAPE && characters.peek() == Some(&'[') {
            characters.next();
            for inner in characters.by_ref() {
                if inner == 'm' {
                    break;
                }
            }
            continue;
        }
        // @lfy def/cli/style.lfy:width
        columns += 1;
    }
    columns
}

/// Text followed by spaces up to a width, so the columns after it line up; text already that
/// wide or wider is returned unchanged, never cut.
// @lfy def/cli/style.lfy:padded
pub fn padded(text: &str, columns: usize) -> String {
    let have = width(text);
    // @lfy def/cli/style.lfy:padded
    if have >= columns {
        return text.to_string();
    }
    // @lfy def/cli/style.lfy:padded
    format!("{text}{}", " ".repeat(columns - have))
}

/// A length of time, short enough to scan in a column.
///
/// Under a minute it is the seconds to one decimal and `s`, as `12.3s`; under an hour the
/// whole minutes, `m`, the remaining whole seconds as two digits, and `s`, as `2m05s`; above
/// that the whole hours, `h`, the remaining whole minutes as two digits, and `m`, as `1h02m`.
/// Every part is truncated, never rounded up, so 59.99 seconds is `59.9s` and never `60.0s`.
// @lfy def/cli/style.lfy:duration
pub fn duration(seconds: f64) -> String {
    let seconds = if seconds.is_finite() && seconds > 0.0 { seconds } else { 0.0 };
    // A tenth of a second, truncated. The tiny margin undoes the error of holding a
    // decimal fraction in binary, so that 0.3 seconds is 0.3s rather than 0.2s; it is far
    // smaller than a tenth, so nothing is ever carried up to the next one.
    // @lfy def/cli/style.lfy:duration
    let tenths = (seconds * 10.0 + 1e-6).floor() as u64;
    let whole = tenths / 10;
    // @lfy def/cli/style.lfy:duration
    if whole < 60 {
        format!("{whole}.{}s", tenths % 10)
    } else if whole < 3600 {
        // @lfy def/cli/style.lfy:duration
        format!("{}m{:02}s", whole / 60, whole % 60)
    } else {
        // @lfy def/cli/style.lfy:duration
        format!("{}h{:02}m", whole / 3600, (whole % 3600) / 60)
    }
}

/// The tone a progress line of a step is painted in.
// @lfy def/cli/style.lfy:toneOf
pub fn tone_of(step: Step) -> Tone {
    match step {
        // @lfy def/cli/style.lfy:toneOf
        Step::Accepted | Step::Reviewed | Step::GlobalReviewed => Tone::Success,
        // @lfy def/cli/style.lfy:toneOf
        Step::Rejected | Step::Failed => Tone::Failure,
        // @lfy def/cli/style.lfy:toneOf
        Step::Retrying | Step::Blocked | Step::Clarification => Tone::Warning,
        // @lfy def/cli/style.lfy:toneOf
        Step::Requesting | Step::Compiling | Step::Checking | Step::Verifying | Step::GlobalVerifying => Tone::Active,
        // @lfy def/cli/style.lfy:toneOf
        Step::Planned | Step::Finished => Tone::Subject,
    }
}

/// One character that marks a progress line's step, so the eye finds the outcomes without
/// reading.
// @lfy def/cli/style.lfy:glyphOf
pub fn glyph_of(step: Step) -> &'static str {
    match step {
        // @lfy def/cli/style.lfy:glyphOf
        Step::Planned | Step::Finished => "◆",
        // @lfy def/cli/style.lfy:glyphOf
        Step::Requesting | Step::Checking => "·",
        // @lfy def/cli/style.lfy:glyphOf
        Step::Compiling | Step::Verifying | Step::GlobalVerifying => "▸",
        // @lfy def/cli/style.lfy:glyphOf
        Step::Accepted | Step::Reviewed | Step::GlobalReviewed => "✓",
        // @lfy def/cli/style.lfy:glyphOf
        Step::Rejected | Step::Failed => "✗",
        // @lfy def/cli/style.lfy:glyphOf
        Step::Retrying => "↻",
        // @lfy def/cli/style.lfy:glyphOf
        Step::Blocked => "■",
        // @lfy def/cli/style.lfy:glyphOf
        Step::Clarification => "?",
    }
}

/// The tone a diagnostic's severity is painted in.
// @lfy def/cli/style.lfy:severityTone
pub fn severity_tone(severity: Severity) -> Tone {
    match severity {
        // @lfy def/cli/style.lfy:severityTone
        Severity::Error => Tone::Failure,
        Severity::Warning => Tone::Warning,
        Severity::Information | Severity::Hint => Tone::Active,
    }
}

/// The tone a review's status is painted in.
// @lfy def/cli/style.lfy:statusTone
pub fn status_tone(status: ReviewStatus) -> Tone {
    match status {
        // @lfy def/cli/style.lfy:statusTone
        ReviewStatus::Satisfied => Tone::Success,
        ReviewStatus::Violated => Tone::Failure,
        ReviewStatus::Unverifiable => Tone::Warning,
    }
}

/// The tone a unit's reason is painted in; `None`, printed as up to date, is muted.
// @lfy def/cli/style.lfy:reasonTone
pub fn reason_tone(reason: Option<Reason>) -> Tone {
    match reason {
        // @lfy def/cli/style.lfy:reasonTone
        Some(Reason::Requested) => Tone::Active,
        // @lfy def/cli/style.lfy:reasonTone
        Some(Reason::Fresh) => Tone::Success,
        // @lfy def/cli/style.lfy:reasonTone
        Some(Reason::Changed | Reason::Requirements | Reason::Dependency) => Tone::Warning,
        // @lfy def/cli/style.lfy:reasonTone
        Some(Reason::Violated) => Tone::Failure,
        // @lfy def/cli/style.lfy:reasonTone
        None => Tone::Muted,
    }
}

/// A count and what it counts, painted only when it is worth noticing.
///
/// The text is the count, a space, and the label; a count that is not 0 is painted in the
/// tone, and 0 is muted, so a summary shows at a glance which counts matter.
// @lfy def/cli/style.lfy:tally
pub fn tally(count: usize, label: &str, tone: Tone, on: bool) -> String {
    // @lfy def/cli/style.lfy:tally
    let text = format!("{count} {label}");
    // @lfy def/cli/style.lfy:tally
    paint(&text, if count == 0 { Tone::Muted } else { tone }, on)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every member of `ColorChoice` is spelled as `--color` names it, and nothing else is
    /// one.
    // @lfy def/cli/style.lfy:ColorChoice
    #[test]
    fn a_color_choice_is_spelled_as_it_is_typed() {
        assert_eq!(ColorChoice::Auto.value(), "auto");
        assert_eq!(ColorChoice::Always.value(), "always");
        assert_eq!(ColorChoice::Never.value(), "never");
        for choice in ColorChoice::ALL {
            assert_eq!(ColorChoice::lookup(choice.value()), Some(choice));
        }
        assert_eq!(ColorChoice::lookup("sometimes"), None);
    }

    /// Each tone's value is the ANSI SGR parameters that color it.
    // @lfy def/cli/style.lfy:Tone
    #[test]
    fn a_tone_is_the_parameters_that_color_it() {
        assert_eq!(Tone::Success.value(), "32");
        assert_eq!(Tone::Failure.value(), "31");
        assert_eq!(Tone::Warning.value(), "33");
        assert_eq!(Tone::Active.value(), "36");
        assert_eq!(Tone::Subject.value(), "1");
        assert_eq!(Tone::Muted.value(), "2");
    }

    /// Always is true whatever the environment says, never is false, and auto asks the
    /// environment: `NO_COLOR` or a dumb terminal turns color off, `CLICOLOR_FORCE` turns it
    /// on through a pager, and otherwise the stream decides.
    // @lfy def/cli/style.lfy:colorsOn
    #[test]
    fn colors_on_decides_once_per_stream() {
        let never = || false;
        let always = || true;
        // @lfy def/cli/style.lfy:colorsOn
        assert!(colored(ColorChoice::Always, Some("1"), Some("dumb"), None, never));
        // @lfy def/cli/style.lfy:colorsOn
        assert!(!colored(ColorChoice::Never, None, None, Some("1"), always));
        // Any text but the empty one turns color off. @lfy def/cli/style.lfy:colorsOn
        assert!(!colored(ColorChoice::Auto, Some("1"), None, Some("1"), always));
        assert!(!colored(ColorChoice::Auto, Some("0"), None, None, always));
        assert!(colored(ColorChoice::Auto, Some(""), None, None, always));
        // @lfy def/cli/style.lfy:colorsOn
        assert!(!colored(ColorChoice::Auto, None, Some("dumb"), Some("1"), always));
        // Set to anything but the empty one and 0, color is kept through a pager.
        // @lfy def/cli/style.lfy:colorsOn
        assert!(colored(ColorChoice::Auto, None, Some("xterm"), Some("1"), never));
        assert!(colored(ColorChoice::Auto, Some(""), None, Some("yes"), never));
        // Unset, empty, or 0, the stream decides. @lfy def/cli/style.lfy:colorsOn
        assert!(!colored(ColorChoice::Auto, None, Some("xterm"), None, never));
        assert!(!colored(ColorChoice::Auto, None, None, Some(""), never));
        assert!(!colored(ColorChoice::Auto, None, None, Some("0"), never));
        assert!(colored(ColorChoice::Auto, None, None, None, always));
        // Standard output, piped with CLICOLOR_FORCE unset, is not colored.
        // @lfy def/cli/style.lfy:colorsOn
        assert!(!colors_on(ColorChoice::Never, false));
        assert!(colors_on(ColorChoice::Always, false));
    }

    /// Text is wrapped in the escapes its tone names, the reset always follows, and a stream
    /// that is not colored or text that is empty is left as it is.
    // @lfy def/cli/style.lfy:paint
    #[test]
    fn paint_wraps_text_in_the_escapes_of_its_tone() {
        // @lfy def/cli/style.lfy:paint
        assert_eq!(paint("ok", Tone::Success, true), "\u{1b}[32mok\u{1b}[0m");
        // @lfy def/cli/style.lfy:paint
        assert_eq!(paint("ok", Tone::Success, false), "ok");
        // @lfy def/cli/style.lfy:paint
        assert_eq!(paint("", Tone::Failure, true), "");
        // A painted column lines up as its plain text would.
        // @lfy def/cli/style.lfy:paint
        for tone in [Tone::Success, Tone::Failure, Tone::Warning, Tone::Active, Tone::Subject, Tone::Muted] {
            assert_eq!(width(&paint("batch", tone, true)), width("batch"));
        }
    }

    /// Characters are counted, not bytes, and every escape sequence is skipped.
    // @lfy def/cli/style.lfy:width
    #[test]
    fn width_counts_characters_and_skips_escapes() {
        assert_eq!(width("batch"), 5);
        // @lfy def/cli/style.lfy:width
        assert_eq!(width("\u{1b}[1mbatch\u{1b}[0m"), 5);
        // @lfy def/cli/style.lfy:width
        assert_eq!(width("✓ ok"), 4);
        assert_eq!(width(""), 0);
    }

    /// Spaces are appended up to the width, and text that wide or wider is never cut.
    // @lfy def/cli/style.lfy:padded
    #[test]
    fn padded_fills_a_width_and_never_cuts() {
        // @lfy def/cli/style.lfy:padded
        assert_eq!(padded("ab", 4), "ab  ");
        // @lfy def/cli/style.lfy:padded
        assert_eq!(padded("abcdef", 4), "abcdef");
        assert_eq!(padded("ab", 2), "ab");
        // A painted text is padded by what it measures, not by its bytes.
        // @lfy def/cli/style.lfy:padded
        assert_eq!(width(&padded(&paint("ab", Tone::Subject, true), 4)), 4);
    }

    /// Seconds, minutes, and hours, every part truncated and never rounded up.
    // @lfy def/cli/style.lfy:duration
    #[test]
    fn duration_is_short_enough_to_scan() {
        // @lfy def/cli/style.lfy:duration
        assert_eq!(duration(0.42), "0.4s");
        assert_eq!(duration(12.34), "12.3s");
        // @lfy def/cli/style.lfy:duration
        assert_eq!(duration(59.99), "59.9s");
        // @lfy def/cli/style.lfy:duration
        assert_eq!(duration(125.0), "2m05s");
        // @lfy def/cli/style.lfy:duration
        assert_eq!(duration(3725.0), "1h02m");
        // @lfy def/cli/style.lfy:duration
        assert_eq!(duration(0.0), "0.0s");
        assert_eq!(duration(0.3), "0.3s");
        assert_eq!(duration(60.0), "1m00s");
        assert_eq!(duration(3600.0), "1h00m");
    }

    /// Every step has the tone its outcome means and the glyph that marks it.
    // @lfy def/cli/style.lfy:toneOf
    #[test]
    fn a_step_has_a_tone_and_a_glyph() {
        // @lfy def/cli/style.lfy:toneOf
        assert_eq!(tone_of(Step::Accepted), Tone::Success);
        assert_eq!(tone_of(Step::Reviewed), Tone::Success);
        assert_eq!(tone_of(Step::GlobalReviewed), Tone::Success);
        // @lfy def/cli/style.lfy:toneOf
        assert_eq!(tone_of(Step::Rejected), Tone::Failure);
        assert_eq!(tone_of(Step::Failed), Tone::Failure);
        // @lfy def/cli/style.lfy:toneOf
        assert_eq!(tone_of(Step::Retrying), Tone::Warning);
        assert_eq!(tone_of(Step::Blocked), Tone::Warning);
        assert_eq!(tone_of(Step::Clarification), Tone::Warning);
        // @lfy def/cli/style.lfy:toneOf
        assert_eq!(tone_of(Step::Requesting), Tone::Active);
        assert_eq!(tone_of(Step::Compiling), Tone::Active);
        assert_eq!(tone_of(Step::Checking), Tone::Active);
        assert_eq!(tone_of(Step::Verifying), Tone::Active);
        assert_eq!(tone_of(Step::GlobalVerifying), Tone::Active);
        // @lfy def/cli/style.lfy:toneOf
        assert_eq!(tone_of(Step::Planned), Tone::Subject);
        assert_eq!(tone_of(Step::Finished), Tone::Subject);
    }

    /// One character marks each step.
    // @lfy def/cli/style.lfy:glyphOf
    #[test]
    fn a_glyph_marks_each_step() {
        // @lfy def/cli/style.lfy:glyphOf
        assert_eq!(glyph_of(Step::Planned), "◆");
        assert_eq!(glyph_of(Step::Finished), "◆");
        // @lfy def/cli/style.lfy:glyphOf
        assert_eq!(glyph_of(Step::Requesting), "·");
        assert_eq!(glyph_of(Step::Checking), "·");
        // @lfy def/cli/style.lfy:glyphOf
        assert_eq!(glyph_of(Step::Compiling), "▸");
        assert_eq!(glyph_of(Step::Verifying), "▸");
        assert_eq!(glyph_of(Step::GlobalVerifying), "▸");
        // @lfy def/cli/style.lfy:glyphOf
        assert_eq!(glyph_of(Step::Accepted), "✓");
        assert_eq!(glyph_of(Step::Reviewed), "✓");
        assert_eq!(glyph_of(Step::GlobalReviewed), "✓");
        // @lfy def/cli/style.lfy:glyphOf
        assert_eq!(glyph_of(Step::Failed), "✗");
        assert_eq!(glyph_of(Step::Rejected), "✗");
        // @lfy def/cli/style.lfy:glyphOf
        assert_eq!(glyph_of(Step::Retrying), "↻");
        assert_eq!(glyph_of(Step::Blocked), "■");
        assert_eq!(glyph_of(Step::Clarification), "?");
        // One character, so every step column is one wide. @lfy def/cli/style.lfy:glyphOf
        for step in STEPS {
            assert_eq!(width(glyph_of(step)), 1, "{}", step.name());
        }
    }

    /// Every step, so that a test can walk them all.
    const STEPS: [Step; 15] = [
        Step::Planned,
        Step::Requesting,
        Step::Compiling,
        Step::Checking,
        Step::Accepted,
        Step::Rejected,
        Step::Verifying,
        Step::Reviewed,
        Step::GlobalVerifying,
        Step::GlobalReviewed,
        Step::Retrying,
        Step::Blocked,
        Step::Clarification,
        Step::Failed,
        Step::Finished,
    ];

    /// A severity, a status, and a reason each have the tone they mean.
    // @lfy def/cli/style.lfy:severityTone
    #[test]
    fn a_severity_a_status_and_a_reason_have_their_tones() {
        // @lfy def/cli/style.lfy:severityTone
        assert_eq!(severity_tone(Severity::Error), Tone::Failure);
        assert_eq!(severity_tone(Severity::Warning), Tone::Warning);
        assert_eq!(severity_tone(Severity::Information), Tone::Active);
        assert_eq!(severity_tone(Severity::Hint), Tone::Active);
        // @lfy def/cli/style.lfy:statusTone
        assert_eq!(status_tone(ReviewStatus::Satisfied), Tone::Success);
        assert_eq!(status_tone(ReviewStatus::Violated), Tone::Failure);
        assert_eq!(status_tone(ReviewStatus::Unverifiable), Tone::Warning);
        // @lfy def/cli/style.lfy:reasonTone
        assert_eq!(reason_tone(Some(Reason::Requested)), Tone::Active);
        assert_eq!(reason_tone(Some(Reason::Fresh)), Tone::Success);
        // @lfy def/cli/style.lfy:reasonTone
        assert_eq!(reason_tone(Some(Reason::Changed)), Tone::Warning);
        assert_eq!(reason_tone(Some(Reason::Requirements)), Tone::Warning);
        assert_eq!(reason_tone(Some(Reason::Dependency)), Tone::Warning);
        // @lfy def/cli/style.lfy:reasonTone
        assert_eq!(reason_tone(Some(Reason::Violated)), Tone::Failure);
        // @lfy def/cli/style.lfy:reasonTone
        assert_eq!(reason_tone(None), Tone::Muted);
    }

    /// The count and what it counts, painted in the tone when it is not 0 and muted when it
    /// is.
    // @lfy def/cli/style.lfy:tally
    #[test]
    fn a_tally_is_painted_only_when_it_is_worth_noticing() {
        // @lfy def/cli/style.lfy:tally
        assert_eq!(tally(2, "rejected", Tone::Failure, true), "\u{1b}[31m2 rejected\u{1b}[0m");
        // @lfy def/cli/style.lfy:tally
        assert_eq!(tally(0, "rejected", Tone::Failure, true), "\u{1b}[2m0 rejected\u{1b}[0m");
        // @lfy def/cli/style.lfy:tally
        assert_eq!(tally(0, "rejected", Tone::Failure, false), "0 rejected");
        // The words are the count, a space, and the label, painted or not.
        // @lfy def/cli/style.lfy:tally
        assert_eq!(tally(2, "rejected", Tone::Failure, false), "2 rejected");
    }
}
