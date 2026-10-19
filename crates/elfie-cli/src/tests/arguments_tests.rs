// @lfy def/cli/main.lfy:parse
//! Tests of `parse`: the invocation the arguments spell, the root they name, and the usage
//! errors they make.

use super::*;

// @lfy def/cli/main.lfy:parse#parse:parse:9950bf574b6e14800ed642db3dc3aa89927c17ef13574689ea665810703ae049
// @lfy def/cli/main.lfy:parse#parse:parse:3874a32a6a9ce574d2416d5c0171397b2a5a6f3c2857cb5d98d62c92e1a25770
#[test]
fn check_with_no_arguments_parses() {
    let invocation = parse_arguments(&args(&["check"])).unwrap();
    assert_eq!(invocation.command, Command::Check);
    assert!(invocation.arguments.is_empty());
}

// @lfy def/cli/main.lfy:parse#parse:parse:92a69bd92ba69e35f172e62ee820fff5befe5e882a02dec0f3244e94f3e80d88
#[test]
fn the_root_is_the_nearest_directory_holding_the_manifest() {
    let invocation = parse_arguments(&args(&["check"])).unwrap();
    assert!(Path::new(&invocation.root).join("elfie.json").exists(), "{}", invocation.root);
}

// @lfy def/cli/main.lfy:parse#parse:parse:e3c290f038d675be7f56fc738628a4aa9ffa92e3b07e8784a6357fa8b2395385
// @lfy def/cli/main.lfy:parse#parse:parse:a10f50e751f0b52b43514389889c366b65d704b81d4894a77a28e783fcbcebfc
#[test]
fn the_tree_flag_is_the_tree_command() {
    let invocation = parse_arguments(&args(&["--tree", "def/a.lfy"])).unwrap();
    assert_eq!(invocation.command, Command::Tree);
    assert_eq!(invocation.arguments, vec!["def/a.lfy".to_string()]);
    // A command named as well still names itself.
    assert_eq!(parse_arguments(&args(&["--tree", "def/a.lfy", "--json"])).unwrap().command, Command::Tree);
}

// @lfy def/cli/main.lfy:parse#parse:parse:5f3d8a6fb3ccffa0749573417617024cf2ac816e469eea41263c10278b25db02
#[test]
fn nothing_naming_a_command_is_help() {
    assert_eq!(parse_arguments(&args(&[])).unwrap().command, Command::Help);
    assert_eq!(parse_arguments(&args(&["--json"])).unwrap().command, Command::Help);
    // --tree followed by no file names no command, so nothing that does not begin with a
    // dash is given and the command is help; --tree is not the last argument here.
    assert_eq!(parse_arguments(&args(&["--tree", "--json"])).unwrap().command, Command::Help);
}

/// An argument beginning with a single dash names no command: the first argument without a
/// dash still does, and with none the command is help.
// @lfy def/cli/main.lfy:parse#parse:parse:3874a32a6a9ce574d2416d5c0171397b2a5a6f3c2857cb5d98d62c92e1a25770
// @lfy def/cli/main.lfy:parse#parse:parse:5f3d8a6fb3ccffa0749573417617024cf2ac816e469eea41263c10278b25db02
#[test]
fn a_single_dash_argument_names_no_command() {
    // @lfy def/cli/main.lfy:parse#parse:parse:3874a32a6a9ce574d2416d5c0171397b2a5a6f3c2857cb5d98d62c92e1a25770
    let invocation = parse_arguments(&args(&["-x", "check"])).unwrap();
    assert_eq!(invocation.command, Command::Check);
    assert!(invocation.arguments.is_empty());
    // @lfy def/cli/main.lfy:parse#parse:parse:5f3d8a6fb3ccffa0749573417617024cf2ac816e469eea41263c10278b25db02
    assert_eq!(parse_arguments(&args(&["-q"])).unwrap().command, Command::Help);
}

// @lfy def/cli/main.lfy:parse#parse:parse:3fd497a063b547c5ecbba95bd47044f648030f147a69b7241615811b47f2f09d
// @lfy def/cli/main.lfy:parse#parse:parse:407b80d916b66360f99967d28e89ba7016a7c488369e0a70acdef6af3936646d
#[test]
fn an_unknown_command_is_a_usage_error() {
    let error = parse_arguments(&args(&["frobnicate"])).unwrap_err();
    assert!(error.contains("frobnicate"));
}

