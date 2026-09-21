//! Elfie compiler crate root.
//!
//! Compiled output of `def/grammar` lives in [`grammar`], of `def/lexer` in [`lexer`], and
//! of `def/parser` in [`parser`].

pub mod format; // @lfy def/format/main.lfy:format
pub mod generation; // @lfy def/generation/main.lfy:plan
pub mod grammar; // @lfy def/grammar/main.lfy:1
pub mod lexer; // @lfy def/lexer/main.lfy:lex
pub mod model; // @lfy def/model/main.lfy:bind
pub mod parser; // @lfy def/parser/main.lfy:parse
pub mod query; // @lfy def/query/main.lfy:rangeOf
pub mod workspace; // @lfy def/workspace/main.lfy:load
