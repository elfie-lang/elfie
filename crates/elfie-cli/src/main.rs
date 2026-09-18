//! `elfie <file>`: lexes an Elfie source file and prints its tokens, one per line.
//! `elfie --tree <file>`: parses it and prints the tree, then every error node.
//!
//! Each token line shows the token's `line:column`, the terminal it was lexed as (or
//! `Invalid`), its raw source text, and its value when that differs from the raw text. A
//! lexing error is reported on stderr with exit status 1, and so is a tree with errors.

use std::env;
use std::fs;
use std::io::{self, BufWriter, ErrorKind, Write};
use std::process::ExitCode;

use elfie_core::grammar::GrammarRule;
use elfie_core::lexer::lex;
use elfie_core::parser::parse;

fn main() -> ExitCode {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let tree = args.first().is_some_and(|arg| arg == "--tree");
    if tree {
        args.remove(0);
    }
    let [path] = args.as_slice() else {
        eprintln!("usage: elfie [--tree] <file>");
        return ExitCode::from(2);
    };
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{path}: {error}");
            return ExitCode::from(2);
        }
    };
    let tokens = match lex(&source, Some(path)) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let mut out = BufWriter::new(io::stdout().lock());
    let written = if tree {
        let tree = parse(tokens, None);
        let mut result = out.write_all(tree.render().as_bytes());
        for error in &tree.errors {
            let token = tree.tokens.get(error.start).or(tree.tokens.last());
            let (line, column) = token.map_or((0, 0), |token| (token.line, token.column));
            result = result.and_then(|()| {
                writeln!(
                    out,
                    "error at {path}:{line}:{column}: expected [{}] but found {:?}",
                    error.expected.join(", "),
                    tree.raw(error.start, error.end)
                )
            });
        }
        result.and_then(|()| out.flush()).map(|()| tree.errors.is_empty())
    } else {
        tokens
            .iter()
            .try_for_each(|token| {
                let rule = token.rule.map_or("Invalid", |rule| rule.identifier());
                write!(out, "{}:{}\t{rule}\t{:?}", token.line, token.column, token.raw)?;
                if token.value != token.raw {
                    write!(out, "\t=> {:?}", token.value)?;
                }
                writeln!(out)
            })
            .and_then(|()| out.flush())
            .map(|()| true)
    };
    match written {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        // The reader stopped early (e.g. `| head`); there is nothing left to report.
        Err(error) if error.kind() == ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
