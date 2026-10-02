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
        // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:13ea2914d2ef0d42364a5666b7b2e3e5ad5a2bec225c08e5074369ec2a1486ed
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
        // Always, whatever the environment says.
        // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:301b6c7660010e2ddf5897afb4219bba93647bf791c7d83c62ffc2fd6dc246ed
        ColorChoice::Always => true,
        // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:51b5bc7dcc299fd9e0162c246879a8a7677937b08cdb8efab6713912b41d712f
        ColorChoice::Never => false,
        ColorChoice::Auto => {
            // Any text but the empty one turns color off.
            // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:e6362de3f6ed08806c842adf4a92b9e1389c953e360cdec10164ec440069430f
            if no_color.is_some_and(|value| !value.is_empty()) {
                return false;
            }
            // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:5e68c907a3e7ff5175cea826734bfd9aef043996d9ea0da973a5ff0e8cec8c52
            if term == Some("dumb") {
                return false;
            }
            // Any text but the empty one and 0 keeps color through a pager.
            // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:08cff9bf4762648d7455742690fb1012d29c0148f78173fae42aeac6896769ad
            if force.is_some_and(|value| !value.is_empty() && value != "0") {
                return true;
            }
            // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:13ea2914d2ef0d42364a5666b7b2e3e5ad5a2bec225c08e5074369ec2a1486ed
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
    // A stream that is not colored is the text unchanged.
    // @lfy def/cli/style.lfy:paint#paint:paint:a71b34754c2a1f6fb1875c94ac0f67275289f340fc1aa2289d16aa2b753d50ca
    // Nothing to color is the text unchanged.
    // @lfy def/cli/style.lfy:paint#paint:paint:887caee4bf900a333c30d4891d2ac0cd87f1e9b7e054d0802c5ceae73aff0def
    if !on || text.is_empty() {
        return text.to_string();
    }
    // The reset always follows, so no color runs past the text and what is returned measures
    // what the text measures.
    // @lfy def/cli/style.lfy:paint#paint:paint:a26b232d91a356a224e81e8eae8f29d98a73d6a42635d349c22851e7960204ae
    // @lfy def/cli/style.lfy:paint#paint:paint:d2394fdec9545fbdcb8b86ed5786fe0b235b7a97b0aed0d14692666e082d6232
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
        // @lfy def/cli/style.lfy:width#width:width:1c8bf7433715e6af2e4dc86332c9c837f8809157894f987235eead83163027cf
        if character == ESCAPE && characters.peek() == Some(&'[') {
            characters.next();
            for inner in characters.by_ref() {
                if inner == 'm' {
                    break;
                }
            }
            continue;
        }
        // @lfy def/cli/style.lfy:width#width:width:1c8bf7433715e6af2e4dc86332c9c837f8809157894f987235eead83163027cf
        columns += 1;
    }
    columns
}

/// Text followed by spaces up to a width, so the columns after it line up; text already that
/// wide or wider is returned unchanged, never cut.
// @lfy def/cli/style.lfy:padded
pub fn padded(text: &str, columns: usize) -> String {
    let have = width(text);
    // @lfy def/cli/style.lfy:padded#padded:padded:f4d261ad0420755417b66b9631cd961cc36a4f20f25082ab1f6cf0d2aff8ad98
    if have >= columns {
        return text.to_string();
    }
    // @lfy def/cli/style.lfy:padded#padded:padded:c7299e2ac1d307b6a6dfdb936fc07ea56be5e075d04dab33fca571b588efc33d
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
    // @lfy def/cli/style.lfy:duration#duration:duration:8ca88562d8c6e5dec77a957242a697f30dc0f73ac0cede5f8c4b237a76d0c419
    let tenths = (seconds * 10.0 + 1e-6).floor() as u64;
    let whole = tenths / 10;
    // @lfy def/cli/style.lfy:duration#duration:duration:bdaf33d10a559c31fed03ad63033171ff7748854325342070ef9005bcd9e726c
    if whole < 60 {
        format!("{whole}.{}s", tenths % 10)
    } else if whole < 3600 {
        // @lfy def/cli/style.lfy:duration#duration:duration:45752ec47a9540a84d457cb57940b15769f2ed08a0ea2868411d6a8491d377ad
        format!("{}m{:02}s", whole / 60, whole % 60)
    } else {
        // @lfy def/cli/style.lfy:duration#duration:duration:dd742c1c3cc3a97b80b6de90cc6c051d50a547c403eede626647736f71a1aac3
        format!("{}h{:02}m", whole / 3600, (whole % 3600) / 60)
    }
}

