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

/// When the CLI colors what it prints; each value is the word `--color` takes.
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

/// One of the two text streams a process writes, as the standard library's
/// `Process.Stream` names them: the two the CLI decides color for apart.
// @lfy def/cli/style.lfy:colorsOn
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Output,
    Error,
}

impl Stream {
    /// Whether the stream is written to a terminal rather than a file or a pipe.
    // @lfy def/cli/style.lfy:colorsOn
    fn is_terminal(self) -> bool {
        match self {
            Stream::Output => std::io::stdout().is_terminal(),
            Stream::Error => std::io::stderr().is_terminal(),
        }
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
/// `choice` is what `--color` asked for; `stream` is the stream the text is printed to, so
/// standard output and standard error are decided apart.
// @lfy def/cli/style.lfy:colorsOn
pub fn colors_on(choice: ColorChoice, stream: Stream) -> bool {
    let variable = |name: &str| std::env::var(name).ok();
    // @lfy def/cli/style.lfy:colorsOn
    colored(
        choice,
        variable("NO_COLOR").as_deref(),
        variable("TERM").as_deref(),
        variable("CLICOLOR_FORCE").as_deref(),
        // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:a25c587b03fe66a831b8c8a99ceb6fcbed28fb6856146da98ef190b32680e967
        move || stream.is_terminal(),
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
        // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:cd544380f546397d1071f89849f1daa1878e747601e015261b0c6b6ba3029a4f
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
            // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:5ad3b31609f776334b71beceb50e4ed4c72f51139b57b31677bec1ba7b8ac634
            if force.is_some_and(|value| !value.is_empty() && value != "0") {
                return true;
            }
            // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:a25c587b03fe66a831b8c8a99ceb6fcbed28fb6856146da98ef190b32680e967
            is_terminal()
        }
    }
}

/// Text wrapped in the escapes that color it by its tone, or left as it is.
///
/// `on` is whether the stream it goes to is colored, as [`colors_on`] decides. The reset
/// always follows the painted text, so no color runs past it, and the escapes take no
/// columns, so a painted column lines up as its plain text would.
// @lfy def/cli/style.lfy:paint
pub fn paint(text: &str, tone: Tone, on: bool) -> String {
    // A stream that is not colored is the text unchanged.
    // @lfy def/cli/style.lfy:paint#paint:paint:a71b34754c2a1f6fb1875c94ac0f67275289f340fc1aa2289d16aa2b753d50ca
    // Nothing to color is the text unchanged.
    // @lfy def/cli/style.lfy:paint#paint:paint:887caee4bf900a333c30d4891d2ac0cd87f1e9b7e054d0802c5ceae73aff0def
    if !on || text.is_empty() {
        return text.to_string();
    }
    // The escape character, [, the value of the tone, m, the text, and the escape character,
    // [, 0, m: the reset always follows, so no color runs past the text.
    // @lfy def/cli/style.lfy:paint#paint:paint:908e76236a6f4a230ad82b290f0e2fb7b35a7cf4b83bf2aa7290709f3e7e1d55
    format!("{ESCAPE}[{}m{text}{ESCAPE}[0m", tone.value())
}

/// How many columns text takes in a terminal, painted or not: what [`padded`] fills up to,
/// and what tells a caller which of several texts is the longest.
///
/// Characters are counted, not bytes, and every escape sequence that begins with the escape
/// character and `[` and ends with `m` is skipped, so painted and plain text measure the
/// same.
// @lfy def/cli/style.lfy:padded
pub fn width(text: &str) -> usize {
    let mut columns = 0;
    let mut characters = text.chars().peekable();
    while let Some(character) = characters.next() {
        // An escape sequence takes no columns.
        // @lfy def/cli/style.lfy:padded#padded:padded:2253ff73d22ed1d25a013fa11c2998d81885467ef3e649a57d0ad9f7c8a09555
        if character == ESCAPE && characters.peek() == Some(&'[') {
            characters.next();
            for inner in characters.by_ref() {
                if inner == 'm' {
                    break;
                }
            }
            continue;
        }
        // Each character takes one column, however many bytes it has.
        // @lfy def/cli/style.lfy:padded#padded:padded:0cc7a759efec56a39fec3ff1c04cb4db06c81adae743cd1d1b5cc92548386ee1
        columns += 1;
    }
    columns
}

/// Text followed by spaces up to a width, so the columns after it line up; text already that
/// wide or wider is returned unchanged, never cut.
// @lfy def/cli/style.lfy:padded
pub fn padded(text: &str, columns: usize) -> String {
    let have = width(text);
    // @lfy def/cli/style.lfy:padded#padded:padded:24bf222383423ca110cd5c6379c1e63c4a77daf3a3798e75c73e43044a118ef8
    if have >= columns {
        return text.to_string();
    }
    // @lfy def/cli/style.lfy:padded#padded:padded:8246266464e0a3704c9fd5653df6b5c7c806e0d53455d48116d131042168dac7
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
    // @lfy def/cli/style.lfy:duration#duration:duration:3fe7dc4f6bad3b5f682e726aa5513053726b47f2a663ac50b7f2dae9b9d906e4
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
        // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:972c80f035f3bc6703b93bce30b51744ec94bf0987488bd03c22bd57f0593ea1
        Step::Accepted | Step::Reviewed | Step::Merged | Step::GlobalReviewed => Tone::Success,
        // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:7a3092b23685652356ef40136c995e18bdcb5ef3b54278d351b792cab9d62fbf
        Step::Rejected | Step::Failed => Tone::Failure,
        // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:43a01238d78e009d831e15472c8fecf2a6c77026e5f464e4a76ff30e09704b22
        Step::Touched | Step::Stubbed | Step::Retrying | Step::Undone | Step::Blocked | Step::Clarification => {
            Tone::Warning
        }
        // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:842e69696e0cc80aa29d2a1bbb38fc9ab229947c878b47e0b68164f98f30aa46
        Step::Requesting
        | Step::Resuming
        | Step::Relocated
        | Step::Started
        | Step::Rewording
        | Step::Building
        | Step::Compiling
        | Step::Checking
        | Step::Verifying
        | Step::GlobalVerifying
        | Step::FinalGate
        | Step::Merging => Tone::Active,
        // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:a27712c13d5dd0df10d6f286a820faaa7ab5ef5ae98e7addfe626e43c7ba3be7
        Step::Planned | Step::Finished => Tone::Subject,
    }
}

