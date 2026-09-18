//! Elfie compiler crate root.
//!
//! Compiled output of `def/grammar` lives in [`grammar`], of `def/lexer` in [`lexer`], and
//! of `def/parser` in [`parser`].

pub mod format; // @lfy def/format/main.lfy:15
pub mod generation; // @lfy def/generation/main.lfy:12
pub mod grammar; // @lfy def/grammar/main.lfy:1
pub mod lexer; // @lfy def/lexer/main.lfy:19
pub mod model; // @lfy def/model/main.lfy:10
pub mod parser; // @lfy def/parser/main.lfy:12
pub mod query; // @lfy def/query/main.lfy:22
pub mod workspace; // @lfy def/workspace/main.lfy:9