/// The tone a progress line of a step is painted in.
// @lfy def/cli/style.lfy:toneOf
pub fn tone_of(step: Step) -> Tone {
    match step {
        // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:80d019b710b01db5e2ec24642071f7f1e01b6845806c26d4c35ae43b1a37bb87
        Step::Accepted | Step::Reviewed | Step::GlobalReviewed => Tone::Success,
        // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:7c99092c688d2e1c6afefb08fe08b45571d67677d1ae1f96b868c85f53c58dbb
        Step::Rejected | Step::Failed => Tone::Failure,
        // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:1d1d00113b3b3f23ccc6f043b93c4a0e372cafc331cedbbf972a0cdc45923fd5
        Step::Retrying | Step::Blocked | Step::Clarification => Tone::Warning,
        // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:bbca4f24023ee9e8c8a9cf9a5d9f763c46b4237ee5bb9566ab980120014c4409
        Step::Requesting | Step::Compiling | Step::Checking | Step::Verifying | Step::GlobalVerifying => Tone::Active,
        // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:58836dc724cf2963083aa70014885f28a3b6370d6f123ee57956f0b156ad301e
        Step::Planned | Step::Finished => Tone::Subject,
    }
}

/// One character that marks a progress line's step, so the eye finds the outcomes without
/// reading.
// @lfy def/cli/style.lfy:glyphOf
pub fn glyph_of(step: Step) -> &'static str {
    match step {
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:f5fc9102b729a1695599d9f1c60d947a06f66947aa31a72af718d62b08a0ba8c
        Step::Planned | Step::Finished => "◆",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:18c86a248eae3e7333c065b9e2dc69023a465f7c5112c985230ddccaa2cc996c
        Step::Requesting | Step::Checking => "·",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:ff36ebe0ff46f6923c7a8c1123617f2ad6d7b5c219c1e3dc8ceef03e7e811d92
        Step::Compiling | Step::Verifying | Step::GlobalVerifying => "▸",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:c72d261846854334790bdbce4677f86dcb6648858138ef897da2aa026e274640
        Step::Accepted | Step::Reviewed | Step::GlobalReviewed => "✓",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:da6a8a46c2c6f5b8f318dc28656257270d35a12b752260f5d7b691075022951b
        Step::Rejected | Step::Failed => "✗",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:d4054bc957dfed7e7c21e21c6304298b7fd86cfad457a667298ef0af7cc5438d
        Step::Retrying => "↻",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:d4054bc957dfed7e7c21e21c6304298b7fd86cfad457a667298ef0af7cc5438d
        Step::Blocked => "■",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:d4054bc957dfed7e7c21e21c6304298b7fd86cfad457a667298ef0af7cc5438d
        Step::Clarification => "?",
    }
}

/// The tone a diagnostic's severity is painted in.
// @lfy def/cli/style.lfy:severityTone
pub fn severity_tone(severity: Severity) -> Tone {
    match severity {
        // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:988a081cdf2488795bd371340bfa3af51271217d1d6f89ff4be4c756bb79ea73
        Severity::Error => Tone::Failure,
        Severity::Warning => Tone::Warning,
        Severity::Information | Severity::Hint => Tone::Active,
    }
}

/// The tone a review's status is painted in.
// @lfy def/cli/style.lfy:statusTone
pub fn status_tone(status: ReviewStatus) -> Tone {
    match status {
        // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:b60eaf654eb94c310fcc43a7ae2efec00d4918c0f9ad0e6852dcc98f5aa08835
        ReviewStatus::Satisfied => Tone::Success,
        ReviewStatus::Violated => Tone::Failure,
        ReviewStatus::Unverifiable => Tone::Warning,
    }
}