/// One character that marks a progress line's step.
///
/// Each step has a glyph, so the eye finds the outcomes in a long compile without reading.
// @lfy def/cli/style.lfy:glyphOf
pub fn glyph_of(step: Step) -> &'static str {
    match step {
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:fac01e3084be271eaea56a78e97d67e2f31975cd28b771f7b373a1b8f8eea90a
        Step::Planned | Step::Finished => "◆",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:ca3ce6724ba3d007f694a95afc4fd6b3ff0231cbf0e7c8ca8e2719a371aa5876
        Step::Requesting | Step::Resuming | Step::Relocated | Step::Rewording | Step::Checking => "·",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:834c8102d686988039c29321e6138ce3e53b43e1e1a5934976ad2a1c4118d139
        Step::Started
        | Step::Building
        | Step::Compiling
        | Step::Verifying
        | Step::GlobalVerifying
        | Step::FinalGate
        | Step::Merging => "▸",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:7ba0eaadbcbd83b254658d5dbf35be911598a84c800f9ded6e518379216a9919
        Step::Accepted | Step::Reviewed | Step::Merged | Step::GlobalReviewed => "✓",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:2469217fa6b8df9b0a1aebc6d1249afceada2fd0f1ca56a99cdba48503f1094c
        Step::Rejected | Step::Failed => "✗",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:1c2c367ec30ab165c3232e2e9eeba1b807605a3aea2d98a01c83084e33539f68
        Step::Touched => "✎",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:6e053fb3f3135ac97cf1465c89bbe75a589870bd0df5aa26c4dd86842610c70d
        Step::Stubbed => "○",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:4ced2f05657b9ffbadefd96026dd4b97acc62dcdda17539bbdf89cd9fa3ca17c
        Step::Retrying | Step::Undone => "↻",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:2b68ddec02b3857f759454908b6536c3e8b3fc9eab54f2517660b9a24fa7d40e
        Step::Blocked => "■",
        // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:d3da3373fd7152e3807d83a5aec0e352db8e8b2c54d6d0623fc515eaeaf937da
        Step::Clarification => "?",
    }
}

