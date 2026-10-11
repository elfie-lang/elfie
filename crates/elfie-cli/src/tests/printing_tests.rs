// @lfy def/cli/main.lfy:main
//! Tests of how the CLI paints what it prints: the --color choice, which stream is painted,
//! and the messages it writes to standard error.

use super::*;

/// What --color asked for: the value it names, always for the bare flag, and auto when it
/// is not given.
// @lfy def/cli/main.lfy:main
#[test]
fn the_color_choice_is_what_was_asked_for() {
    // @lfy def/cli/main.lfy:main
    assert_eq!(color_choice(&parse_arguments(&args(&["--color", "never", "check"])).unwrap()), ColorChoice::Never);
    // @lfy def/cli/main.lfy:main
    assert_eq!(color_choice(&parse_arguments(&args(&["check", "--color=always"])).unwrap()), ColorChoice::Always);
    // The bare flag is read as always. @lfy def/cli/main.lfy:main
    assert_eq!(color_choice(&parse_arguments(&args(&["--color", "check"])).unwrap()), ColorChoice::Always);
    // Not given at all, the choice is auto. @lfy def/cli/main.lfy:main
    assert_eq!(color_choice(&parse_arguments(&args(&["check"])).unwrap()), ColorChoice::Auto);
}

/// With --json nothing is painted on either stream, whatever --color says.
// @lfy def/cli/main.lfy:main
#[test]
fn json_paints_nothing_and_always_paints_both_streams() {
    let invocation = parse_arguments(&args(&["check", "--json", "--color=always"])).unwrap();
    // @lfy def/cli/main.lfy:main
    let style = Paint::of(&invocation);
    assert!(!style.out && !style.err);
    // Each stream is decided apart, and always wins over the environment.
    // @lfy def/cli/main.lfy:main
    let invocation = parse_arguments(&args(&["check", "--color=always"])).unwrap();
    let style = Paint::of(&invocation);
    assert!(style.out && style.err);
    // @lfy def/cli/main.lfy:main
    let invocation = parse_arguments(&args(&["check", "--color=never"])).unwrap();
    let style = Paint::of(&invocation);
    assert!(!style.out && !style.err);
    // Arguments that spell no invocation are read the same way, so a usage message obeys
    // the --color choice the person wrote and --json. @lfy def/cli/main.lfy:main
    assert_eq!(choice_in(&args(&["--color=never", "frobnicate"])), ColorChoice::Never);
    assert_eq!(choice_in(&args(&["--color", "always", "frobnicate"])), ColorChoice::Always);
    // The bare flag is read as always, and nothing given at all is auto.
    // @lfy def/cli/main.lfy:main
    assert_eq!(choice_in(&args(&["--color", "frobnicate"])), ColorChoice::Always);
    assert_eq!(choice_in(&args(&["frobnicate"])), ColorChoice::Auto);
    // @lfy def/cli/main.lfy:main
    let style = Paint::of_arguments(&args(&["--color=never", "frobnicate"]));
    assert!(!style.out && !style.err);
    // @lfy def/cli/main.lfy:main
    let style = Paint::of_arguments(&args(&["--color", "always", "frobnicate"]));
    assert!(style.out && style.err);
    // With --json nothing is painted on either stream, whatever --color says, even when
    // the arguments are a usage error. @lfy def/cli/main.lfy:main
    let style = Paint::of_arguments(&args(&["--json", "check", "--color=sometimes"]));
    assert!(!style.out && !style.err);
    // @lfy def/cli/main.lfy:main
    let style = Paint::of_arguments(&args(&["--json", "--color", "always", "frobnicate"]));
    assert!(!style.out && !style.err);
}

/// A message the CLI prints to standard error begins with elfie: painted failure, and a
/// path that begins it is subject.
// @lfy def/cli/main.lfy:main
#[test]
fn a_message_to_standard_error_names_itself() {
    // The words are the same painted or not, so a script reads what it read before.
    // @lfy def/cli/main.lfy:main
    complain(false, None, "frobnicate is not a command");
    // @lfy def/cli/main.lfy:main
    assert_eq!(complaint(false, None, "frobnicate is not a command"), "elfie: frobnicate is not a command");
    // @lfy def/cli/main.lfy:main
    complain(true, Some("def/a.lfy"), "no such file");
    // @lfy def/cli/main.lfy:main
    assert_eq!(complaint(false, Some("def/a.lfy"), "no such file"), "elfie: def/a.lfy: no such file");
    // elfie: is painted failure and a path that begins the message is subject.
    // @lfy def/cli/main.lfy:main
    let painted = complaint(true, Some("def/a.lfy"), "no such file");
    assert!(painted.starts_with(&paint("elfie:", Tone::Failure, true)), "{painted:?}");
    assert!(painted.contains(&paint("def/a.lfy", Tone::Subject, true)), "{painted:?}");
    // Painting leaves the width of the prefix as it was.
    // @lfy def/cli/main.lfy:main
    assert_eq!(width(&paint("elfie:", Tone::Failure, true)), width("elfie:"));
    assert_eq!(width(&painted), width(&complaint(false, Some("def/a.lfy"), "no such file")));
}