/// The tone a unit's reason is painted in; `None`, printed as up to date, is muted.
// @lfy def/cli/style.lfy:reasonTone
pub fn reason_tone(reason: Option<Reason>) -> Tone {
    match reason {
        // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:8d6898f1de1b0a930511711a62d207dc605bbf10d23c194db8d0d5c99027adc9
        Some(Reason::Requested) => Tone::Active,
        // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:8d6898f1de1b0a930511711a62d207dc605bbf10d23c194db8d0d5c99027adc9
        Some(Reason::Fresh) => Tone::Success,
        // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:766ff7ed234740d82081af5018d2c64a3edcda40c6ecdc9e437c3e25256f5a99
        Some(Reason::Changed | Reason::Requirements | Reason::Dependency) => Tone::Warning,
        // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:6e3e72b4caa7cfe4a0b109f5211121030d44d883f3b9583151f1640e720c38a8
        Some(Reason::Violated) => Tone::Failure,
        // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:6a45d395babe1a35f49fcc8e83830ef2477f6b524531a1baf31095cdffa95a09
        None => Tone::Muted,
    }
}

/// A count and what it counts, painted only when it is worth noticing.
///
/// The text is the count, a space, and the label; a count that is not 0 is painted in the
/// tone, and 0 is muted, so a summary shows at a glance which counts matter.
// @lfy def/cli/style.lfy:tally
pub fn tally(count: usize, label: &str, tone: Tone, on: bool) -> String {
    // @lfy def/cli/style.lfy:tally#tally:tally:d5560e01139c5b0cceeea67c7f5579d2dbd37213245e5f781499a076fdde4de1
    let text = format!("{count} {label}");
    // @lfy def/cli/style.lfy:tally#tally:tally:df353484a54814b80445678c577e6e3e4cf2405d8c9de995bb17acac86ca4d8b
    // @lfy def/cli/style.lfy:tally#tally:tally:4cccc0e5af75e2d33b3b247f091056de3fae98a0aecb15e962e97b9e6ff06ebb
    paint(&text, if count == 0 { Tone::Muted } else { tone }, on)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    /// Never a terminal.
    fn never() -> bool {
        false
    }

    /// Always a terminal.
    fn always() -> bool {
        true
    }

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

    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:826819f63633d9f55a14686004593dc83a45240b548d4948f442673fe73bc38a
    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:301b6c7660010e2ddf5897afb4219bba93647bf791c7d83c62ffc2fd6dc246ed
    #[test]
    fn always_is_colored_whatever_the_environment_says() {
        assert!(colors_on(ColorChoice::Always, false));
        assert!(colored(ColorChoice::Always, Some("1"), Some("dumb"), None, never));
    }

    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:4ccddab6622467889792bc98c596017ee97bef82aa54a6b5704456a4ef6d14b0
    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:51b5bc7dcc299fd9e0162c246879a8a7677937b08cdb8efab6713912b41d712f
    #[test]
    fn never_is_not_colored() {
        assert!(!colors_on(ColorChoice::Never, true));
        assert!(!colored(ColorChoice::Never, None, None, Some("1"), always));
    }

    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:e6362de3f6ed08806c842adf4a92b9e1389c953e360cdec10164ec440069430f
    #[test]
    fn no_color_as_any_text_but_the_empty_one_turns_color_off() {
        assert!(!colored(ColorChoice::Auto, Some("1"), None, Some("1"), always));
        assert!(!colored(ColorChoice::Auto, Some("0"), None, None, always));
        assert!(colored(ColorChoice::Auto, Some(""), None, None, always));
    }

    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:5e68c907a3e7ff5175cea826734bfd9aef043996d9ea0da973a5ff0e8cec8c52
    #[test]
    fn a_dumb_terminal_turns_color_off() {
        assert!(!colored(ColorChoice::Auto, None, Some("dumb"), Some("1"), always));
    }

    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:08cff9bf4762648d7455742690fb1012d29c0148f78173fae42aeac6896769ad
    #[test]
    fn clicolor_force_keeps_color_through_a_pager() {
        assert!(colored(ColorChoice::Auto, None, Some("xterm"), Some("1"), never));
        assert!(colored(ColorChoice::Auto, Some(""), None, Some("yes"), never));
    }

    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:a46d5515967b7a194a7948144cb6fd6cb6d984e19efecb3bb61dc7d1056df775
    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:13ea2914d2ef0d42364a5666b7b2e3e5ad5a2bec225c08e5074369ec2a1486ed
    #[test]
    fn auto_with_nothing_forcing_it_is_what_the_stream_is() {
        // Standard output, piped with CLICOLOR_FORCE unset, is not colored.
        assert!(!colored(ColorChoice::Auto, None, Some("xterm"), None, never));
        assert!(!colored(ColorChoice::Auto, None, None, Some(""), never));
        assert!(!colored(ColorChoice::Auto, None, None, Some("0"), never));
        assert!(colored(ColorChoice::Auto, None, None, None, always));
    }

    // @lfy def/cli/style.lfy:paint#paint:paint:de381fd52ede1936e22b7a7e414805c9ffc5e7f5f98bb6e0604f5293bd584bd9
    // @lfy def/cli/style.lfy:paint#paint:paint:a26b232d91a356a224e81e8eae8f29d98a73d6a42635d349c22851e7960204ae
    #[test]
    fn paint_wraps_text_in_the_escapes_of_its_tone() {
        assert_eq!(paint("ok", Tone::Success, true), "\u{1b}[32mok\u{1b}[0m");
        // The reset always follows, so no color runs past the text.
        assert!(paint("ok", Tone::Success, true).ends_with("\u{1b}[0m"));
    }

    // @lfy def/cli/style.lfy:paint#paint:paint:714bf468d158f2876b569511f35c05e1a21fb1724f0ca965c7628d89960dab89
    // @lfy def/cli/style.lfy:paint#paint:paint:a71b34754c2a1f6fb1875c94ac0f67275289f340fc1aa2289d16aa2b753d50ca
    #[test]
    fn a_stream_that_is_not_colored_leaves_text_as_it_is() {
        assert_eq!(paint("ok", Tone::Success, false), "ok");
    }

    // @lfy def/cli/style.lfy:paint#paint:paint:bd96aa24959517e862a0e4b99692fbb12fd640fe75292517d756ba594cf16051
    // @lfy def/cli/style.lfy:paint#paint:paint:887caee4bf900a333c30d4891d2ac0cd87f1e9b7e054d0802c5ceae73aff0def
    #[test]
    fn empty_text_is_left_as_it_is() {
        assert_eq!(paint("", Tone::Failure, true), "");
    }

    // @lfy def/cli/style.lfy:paint#paint:paint:d2394fdec9545fbdcb8b86ed5786fe0b235b7a97b0aed0d14692666e082d6232
    #[test]
    fn a_painted_column_lines_up_as_its_plain_text_would() {
        for tone in [Tone::Success, Tone::Failure, Tone::Warning, Tone::Active, Tone::Subject, Tone::Muted] {
            assert_eq!(width(&paint("batch", tone, true)), width("batch"));
        }
    }

    // @lfy def/cli/style.lfy:width#width:width:9e76b8b723f9540e09b2b776dfaf845e2b8f285bc83a5fa25fc213b3c105409b
    #[test]
    fn width_counts_the_characters_of_plain_text() {
        assert_eq!(width("batch"), 5);
        assert_eq!(width(""), 0);
    }

    // @lfy def/cli/style.lfy:width#width:width:f3f70bd8393933c75f4154e52e6fc1f09b03e7021c28ce686f23c8627dceb52a
    // @lfy def/cli/style.lfy:width#width:width:1c8bf7433715e6af2e4dc86332c9c837f8809157894f987235eead83163027cf
    #[test]
    fn width_skips_every_escape_sequence() {
        assert_eq!(width("\u{1b}[1mbatch\u{1b}[0m"), 5);
    }

    // @lfy def/cli/style.lfy:width#width:width:d4af67135cd0c7d3c956c72196261778e010f8d01c709e4ef3500b5322e4fb32
    #[test]
    fn width_counts_characters_and_not_bytes() {
        assert_eq!(width("✓ ok"), 4);
    }

    // @lfy def/cli/style.lfy:padded#padded:padded:26429a29e6c03161f9989e316d03d7775c25e490bc1cde9771e058db8d3b9359
    // @lfy def/cli/style.lfy:padded#padded:padded:c7299e2ac1d307b6a6dfdb936fc07ea56be5e075d04dab33fca571b588efc33d
    #[test]
    fn padded_appends_spaces_up_to_the_width() {
        assert_eq!(padded("ab", 4), "ab  ");
        // A painted text is padded by what it measures, not by its bytes.
        assert_eq!(width(&padded(&paint("ab", Tone::Subject, true), 4)), 4);
    }

    // @lfy def/cli/style.lfy:padded#padded:padded:d13e67da436a0aaa2823e82520f26b91a24330e945f857cb910c002a6078a097
    // @lfy def/cli/style.lfy:padded#padded:padded:f4d261ad0420755417b66b9631cd961cc36a4f20f25082ab1f6cf0d2aff8ad98
    #[test]
    fn padded_never_cuts() {
        assert_eq!(padded("abcdef", 4), "abcdef");
        assert_eq!(padded("ab", 2), "ab");
    }

    // @lfy def/cli/style.lfy:duration#duration:duration:8a51c8b1053d1d0b2bebccc2a31c2a9c19dda84690ab75418fddf2ecd276b196
    // @lfy def/cli/style.lfy:duration#duration:duration:bdaf33d10a559c31fed03ad63033171ff7748854325342070ef9005bcd9e726c
    #[test]
    fn duration_of_a_fraction_of_a_second() {
        assert_eq!(duration(0.42), "0.4s");
        assert_eq!(duration(0.0), "0.0s");
        assert_eq!(duration(0.3), "0.3s");
    }

    // @lfy def/cli/style.lfy:duration#duration:duration:183d9e5f3d4a488194fbfbc8b2d94530103239b383293657a7cf4d3df81421b6
    #[test]
    fn duration_under_a_minute_is_seconds_to_one_decimal() {
        assert_eq!(duration(12.34), "12.3s");
    }

    // @lfy def/cli/style.lfy:duration#duration:duration:5cceda616cd0c4c6cce21639c18f4b5f3b5646b449845b99d8c12cec76451e09
    // @lfy def/cli/style.lfy:duration#duration:duration:8ca88562d8c6e5dec77a957242a697f30dc0f73ac0cede5f8c4b237a76d0c419
    #[test]
    fn every_part_of_a_duration_is_truncated_and_never_rounded_up() {
        assert_eq!(duration(59.99), "59.9s");
    }

    // @lfy def/cli/style.lfy:duration#duration:duration:2560969ace129c52e954c736b432cfff0494d09b4f1b5328a38a39902a50c310
    // @lfy def/cli/style.lfy:duration#duration:duration:45752ec47a9540a84d457cb57940b15769f2ed08a0ea2868411d6a8491d377ad
    #[test]
    fn duration_of_a_minute_or_more_is_minutes_and_seconds() {
        assert_eq!(duration(125.0), "2m05s");
        assert_eq!(duration(60.0), "1m00s");
    }

    // @lfy def/cli/style.lfy:duration#duration:duration:3d4b8564c921f0dcc9152446962bec9fd969cc6478793420bb9476d1c2e477f7
    // @lfy def/cli/style.lfy:duration#duration:duration:dd742c1c3cc3a97b80b6de90cc6c051d50a547c403eede626647736f71a1aac3
    #[test]
    fn duration_of_an_hour_or_more_is_hours_and_minutes() {
        assert_eq!(duration(3725.0), "1h02m");
        assert_eq!(duration(3600.0), "1h00m");
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:26e771016ac5d78412265b6ce2f642e945bad5ab4c390694540e314c9e2db6d3
    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:80d019b710b01db5e2ec24642071f7f1e01b6845806c26d4c35ae43b1a37bb87
    #[test]
    fn accepted_reviewed_and_global_reviewed_are_success() {
        assert_eq!(tone_of(Step::Accepted), Tone::Success);
        assert_eq!(tone_of(Step::Reviewed), Tone::Success);
        assert_eq!(tone_of(Step::GlobalReviewed), Tone::Success);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:398ba084d601d05be00d95e4cb7fa05f7d4b9d5b77ada16eaee2e88e9ecb9258
    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:7c99092c688d2e1c6afefb08fe08b45571d67677d1ae1f96b868c85f53c58dbb
    #[test]
    fn rejected_and_failed_are_failure() {
        assert_eq!(tone_of(Step::Rejected), Tone::Failure);
        assert_eq!(tone_of(Step::Failed), Tone::Failure);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:64ef0ccf8dc53c748db7e22025891b0e5700055323e7c72d20738955e09a9822
    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:1d1d00113b3b3f23ccc6f043b93c4a0e372cafc331cedbbf972a0cdc45923fd5
    #[test]
    fn retrying_blocked_and_clarification_are_warning() {
        assert_eq!(tone_of(Step::Clarification), Tone::Warning);
        assert_eq!(tone_of(Step::Retrying), Tone::Warning);
        assert_eq!(tone_of(Step::Blocked), Tone::Warning);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:e62a94e69d89b6cf00197a96f8e054019c8bc20c03f01ea04442e78b3bdfb462
    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:bbca4f24023ee9e8c8a9cf9a5d9f763c46b4237ee5bb9566ab980120014c4409
    #[test]
    fn the_steps_under_way_are_active() {
        assert_eq!(tone_of(Step::Compiling), Tone::Active);
        assert_eq!(tone_of(Step::Requesting), Tone::Active);
        assert_eq!(tone_of(Step::Checking), Tone::Active);
        assert_eq!(tone_of(Step::Verifying), Tone::Active);
        assert_eq!(tone_of(Step::GlobalVerifying), Tone::Active);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:58836dc724cf2963083aa70014885f28a3b6370d6f123ee57956f0b156ad301e
    #[test]
    fn planned_and_finished_are_subject() {
        assert_eq!(tone_of(Step::Planned), Tone::Subject);
        assert_eq!(tone_of(Step::Finished), Tone::Subject);
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:f7a6ac7720627542ea682c531652973a124ab72f22c3acd82dc2b7e51a5c1c9c
    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:c72d261846854334790bdbce4677f86dcb6648858138ef897da2aa026e274640
    #[test]
    fn accepted_reviewed_and_global_reviewed_are_a_check() {
        assert_eq!(glyph_of(Step::Accepted), "✓");
        assert_eq!(glyph_of(Step::Reviewed), "✓");
        assert_eq!(glyph_of(Step::GlobalReviewed), "✓");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:fea38d82fe4a7ac92d163cfb7f1268f6f986aa94cd99f2e727006e6c6f0b6961
    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:da6a8a46c2c6f5b8f318dc28656257270d35a12b752260f5d7b691075022951b
    #[test]
    fn rejected_and_failed_are_a_cross() {
        assert_eq!(glyph_of(Step::Failed), "✗");
        assert_eq!(glyph_of(Step::Rejected), "✗");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:e121e7b63e58ebedb21c162928a8e56a1198652788186652f3df7180185086ed
    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:ff36ebe0ff46f6923c7a8c1123617f2ad6d7b5c219c1e3dc8ceef03e7e811d92
    #[test]
    fn compiling_verifying_and_global_verifying_are_a_triangle() {
        assert_eq!(glyph_of(Step::Compiling), "▸");
        assert_eq!(glyph_of(Step::Verifying), "▸");
        assert_eq!(glyph_of(Step::GlobalVerifying), "▸");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:f5fc9102b729a1695599d9f1c60d947a06f66947aa31a72af718d62b08a0ba8c
    #[test]
    fn planned_and_finished_are_a_diamond() {
        assert_eq!(glyph_of(Step::Planned), "◆");
        assert_eq!(glyph_of(Step::Finished), "◆");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:18c86a248eae3e7333c065b9e2dc69023a465f7c5112c985230ddccaa2cc996c
    #[test]
    fn requesting_and_checking_are_a_dot() {
        assert_eq!(glyph_of(Step::Requesting), "·");
        assert_eq!(glyph_of(Step::Checking), "·");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:d4054bc957dfed7e7c21e21c6304298b7fd86cfad457a667298ef0af7cc5438d
    #[test]
    fn retrying_blocked_and_clarification_have_a_glyph_of_their_own() {
        assert_eq!(glyph_of(Step::Retrying), "↻");
        assert_eq!(glyph_of(Step::Blocked), "■");
        assert_eq!(glyph_of(Step::Clarification), "?");
        // One character, so every step column is one wide.
        for step in STEPS {
            assert_eq!(width(glyph_of(step)), 1, "{}", step.name());
        }
    }

    // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:b887b8bbc3bf0d2b35204bb8d40567ee15b8359515d842f0b6aefce7c1514016
    // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:988a081cdf2488795bd371340bfa3af51271217d1d6f89ff4be4c756bb79ea73
    #[test]
    fn a_severity_has_the_tone_it_means() {
        assert_eq!(severity_tone(Severity::Error), Tone::Failure);
        assert_eq!(severity_tone(Severity::Warning), Tone::Warning);
        assert_eq!(severity_tone(Severity::Information), Tone::Active);
    }

    // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:e7820cabe74cc4201ffdb24b7474cf75c4637ae197453aa49d6ec67a0e8c2a38
    #[test]
    fn a_hint_is_active() {
        assert_eq!(severity_tone(Severity::Hint), Tone::Active);
    }

    // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:48c63f43020ad019c2446d8b0fcdd7e4649b5cbc68a8f6ce859cbb3d2335d80b
    // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:b60eaf654eb94c310fcc43a7ae2efec00d4918c0f9ad0e6852dcc98f5aa08835
    #[test]
    fn a_status_has_the_tone_it_means() {
        assert_eq!(status_tone(ReviewStatus::Satisfied), Tone::Success);
        assert_eq!(status_tone(ReviewStatus::Unverifiable), Tone::Warning);
    }

    // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:59313569ee3ab4c04e0454da3401560e252509eca59d21dc061524c8abaed292
    #[test]
    fn a_violated_review_is_failure() {
        assert_eq!(status_tone(ReviewStatus::Violated), Tone::Failure);
    }

    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:28cde573f1e33dad6b6c8133c43fe701cc242358daa04c84ac138f95353bc234
    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:766ff7ed234740d82081af5018d2c64a3edcda40c6ecdc9e437c3e25256f5a99
    #[test]
    fn changed_requirements_and_dependency_are_warning() {
        assert_eq!(reason_tone(Some(Reason::Changed)), Tone::Warning);
        assert_eq!(reason_tone(Some(Reason::Requirements)), Tone::Warning);
        assert_eq!(reason_tone(Some(Reason::Dependency)), Tone::Warning);
    }

    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:8d6a1a88a7c0cd4d32dd28b82d1a681c15cfca1dae5ce97b36d3463f874a07bd
    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:6e3e72b4caa7cfe4a0b109f5211121030d44d883f3b9583151f1640e720c38a8
    #[test]
    fn a_violated_unit_is_failure() {
        assert_eq!(reason_tone(Some(Reason::Violated)), Tone::Failure);
    }

    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:692c8c2a7bd4e2e8b9e12899b0cffe1543db43a1dbeb701777c21d76f320927b
    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:6a45d395babe1a35f49fcc8e83830ef2477f6b524531a1baf31095cdffa95a09
    #[test]
    fn a_unit_that_is_up_to_date_is_muted() {
        assert_eq!(reason_tone(None), Tone::Muted);
    }

    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:8d6898f1de1b0a930511711a62d207dc605bbf10d23c194db8d0d5c99027adc9
    #[test]
    fn requested_is_active_and_fresh_is_success() {
        assert_eq!(reason_tone(Some(Reason::Requested)), Tone::Active);
        assert_eq!(reason_tone(Some(Reason::Fresh)), Tone::Success);
    }

    // @lfy def/cli/style.lfy:tally#tally:tally:d0304e1e1a879d6fbb15edea0c7f5612a2dc8e06e0314f9d56253d6a1544e92d
    // @lfy def/cli/style.lfy:tally#tally:tally:df353484a54814b80445678c577e6e3e4cf2405d8c9de995bb17acac86ca4d8b
    // @lfy def/cli/style.lfy:tally#tally:tally:d5560e01139c5b0cceeea67c7f5579d2dbd37213245e5f781499a076fdde4de1
    #[test]
    fn a_count_that_is_not_zero_is_painted_in_its_tone() {
        assert_eq!(tally(2, "rejected", Tone::Failure, true), "\u{1b}[31m2 rejected\u{1b}[0m");
        // The words are the count, a space, and the label, painted or not.
        assert_eq!(tally(2, "rejected", Tone::Failure, false), "2 rejected");
    }

    // @lfy def/cli/style.lfy:tally#tally:tally:99193b5bef282bd8e2f1340ab99b6f172e5abb8a11b4d354e823c86c4bb65a72
    // @lfy def/cli/style.lfy:tally#tally:tally:4cccc0e5af75e2d33b3b247f091056de3fae98a0aecb15e962e97b9e6ff06ebb
    #[test]
    fn a_count_of_zero_is_muted() {
        assert_eq!(tally(0, "rejected", Tone::Failure, true), "\u{1b}[2m0 rejected\u{1b}[0m");
    }

    // @lfy def/cli/style.lfy:tally#tally:tally:7ffd7bc4d190179038dd14069b99a6688dc195a93ebc42faa16cc1381c4b7544
    #[test]
    fn a_tally_on_a_stream_that_is_not_colored_is_plain() {
        assert_eq!(tally(0, "rejected", Tone::Failure, false), "0 rejected");
    }
}