/// The tone a diagnostic's severity is painted in.
// @lfy def/cli/style.lfy:severityTone
pub fn severity_tone(severity: Severity) -> Tone {
    match severity {
        // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:6aa259ecfa3a4402af73a3d2711a299f29be7292e37ad3240a2c15bd518c7535
        Severity::Error => Tone::Failure,
        // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:068fe8b78a1d721c071d0990013048e00b0432eea7cc9cc97b9afe24f7f87f11
        Severity::Warning => Tone::Warning,
        // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:fcfa327db29ade75bdc1f35a572073de41cb1b08b69befea01c4cafefb760888
        Severity::Information | Severity::Hint => Tone::Active,
    }
}

/// The tone a review's status is painted in.
// @lfy def/cli/style.lfy:statusTone
pub fn status_tone(status: ReviewStatus) -> Tone {
    match status {
        // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:c24f9f3c7757160b932c3f9d9582d7cf05c9fedad083bb2493348ce05446d8c6
        ReviewStatus::Satisfied => Tone::Success,
        // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:981a47dfe41157a73faec41c3e843c11875d9344170d8d647af7824b03ddb8c6
        ReviewStatus::Violated => Tone::Failure,
        // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:730e8685ff5d8c9ce873fb71fd6781c7e8981e167f121771f8f30972ccdab6b1
        ReviewStatus::Unverifiable => Tone::Warning,
    }
}

/// The tone a unit's reason, or up to date, is painted in; `None`, printed as up to date, is
/// muted.
// @lfy def/cli/style.lfy:reasonTone
pub fn reason_tone(reason: Option<Reason>) -> Tone {
    match reason {
        // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:f04315f94350f0bef98ff50819e968748e5c2f083dceda0c6493aae64397be61
        Some(Reason::Requested) => Tone::Active,
        // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:f27ff452bb57d8763b5de9c9e6038486b6c8c90d53d811f4c80848a3d0c338c6
        Some(Reason::Fresh) => Tone::Success,
        // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:7a6569c3d2c2f3abe4d43240ef7f179f37a994602b4504c9aa4484f60d72868c
        Some(Reason::Changed | Reason::Requirements | Reason::Dependency | Reason::Touched) => Tone::Warning,
        // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:75be1cb8a9f16327a21946c1232feac3af781c105e0528e0b0b537dd6c7ad085
        Some(Reason::Violated) => Tone::Failure,
        // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:dcd627388a7601c2b33f1da9e26b5f10a796a79110967bf946361afce58a56de
        None => Tone::Muted,
    }
}