// @lfy def/cli/main.lfy:parse#parse:parse:3e415cc8c432995a2f20132ebe9faa1a51e783fa11b13cdc472e6a3089c09986
#[test]
fn an_option_taking_a_value_as_the_last_argument_is_a_usage_error() {
    for (arguments, option) in [
        (args(&["check", "--root"]), "--root"),
        (args(&["--tree"]), "--tree"),
        (args(&["compile", "--target"]), "--target"),
        (args(&["compile", "--jobs"]), "--jobs"),
    ] {
        let error = parse_arguments(&arguments).unwrap_err();
        assert!(error.contains(option), "{error}");
    }
    // With --help, -h, or --version among the arguments the command is named instead.
    assert_eq!(parse_arguments(&args(&["--help", "--root"])).unwrap().command, Command::Help);
    assert_eq!(parse_arguments(&args(&["-h", "--tree"])).unwrap().command, Command::Help);
    assert_eq!(parse_arguments(&args(&["--version", "--target"])).unwrap().command, Command::Version);
}

// @lfy def/cli/main.lfy:parse#parse:parse:c6ef1cd5d1ee70ed5efe7e765138382e292ba2e31c91916182971283cce5fc2e
// @lfy def/cli/main.lfy:parse#parse:parse:0c1236254ed2dc7214279b9f60c85ce5ef12cdbea38d4a41543459b1268af732
#[test]
fn an_option_written_with_an_equals_sign_gives_its_text() {
    let invocation = parse_arguments(&args(&["compile", "--target=rust"])).unwrap();
    assert_eq!(invocation.option("target"), Some("rust"));
}

// @lfy def/cli/main.lfy:parse#parse:parse:b8252392bdacab15d1d46035c068176794361e8c15f22b6d21cd543f751e7956
// @lfy def/cli/main.lfy:parse#parse:parse:d430b5e6a7470398614d725eafdc05b807ff1820689bae58a91114f9b344c530
#[test]
fn target_takes_the_value_that_follows_it() {
    // Written before the command, the value it takes does not name the command: the
    // argument after it does.
    // @lfy def/cli/main.lfy:parse#parse:parse:b8252392bdacab15d1d46035c068176794361e8c15f22b6d21cd543f751e7956
    let invocation = parse_arguments(&args(&["--target", "rust", "compile"])).unwrap();
    assert_eq!(invocation.command, Command::Compile);
    assert_eq!(invocation.option("target"), Some("rust"));
    assert!(invocation.arguments.is_empty());
    // Written after it, the value is the same.
    let invocation = parse_arguments(&args(&["compile", "--target", "rust"])).unwrap();
    assert_eq!(invocation.command, Command::Compile);
    assert_eq!(invocation.option("target"), Some("rust"));
}

/// No option but --root, --tree, --target, --jobs, and --color takes the argument after it:
/// any other option is a flag, and the argument after it is the command or positional.
// @lfy def/cli/main.lfy:parse#parse:parse:233533aea4b9472e5d4e1d27500a3950212b9616536f3b46cf00ef040f59af88
// @lfy def/cli/main.lfy:parse#parse:parse:2e73a4f8b63da86790f160a37ed9aa761be453dad91436a10b475b11625c450b
#[test]
fn an_option_written_bare_gives_true_and_takes_no_value() {
    let invocation = parse_arguments(&args(&["compile", "--all", "lexer/main"])).unwrap();
    assert!(invocation.flag("all"));
    assert_eq!(invocation.option("all"), None);
    assert_eq!(invocation.arguments, vec!["lexer/main".to_string()]);
}

/// --jobs takes the value that follows it, which is no positional argument.
// @lfy def/cli/main.lfy:parse#parse:parse:a68f291425ca1fc7f28c32aa7cdc9158127b33b4b73f6bcc8842415bda02baca
// @lfy def/cli/main.lfy:parse#parse:parse:abfabb6a971c58385dc2dcfa8dbe32f3a2a14269c471c65743fbfd57deec1615
// @lfy def/cli/main.lfy:parse#parse:parse:7473c896dd710a7b0dcccc8450958126fa8e9c70c04172f551e9a551ff75afa3
#[test]
fn jobs_takes_the_value_that_follows_it() {
    let invocation = parse_arguments(&args(&["compile", "--jobs", "2"])).unwrap();
    assert_eq!(invocation.command, Command::Compile);
    assert_eq!(invocation.option("jobs"), Some("2"));
    assert!(invocation.arguments.is_empty(), "{:?}", invocation.arguments);
    // Written before the command, the value it takes does not name the command.
    // @lfy def/cli/main.lfy:parse#parse:parse:3874a32a6a9ce574d2416d5c0171397b2a5a6f3c2857cb5d98d62c92e1a25770
    let invocation = parse_arguments(&args(&["--jobs", "3", "compile", "lexer/main"])).unwrap();
    assert_eq!(invocation.command, Command::Compile);
    assert_eq!(invocation.option("jobs"), Some("3"));
    assert_eq!(invocation.arguments, vec!["lexer/main".to_string()]);
}

