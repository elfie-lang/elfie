//! `elfie <file>`: lexes an Elfie source file and prints its tokens, one per line.
//!
//! Each line shows the token's `line:column`, the terminal it was lexed as (or `Invalid`),
//! its raw source text, and its value when that differs from the raw text. A lexing error
//! is reported on stderr with exit status 1.

use std::env;
use std::fs;
use std::io::{self, BufWriter, ErrorKind, Write};
use std::process::ExitCode;

use elfie::grammar::GrammarRule;
use elfie::lexer::lex;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let (Some(path), None) = (args.next(), args.next()) else {
        eprintln!("usage: elfie <file>");
        return ExitCode::from(2);
    };
    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::from(2);
        }
    };
    let tokens = match lex(&source, Some(&path)) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut out = BufWriter::new(io::stdout().lock());
    let written = tokens.iter().try_for_each(|token| {
        let rule = token.rule.map_or("Invalid", |rule| rule.identifier());
        write!(
            out,
            "{}:{}\t{rule}\t{:?}",
            token.line, token.column, token.raw
        )?;
        if token.value != token.raw {
            write!(out, "\t=> {:?}", token.value)?;
        }
        writeln!(out)
    });
    match written.and_then(|()| out.flush()) {
        Ok(()) => ExitCode::SUCCESS,
        // The reader stopped early (e.g. `| head`); there is nothing left to report.
        Err(error) if error.kind() == ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