/// A count and what it counts, painted only when it is worth noticing.
///
/// The text is the count, a space, and the label; a count that is not 0 is painted in the
/// tone, and 0 is muted, so a summary shows at a glance which counts matter.
// @lfy def/cli/style.lfy:tally
pub fn tally(count: usize, label: &str, tone: Tone, on: bool) -> String {
    let text = format!("{count} {label}");
    // @lfy def/cli/style.lfy:tally#tally:tally:22ccbf3f063d8cff61ae44b8bc0c48122eac7cbd166657279f6a9ddbc3a4aa47
    // @lfy def/cli/style.lfy:tally#tally:tally:808090fb00651fdad96557306be003862711eb272672639c39f0e28851b18aaa
    paint(&text, if count == 0 { Tone::Muted } else { tone }, on)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every step, so that a test can walk them all.
    const STEPS: [Step; 22] = [
        Step::Planned,
        Step::Requesting,
        Step::Resuming,
        Step::Rewording,
        Step::Building,
        Step::Compiling,
        Step::Checking,
        Step::Accepted,
        Step::Touched,
        Step::Stubbed,
        Step::Rejected,
        Step::Verifying,
        Step::Reviewed,
        Step::FinalGate,
        Step::GlobalVerifying,
        Step::GlobalReviewed,
        Step::Merging,
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

    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:ff6531ae54a65c432ac61bfcfec9bed47ec3035c12287116d69a25c95fcfe8be
    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:cd544380f546397d1071f89849f1daa1878e747601e015261b0c6b6ba3029a4f
    #[test]
    fn always_is_colored_whatever_the_environment_says() {
        assert!(colors_on(ColorChoice::Always, Stream::Output));
        assert!(colored(ColorChoice::Always, Some("1"), Some("dumb"), None, never));
    }

    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:d7a9fd32a01c9a0c828826be76e0eb10eeca5a3d0a1dfe80b4288ecfd480676e
    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:51b5bc7dcc299fd9e0162c246879a8a7677937b08cdb8efab6713912b41d712f
    #[test]
    fn never_is_not_colored() {
        assert!(!colors_on(ColorChoice::Never, Stream::Error));
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

    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:5ad3b31609f776334b71beceb50e4ed4c72f51139b57b31677bec1ba7b8ac634
    #[test]
    fn clicolor_force_keeps_color_through_a_pager() {
        assert!(colored(ColorChoice::Auto, None, Some("xterm"), Some("1"), never));
        assert!(colored(ColorChoice::Auto, Some(""), None, Some("yes"), never));
    }

    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:c012147bba6d569ae6683005e50792f5c5e585587e8abb223960264ef3aaf77f
    // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:a25c587b03fe66a831b8c8a99ceb6fcbed28fb6856146da98ef190b32680e967
    #[test]
    fn auto_with_nothing_forcing_it_is_what_the_stream_is() {
        // Standard output, piped with CLICOLOR_FORCE unset, is not colored.
        assert!(!colored(ColorChoice::Auto, None, Some("xterm"), None, never));
        assert!(!colored(ColorChoice::Auto, None, None, Some(""), never));
        assert!(!colored(ColorChoice::Auto, None, None, Some("0"), never));
        assert!(colored(ColorChoice::Auto, None, None, None, always));
        // Each stream is asked about itself, so the two are decided apart.
        // @lfy def/cli/style.lfy:colorsOn#colorsOn:colorsOn:a25c587b03fe66a831b8c8a99ceb6fcbed28fb6856146da98ef190b32680e967
        assert_eq!(colors_on(ColorChoice::Auto, Stream::Output), Stream::Output.is_terminal());
        assert_eq!(colors_on(ColorChoice::Auto, Stream::Error), Stream::Error.is_terminal());
    }

    // @lfy def/cli/style.lfy:paint#paint:paint:de381fd52ede1936e22b7a7e414805c9ffc5e7f5f98bb6e0604f5293bd584bd9
    // @lfy def/cli/style.lfy:paint#paint:paint:908e76236a6f4a230ad82b290f0e2fb7b35a7cf4b83bf2aa7290709f3e7e1d55
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

    // @lfy def/cli/style.lfy:padded#padded:padded:26429a29e6c03161f9989e316d03d7775c25e490bc1cde9771e058db8d3b9359
    // @lfy def/cli/style.lfy:padded#padded:padded:8246266464e0a3704c9fd5653df6b5c7c806e0d53455d48116d131042168dac7
    #[test]
    fn padded_appends_spaces_up_to_the_width() {
        assert_eq!(padded("ab", 4), "ab  ");
        assert_eq!(width(&padded("ab", 4)), 4);
    }

    // @lfy def/cli/style.lfy:padded#padded:padded:d13e67da436a0aaa2823e82520f26b91a24330e945f857cb910c002a6078a097
    // @lfy def/cli/style.lfy:padded#padded:padded:24bf222383423ca110cd5c6379c1e63c4a77daf3a3798e75c73e43044a118ef8
    #[test]
    fn padded_never_cuts() {
        assert_eq!(padded("abcdef", 4), "abcdef");
        assert_eq!(padded("ab", 2), "ab");
    }

    // @lfy def/cli/style.lfy:padded#padded:padded:cb1f3b1f598c5d084090f45e30f5eddee904bcc25fb6e54a99b38058e89a8210
    // @lfy def/cli/style.lfy:padded#padded:padded:2253ff73d22ed1d25a013fa11c2998d81885467ef3e649a57d0ad9f7c8a09555
    #[test]
    fn an_escape_sequence_takes_no_columns() {
        assert_eq!(padded("\u{1b}[1mab\u{1b}[0m", 4), "\u{1b}[1mab\u{1b}[0m  ");
        assert_eq!(width("\u{1b}[1mbatch\u{1b}[0m"), 5);
        // So a painted column lines up as its plain text would.
        // @lfy def/cli/style.lfy:padded#padded:padded:2253ff73d22ed1d25a013fa11c2998d81885467ef3e649a57d0ad9f7c8a09555
        for tone in [Tone::Success, Tone::Failure, Tone::Warning, Tone::Active, Tone::Subject, Tone::Muted] {
            assert_eq!(width(&paint("batch", tone, true)), width("batch"));
        }
        assert_eq!(width(&padded(&paint("ab", Tone::Subject, true), 4)), 4);
    }

    // @lfy def/cli/style.lfy:padded#padded:padded:4b684620f67ebc661bcabb5995a649fa4519006dc7a42fe29f75544a5b3becf2
    // @lfy def/cli/style.lfy:padded#padded:padded:0cc7a759efec56a39fec3ff1c04cb4db06c81adae743cd1d1b5cc92548386ee1
    #[test]
    fn each_character_takes_one_column_however_many_bytes_it_has() {
        assert_eq!(padded("✓ ok", 5), "✓ ok ");
        assert_eq!(width("✓ ok"), 4);
        assert_eq!(width("batch"), 5);
        assert_eq!(width(""), 0);
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
    // @lfy def/cli/style.lfy:duration#duration:duration:3fe7dc4f6bad3b5f682e726aa5513053726b47f2a663ac50b7f2dae9b9d906e4
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
    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:972c80f035f3bc6703b93bce30b51744ec94bf0987488bd03c22bd57f0593ea1
    #[test]
    fn accepted_reviewed_and_global_reviewed_are_success() {
        assert_eq!(tone_of(Step::Accepted), Tone::Success);
        assert_eq!(tone_of(Step::Reviewed), Tone::Success);
        assert_eq!(tone_of(Step::Merged), Tone::Success);
        assert_eq!(tone_of(Step::GlobalReviewed), Tone::Success);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:67b2330e1b2afc0e6a6d1e741b1bfd5a8403efe388ce23cf0428330b8b2cff24
    #[test]
    fn merged_is_success() {
        assert_eq!(tone_of(Step::Merged), Tone::Success);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:398ba084d601d05be00d95e4cb7fa05f7d4b9d5b77ada16eaee2e88e9ecb9258
    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:7a3092b23685652356ef40136c995e18bdcb5ef3b54278d351b792cab9d62fbf
    #[test]
    fn rejected_and_failed_are_failure() {
        assert_eq!(tone_of(Step::Rejected), Tone::Failure);
        assert_eq!(tone_of(Step::Failed), Tone::Failure);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:64ef0ccf8dc53c748db7e22025891b0e5700055323e7c72d20738955e09a9822
    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:43a01238d78e009d831e15472c8fecf2a6c77026e5f464e4a76ff30e09704b22
    #[test]
    fn retrying_blocked_and_clarification_are_warning() {
        assert_eq!(tone_of(Step::Clarification), Tone::Warning);
        assert_eq!(tone_of(Step::Retrying), Tone::Warning);
        assert_eq!(tone_of(Step::Blocked), Tone::Warning);
        assert_eq!(tone_of(Step::Touched), Tone::Warning);
        assert_eq!(tone_of(Step::Stubbed), Tone::Warning);
        assert_eq!(tone_of(Step::Undone), Tone::Warning);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:52c8fcfb81d0809dabbc339c6d67003eaeda7ee9b608d54ab4e462c3e0f255ed
    #[test]
    fn undone_is_warning() {
        assert_eq!(tone_of(Step::Undone), Tone::Warning);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:e62a94e69d89b6cf00197a96f8e054019c8bc20c03f01ea04442e78b3bdfb462
    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:842e69696e0cc80aa29d2a1bbb38fc9ab229947c878b47e0b68164f98f30aa46
    #[test]
    fn the_steps_under_way_are_active() {
        assert_eq!(tone_of(Step::Compiling), Tone::Active);
        assert_eq!(tone_of(Step::Requesting), Tone::Active);
        assert_eq!(tone_of(Step::Resuming), Tone::Active);
        assert_eq!(tone_of(Step::Relocated), Tone::Active);
        assert_eq!(tone_of(Step::Started), Tone::Active);
        assert_eq!(tone_of(Step::Rewording), Tone::Active);
        assert_eq!(tone_of(Step::Building), Tone::Active);
        assert_eq!(tone_of(Step::Checking), Tone::Active);
        assert_eq!(tone_of(Step::Verifying), Tone::Active);
        assert_eq!(tone_of(Step::GlobalVerifying), Tone::Active);
        assert_eq!(tone_of(Step::FinalGate), Tone::Active);
        assert_eq!(tone_of(Step::Merging), Tone::Active);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:e7b45aec008f19c9d28b66101529bd1e71bbf2f5b2da8a0b527de2c10b2100ee
    #[test]
    fn merging_is_active() {
        assert_eq!(tone_of(Step::Merging), Tone::Active);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:acdcb5465a8eee43dbfd7bd0b7597ca384b436d10016e420c5d99aa3fc441ff6
    #[test]
    fn stubbed_is_warning() {
        assert_eq!(tone_of(Step::Stubbed), Tone::Warning);
    }

    // @lfy def/cli/style.lfy:toneOf#toneOf:toneOf:a27712c13d5dd0df10d6f286a820faaa7ab5ef5ae98e7addfe626e43c7ba3be7
    #[test]
    fn planned_and_finished_are_subject() {
        assert_eq!(tone_of(Step::Planned), Tone::Subject);
        assert_eq!(tone_of(Step::Finished), Tone::Subject);
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:f7a6ac7720627542ea682c531652973a124ab72f22c3acd82dc2b7e51a5c1c9c
    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:7ba0eaadbcbd83b254658d5dbf35be911598a84c800f9ded6e518379216a9919
    #[test]
    fn accepted_reviewed_and_global_reviewed_are_a_check() {
        assert_eq!(glyph_of(Step::Accepted), "✓");
        assert_eq!(glyph_of(Step::Reviewed), "✓");
        assert_eq!(glyph_of(Step::Merged), "✓");
        assert_eq!(glyph_of(Step::GlobalReviewed), "✓");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:fea38d82fe4a7ac92d163cfb7f1268f6f986aa94cd99f2e727006e6c6f0b6961
    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:2469217fa6b8df9b0a1aebc6d1249afceada2fd0f1ca56a99cdba48503f1094c
    #[test]
    fn rejected_and_failed_are_a_cross() {
        assert_eq!(glyph_of(Step::Failed), "✗");
        assert_eq!(glyph_of(Step::Rejected), "✗");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:e121e7b63e58ebedb21c162928a8e56a1198652788186652f3df7180185086ed
    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:834c8102d686988039c29321e6138ce3e53b43e1e1a5934976ad2a1c4118d139
    #[test]
    fn building_compiling_verifying_and_global_verifying_are_a_triangle() {
        assert_eq!(glyph_of(Step::Started), "▸");
        assert_eq!(glyph_of(Step::Building), "▸");
        assert_eq!(glyph_of(Step::Compiling), "▸");
        assert_eq!(glyph_of(Step::Verifying), "▸");
        assert_eq!(glyph_of(Step::GlobalVerifying), "▸");
        assert_eq!(glyph_of(Step::Merging), "▸");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:a464de6ebce29632df074ad0237a127abb1fe993a8ce34baf6556dfe7ea153e1
    #[test]
    fn the_final_gate_is_a_triangle() {
        assert_eq!(glyph_of(Step::FinalGate), "▸");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:4839387269bb0fe2cb0535c86bbd84eac2c8a6405f07e349718dc29fd4101061
    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:6e053fb3f3135ac97cf1465c89bbe75a589870bd0df5aa26c4dd86842610c70d
    #[test]
    fn stubbed_is_a_circle() {
        assert_eq!(glyph_of(Step::Stubbed), "○");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:1c2c367ec30ab165c3232e2e9eeba1b807605a3aea2d98a01c83084e33539f68
    #[test]
    fn touched_is_a_pencil() {
        assert_eq!(glyph_of(Step::Touched), "✎");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:fac01e3084be271eaea56a78e97d67e2f31975cd28b771f7b373a1b8f8eea90a
    #[test]
    fn planned_and_finished_are_a_diamond() {
        assert_eq!(glyph_of(Step::Planned), "◆");
        assert_eq!(glyph_of(Step::Finished), "◆");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:ca3ce6724ba3d007f694a95afc4fd6b3ff0231cbf0e7c8ca8e2719a371aa5876
    #[test]
    fn requesting_resuming_rewording_and_checking_are_a_dot() {
        assert_eq!(glyph_of(Step::Requesting), "·");
        assert_eq!(glyph_of(Step::Resuming), "·");
        assert_eq!(glyph_of(Step::Relocated), "·");
        assert_eq!(glyph_of(Step::Rewording), "·");
        assert_eq!(glyph_of(Step::Checking), "·");
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:4ced2f05657b9ffbadefd96026dd4b97acc62dcdda17539bbdf89cd9fa3ca17c
    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:2b68ddec02b3857f759454908b6536c3e8b3fc9eab54f2517660b9a24fa7d40e
    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:d3da3373fd7152e3807d83a5aec0e352db8e8b2c54d6d0623fc515eaeaf937da
    #[test]
    fn retrying_blocked_and_clarification_have_a_glyph_of_their_own() {
        assert_eq!(glyph_of(Step::Retrying), "↻");
        assert_eq!(glyph_of(Step::Undone), "↻");
        assert_eq!(glyph_of(Step::Blocked), "■");
        assert_eq!(glyph_of(Step::Clarification), "?");
        // One character, so every step column is one wide.
        for step in STEPS {
            assert_eq!(width(glyph_of(step)), 1, "{}", step.name());
        }
    }

    // @lfy def/cli/style.lfy:glyphOf#glyphOf:glyphOf:5b058e1324cc149ae766c3ef3d88290215451e6e6dedc5ca7a3aae1fb22b1b3f
    #[test]
    fn undone_is_a_loop() {
        assert_eq!(glyph_of(Step::Undone), "↻");
    }

    // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:b887b8bbc3bf0d2b35204bb8d40567ee15b8359515d842f0b6aefce7c1514016
    // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:6aa259ecfa3a4402af73a3d2711a299f29be7292e37ad3240a2c15bd518c7535
    // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:068fe8b78a1d721c071d0990013048e00b0432eea7cc9cc97b9afe24f7f87f11
    #[test]
    fn a_severity_has_the_tone_it_means() {
        assert_eq!(severity_tone(Severity::Error), Tone::Failure);
        assert_eq!(severity_tone(Severity::Warning), Tone::Warning);
        assert_eq!(severity_tone(Severity::Information), Tone::Active);
    }

    // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:e7820cabe74cc4201ffdb24b7474cf75c4637ae197453aa49d6ec67a0e8c2a38
    // @lfy def/cli/style.lfy:severityTone#severityTone:severityTone:fcfa327db29ade75bdc1f35a572073de41cb1b08b69befea01c4cafefb760888
    #[test]
    fn a_hint_is_active() {
        assert_eq!(severity_tone(Severity::Hint), Tone::Active);
        assert_eq!(severity_tone(Severity::Information), Tone::Active);
    }

    // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:48c63f43020ad019c2446d8b0fcdd7e4649b5cbc68a8f6ce859cbb3d2335d80b
    // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:c24f9f3c7757160b932c3f9d9582d7cf05c9fedad083bb2493348ce05446d8c6
    // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:730e8685ff5d8c9ce873fb71fd6781c7e8981e167f121771f8f30972ccdab6b1
    #[test]
    fn a_status_has_the_tone_it_means() {
        assert_eq!(status_tone(ReviewStatus::Satisfied), Tone::Success);
        assert_eq!(status_tone(ReviewStatus::Unverifiable), Tone::Warning);
    }

    // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:59313569ee3ab4c04e0454da3401560e252509eca59d21dc061524c8abaed292
    // @lfy def/cli/style.lfy:statusTone#statusTone:statusTone:981a47dfe41157a73faec41c3e843c11875d9344170d8d647af7824b03ddb8c6
    #[test]
    fn a_violated_review_is_failure() {
        assert_eq!(status_tone(ReviewStatus::Violated), Tone::Failure);
    }

    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:28cde573f1e33dad6b6c8133c43fe701cc242358daa04c84ac138f95353bc234
    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:7a6569c3d2c2f3abe4d43240ef7f179f37a994602b4504c9aa4484f60d72868c
    #[test]
    fn changed_requirements_and_dependency_are_warning() {
        assert_eq!(reason_tone(Some(Reason::Changed)), Tone::Warning);
        assert_eq!(reason_tone(Some(Reason::Requirements)), Tone::Warning);
        assert_eq!(reason_tone(Some(Reason::Dependency)), Tone::Warning);
        assert_eq!(reason_tone(Some(Reason::Touched)), Tone::Warning);
    }

    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:c40b50ee0774fe73f27222f7898e707cb502bbf1fff5c0a56e409cfa06ecf7ce
    #[test]
    fn a_touched_unit_is_warning() {
        assert_eq!(reason_tone(Some(Reason::Touched)), Tone::Warning);
    }

    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:8d6a1a88a7c0cd4d32dd28b82d1a681c15cfca1dae5ce97b36d3463f874a07bd
    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:75be1cb8a9f16327a21946c1232feac3af781c105e0528e0b0b537dd6c7ad085
    #[test]
    fn a_violated_unit_is_failure() {
        assert_eq!(reason_tone(Some(Reason::Violated)), Tone::Failure);
    }

    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:692c8c2a7bd4e2e8b9e12899b0cffe1543db43a1dbeb701777c21d76f320927b
    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:dcd627388a7601c2b33f1da9e26b5f10a796a79110967bf946361afce58a56de
    #[test]
    fn a_unit_that_is_up_to_date_is_muted() {
        assert_eq!(reason_tone(None), Tone::Muted);
    }

    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:f04315f94350f0bef98ff50819e968748e5c2f083dceda0c6493aae64397be61
    // @lfy def/cli/style.lfy:reasonTone#reasonTone:reasonTone:f27ff452bb57d8763b5de9c9e6038486b6c8c90d53d811f4c80848a3d0c338c6
    #[test]
    fn requested_is_active_and_fresh_is_success() {
        assert_eq!(reason_tone(Some(Reason::Requested)), Tone::Active);
        assert_eq!(reason_tone(Some(Reason::Fresh)), Tone::Success);
    }

    // @lfy def/cli/style.lfy:tally#tally:tally:d0304e1e1a879d6fbb15edea0c7f5612a2dc8e06e0314f9d56253d6a1544e92d
    // @lfy def/cli/style.lfy:tally#tally:tally:22ccbf3f063d8cff61ae44b8bc0c48122eac7cbd166657279f6a9ddbc3a4aa47
    #[test]
    fn a_count_that_is_not_zero_is_painted_in_its_tone() {
        assert_eq!(tally(2, "rejected", Tone::Failure, true), "\u{1b}[31m2 rejected\u{1b}[0m");
        // The words are the count, a space, and the label, painted or not.
        assert_eq!(tally(2, "rejected", Tone::Failure, false), "2 rejected");
    }

    // @lfy def/cli/style.lfy:tally#tally:tally:99193b5bef282bd8e2f1340ab99b6f172e5abb8a11b4d354e823c86c4bb65a72
    // @lfy def/cli/style.lfy:tally#tally:tally:808090fb00651fdad96557306be003862711eb272672639c39f0e28851b18aaa
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