/// A --jobs value that is no whole number of at least 1, written in decimal digits, is a usage
/// error, however it is written.
// @lfy def/cli/main.lfy:parse#parse:parse:dc25d395e7a9d74532effc5ce433ad65909a37e7d0f2e8ad480c2ed9b7de8581
// @lfy def/cli/main.lfy:parse#parse:parse:dbf0b9db8c348d21cd046133ce143cec46a21cb916cff4abe5976e762450d3d5
#[test]
fn a_jobs_value_that_is_no_whole_number_of_at_least_one_is_a_usage_message() {
    // @lfy def/cli/main.lfy:parse#parse:parse:dc25d395e7a9d74532effc5ce433ad65909a37e7d0f2e8ad480c2ed9b7de8581
    let error = parse_arguments(&args(&["compile", "--jobs=0"])).unwrap_err();
    assert_eq!(error, "--jobs must be a whole number of at least 1");
    for value in ["-1", "two", "1.5", "+2", " 2", ""] {
        let error = parse_arguments(&args(&["compile", &format!("--jobs={value}")])).unwrap_err();
        assert_eq!(error, "--jobs must be a whole number of at least 1", "{value:?}");
    }
    let error = parse_arguments(&args(&["compile", "--jobs", "lexer/main"])).unwrap_err();
    assert_eq!(error, "--jobs must be a whole number of at least 1");
    assert_eq!(parse_arguments(&args(&["compile", "--jobs=12"])).unwrap().option("jobs"), Some("12"));
}

// @lfy def/cli/main.lfy:parse#parse:parse:3645fdc7e4fa098477e08321e6ca15068b540b9c205c6dd4dddb5f68dba4d549
#[test]
fn the_root_option_names_the_project_directory() {
    assert_eq!(parse_arguments(&args(&["check", "--root", "/tmp"])).unwrap().root, "/tmp");
    assert_eq!(parse_arguments(&args(&["check", "--root=/tmp"])).unwrap().root, "/tmp");
    // A relative directory is made absolute against the current directory, so every path
    // and ELFIE_ROOT built from it names the same place from any directory.
    // @lfy def/cli/main.lfy:parse#parse:parse:3645fdc7e4fa098477e08321e6ca15068b540b9c205c6dd4dddb5f68dba4d549
    let here = std::env::current_dir().unwrap().join("here");
    assert_eq!(Path::new(&parse_arguments(&args(&["check", "--root", "here"])).unwrap().root), here);
    assert_eq!(Path::new(&parse_arguments(&args(&["check", "--root=here"])).unwrap().root), here);
}

// @lfy def/cli/main.lfy:parse#parse:parse:7473c896dd710a7b0dcccc8450958126fa8e9c70c04172f551e9a551ff75afa3
#[test]
fn what_names_neither_the_command_nor_an_option_value_is_positional() {
    let invocation = parse_arguments(&args(&[
        "compile", "--dry-run", "--target", "rust", "--root", "/tmp", "--jobs", "2", "lexer/main",
    ]))
    .unwrap();
    assert!(invocation.flag("dry-run"));
    assert_eq!(invocation.arguments, vec!["lexer/main".to_string()]);
    // The value --color takes is no positional argument either.
    let invocation = parse_arguments(&args(&["compile", "--color", "never", "lexer/main"])).unwrap();
    assert_eq!(invocation.arguments, vec!["lexer/main".to_string()]);
}

// @lfy def/cli/main.lfy:parse#parse:parse:7bcf6480b22f39361f8cf17177eeb14d00016bfcdcae4c115113d275a3436eaf
// @lfy def/cli/main.lfy:parse#parse:parse:fff69e97f1f4842d2724ff711989149786a96a351c4d05ee30c6c64f1629a3d3
#[test]
fn help_and_version_win_anywhere() {
    assert_eq!(parse_arguments(&args(&["check", "--help"])).unwrap().command, Command::Help);
    assert_eq!(parse_arguments(&args(&["-h"])).unwrap().command, Command::Help);
    assert_eq!(parse_arguments(&args(&["--version"])).unwrap().command, Command::Version);
    assert_eq!(parse_arguments(&args(&["check", "--version"])).unwrap().command, Command::Version);
    // --version names the command only when neither --help nor -h is among the arguments.
    // @lfy def/cli/main.lfy:parse#parse:parse:fff69e97f1f4842d2724ff711989149786a96a351c4d05ee30c6c64f1629a3d3
    assert_eq!(parse_arguments(&args(&["--version", "--help"])).unwrap().command, Command::Help);
    assert_eq!(parse_arguments(&args(&["-h", "--version"])).unwrap().command, Command::Help);
    // An argument that does not begin with a dash names no command when --help is among
    // them.
    assert_eq!(parse_arguments(&args(&["--help", "frobnicate"])).unwrap().command, Command::Help);
}

// @lfy def/cli/main.lfy:parse#parse:parse:414b6a3814d9b33eb9ee85b7db8bcea349d0753972f675579d3a2645c4e54547
#[test]
fn help_and_version_take_no_positional_arguments() {
    for arguments in [args(&["--help", "frobnicate"]), args(&["-h", "check", "x"]), args(&["--version", "x"])] {
        let invocation = parse_arguments(&arguments).unwrap();
        assert!(invocation.arguments.is_empty(), "{:?}", invocation.arguments);
    }
    // The root and the options are still read as they are written.
    let invocation = parse_arguments(&args(&["--help", "--root", "/tmp", "--json"])).unwrap();
    assert_eq!(invocation.root, "/tmp");
    assert!(invocation.flag("json"));
    assert!(invocation.arguments.is_empty());
}

// @lfy def/cli/main.lfy:parse#parse:parse:95e98fdf72dec65667d15defc92aa98f02e4a2d19663a4159b088872839bb8d3
// @lfy def/cli/main.lfy:parse#parse:parse:372de213ff4328da7d560b94bc55aaa40b317fbfb13ed63c19833a9b840bb5f9
#[test]
fn color_followed_by_a_choice_takes_it_as_its_value() {
    let invocation = parse_arguments(&args(&["--color", "never", "check"])).unwrap();
    assert_eq!(invocation.command, Command::Check);
    assert_eq!(invocation.option("color"), Some("never"));
    let invocation = parse_arguments(&args(&["check", "--color=always"])).unwrap();
    assert_eq!(invocation.option("color"), Some("always"));
}

// @lfy def/cli/main.lfy:parse#parse:parse:d965a2707f01d2eabc754383f4dfbd1753a9cb3926e0ff6735022a72b0fcdfd7
// @lfy def/cli/main.lfy:parse#parse:parse:0da4c49e8c974658ca2be342ff4ac6e0e21e09807b88714b57af4c15bca1404c
// @lfy def/cli/main.lfy:parse#parse:parse:5df10c8ed49640a49819e6470920fbd7c525c644a77c7d9cb53141a553068141
#[test]
fn color_not_followed_by_a_choice_is_a_flag() {
    // The next argument is left for what follows, so it still names the command.
    let invocation = parse_arguments(&args(&["--color", "check"])).unwrap();
    assert_eq!(invocation.command, Command::Check);
    assert!(invocation.flag("color"));
    assert_eq!(invocation.option("color"), None);
}

// @lfy def/cli/main.lfy:parse#parse:parse:0be5993fbfc9cfaa0fa07c774c7656d48b82dddb25cffe651e98bfddf6211d41
// @lfy def/cli/main.lfy:parse#parse:parse:5f6a4cec2476302e1f65b3f63fd1793dbdea9475e1d5856222fcb8f1396bc841
#[test]
fn a_color_value_that_names_no_choice_is_a_usage_message() {
    let error = parse_arguments(&args(&["check", "--color=sometimes"])).unwrap_err();
    assert_eq!(error, "--color must be auto, always, or never");
}

// @lfy def/cli/main.lfy:parse#parse:parse:55cfb0320f82b73e9a6a9d2faa97a3dd41367a5d9e70a5b07f676b249b39fcc1
// @lfy def/cli/main.lfy:parse#parse:parse:233533aea4b9472e5d4e1d27500a3950212b9616536f3b46cf00ef040f59af88
#[test]
fn every_other_flag_never_takes_the_next_argument() {
    let invocation = parse_arguments(&args(&["--json", "check"])).unwrap();
    assert_eq!(invocation.command, Command::Check);
    assert!(invocation.flag("json"));
    let invocation = parse_arguments(&args(&["--strict", "--no-verify", "check"])).unwrap();
    assert_eq!(invocation.command, Command::Check);
    assert!(invocation.flag("strict") && invocation.flag("no-verify"));
}
