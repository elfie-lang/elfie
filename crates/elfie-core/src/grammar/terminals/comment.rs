//! Compiled from `def/grammar/terminals/comment.lfy`.

use super::super::traits::category::*;
use super::super::{Entity, grammar_rules};
use super::literal::Literal;
use super::space::Space;

grammar_rules! {
    /// Comment and documentation terminals and the rules that assemble them.
    pub enum Comment {
        BlockCommentOpen is [boundary("/*")]: "Opens a block comment" = r#""/*""#, // @lfy def/grammar/terminals/comment.lfy:5
        BlockCommentClose is [boundary("*/")]: "Closes a block comment" = r#""*/""#, // @lfy def/grammar/terminals/comment.lfy:6
        BlockCommentBody is [body(BLOCK_COMMENT_BODY_EXCLUDED, &[])]: "Text of a block comment; nested opens and closes are tokens of their own" = "(: ( Character - ( [[BlockCommentOpen]] | [[BlockCommentClose]] ) ) :)", // @lfy def/grammar/terminals/comment.lfy:7
        LineCommentOpen is [boundary("//")]: "Opens a comment that ends with the line" = r#""//""#, // @lfy def/grammar/terminals/comment.lfy:8
        LineCommentBody is [body(LINE_COMMENT_BODY_EXCLUDED, &[])]: "Text of a line comment" = "(: ( Character - ( [[NewLine]] ) ) :)", // @lfy def/grammar/terminals/comment.lfy:9

        /// Followed immediately by `/`, it is a `BlockCommentOpen` followed by a
        /// `BlockCommentClose` instead; followed immediately by `*/`, it is a
        /// `BlockCommentOpen` followed by a `BlockCommentBody` with the `*` text followed
        /// by a `BlockCommentClose` instead. See [`block_documentation_open_is_block_comment_open`].
        BlockDocumentationOpen is [boundary("/**")]: "Opens documentation that ends with its close" = r#""/**""#, // @lfy def/grammar/terminals/comment.lfy:11
        BlockDocumentationClose is [boundary("**/")]: "Closes block documentation" = r#""**/""#, // @lfy def/grammar/terminals/comment.lfy:15
        BlockDocumentationBody is [body(BLOCK_DOCUMENTATION_BODY_EXCLUDED, &[])]: "Text of block documentation; references are tokens of their own" = "(: ( Character - ( [[BlockDocumentationOpen]] | [[BlockDocumentationClose]] | [[ReferenceOpen]] ) ) :)", // @lfy def/grammar/terminals/comment.lfy:16
        LineDocumentationOpen is [boundary("///")]: "Opens documentation that ends with the line" = r#""///""#, // @lfy def/grammar/terminals/comment.lfy:17
        LineDocumentationBody is [body(LINE_DOCUMENTATION_BODY_EXCLUDED, &[])]: "Text of line documentation" = "(: ( Character - ( [[NewLine]] | [[ReferenceOpen]] | [[ReferenceClose]] ) ) :)", // @lfy def/grammar/terminals/comment.lfy:18

        Comment is [rule()]: "A comment; carries no meaning" = "( [[BlockCommentOpen]] , (: [[BlockCommentBody]] | [[Comment]] :) , [[BlockCommentClose]] ) | ( [[LineCommentOpen]] , (/ [[LineCommentBody]] /) )", // @lfy def/grammar/terminals/comment.lfy:20
        /// Acceptance criteria:
        /// - Attaches to the declaration that follows it.
        /// - When no declaration follows: attaches to nothing.
        /// - When no declaration exists between two or more documentation blocks and the
        ///   documentation blocks were all opened with `BlockDocumentationOpen`: attaches
        ///   to the "global" object.
        /// - When `LineDocumentationOpen` is the first token and `TemplateReference` is
        ///   used: a `NewLine` inside the `TemplateReference` is not trivia.
        Documentation is [rule()]: "Documentation for the declaration that follows it" = "( [[BlockDocumentationOpen]] , (: [[BlockDocumentationBody]] | [[TemplateReference]] :) , [[BlockDocumentationClose]] ) | ( [[LineDocumentationOpen]] , (: [[LineDocumentationBody]] | [[TemplateReference]] :) )", // @lfy def/grammar/terminals/comment.lfy:21
    }
}

// @lfy def/grammar/terminals/comment.lfy:7
pub const BLOCK_COMMENT_BODY_EXCLUDED: &[Entity] = &[
    Entity::Comment(Comment::BlockCommentOpen),
    Entity::Comment(Comment::BlockCommentClose),
];
// @lfy def/grammar/terminals/comment.lfy:9
pub const LINE_COMMENT_BODY_EXCLUDED: &[Entity] = &[Entity::Space(Space::NewLine)];
// @lfy def/grammar/terminals/comment.lfy:16
pub const BLOCK_DOCUMENTATION_BODY_EXCLUDED: &[Entity] = &[
    Entity::Comment(Comment::BlockDocumentationOpen),
    Entity::Comment(Comment::BlockDocumentationClose),
    Entity::Literal(Literal::ReferenceOpen),
];
// @lfy def/grammar/terminals/comment.lfy:18
pub const LINE_DOCUMENTATION_BODY_EXCLUDED: &[Entity] = &[
    Entity::Space(Space::NewLine),
    Entity::Literal(Literal::ReferenceOpen),
    Entity::Literal(Literal::ReferenceClose),
];

/// The `where` clauses of `BlockDocumentationOpen`: text that matched it is a
/// `BlockCommentOpen` instead when `after`, the text immediately following the match,
/// begins with `/` (the rest is then a `BlockCommentClose`) or with `*/` (the rest is then
/// a `BlockCommentBody` holding `*` and a `BlockCommentClose`).
// @lfy def/grammar/terminals/comment.lfy:12
pub fn block_documentation_open_is_block_comment_open(after: &str) -> bool {
    after.starts_with('/') // @lfy def/grammar/terminals/comment.lfy:12
        || after.starts_with("*/") // @lfy def/grammar/terminals/comment.lfy:13
}

#[cfg(test)]
mod tests {
    use super::super::super::GrammarRule;
    use super::*;

    // @lfy def/grammar/terminals/comment.lfy:7
    #[test]
    fn bodies_stop_before_their_delimiters() {
        assert_eq!(
            Comment::BlockCommentBody.longest_match("a /* b */"),
            Some(2)
        );
        assert_eq!(Comment::BlockCommentBody.longest_match("a **/"), Some(3));
        assert_eq!(Comment::BlockCommentBody.longest_match("*/"), None);
        assert_eq!(Comment::BlockCommentBody.longest_match("**/"), Some(1));
        assert_eq!(Comment::LineCommentBody.longest_match("a */ b\nc"), Some(6));
        assert_eq!(Comment::LineCommentBody.longest_match("\n"), None);
        assert_eq!(
            Comment::BlockDocumentationBody.longest_match("a */ [[b"),
            Some(5)
        );
        assert_eq!(
            Comment::BlockDocumentationBody.longest_match("a /**/ **/"),
            Some(2)
        );
        assert_eq!(
            Comment::BlockDocumentationBody.longest_match("a\nb **/"),
            Some(4)
        );
        assert_eq!(
            Comment::LineDocumentationBody.longest_match("a ]] b"),
            Some(2)
        );
        assert_eq!(
            Comment::LineDocumentationBody.longest_match("a [[b"),
            Some(2)
        );
    }

    // @lfy def/grammar/terminals/comment.lfy:12
    #[test]
    fn block_documentation_open_gives_way_to_a_block_comment_open() {
        assert!(block_documentation_open_is_block_comment_open("/"));
        assert!(block_documentation_open_is_block_comment_open("/ x"));
        assert!(block_documentation_open_is_block_comment_open("*/"));
        assert!(!block_documentation_open_is_block_comment_open(" x"));
        assert!(!block_documentation_open_is_block_comment_open("*"));
        assert!(!block_documentation_open_is_block_comment_open(""));
        assert_eq!(
            Comment::BlockDocumentationOpen.longest_match("/**/"),
            Some(3)
        );
        assert_eq!(Comment::BlockCommentOpen.longest_match("/**/"), Some(2));
    }

    // @lfy def/grammar/terminals/comment.lfy:20
    #[test]
    fn comments_nest_and_documentation_holds_references() {
        assert!(Comment::Comment.matches("/* a /* b */ c */"));
        assert!(Comment::Comment.matches("// a"));
        assert!(Comment::Comment.matches("//"));
        assert!(!Comment::Comment.matches("/* a"));
        assert!(!Comment::Comment.matches("// a\n"));
        assert!(Comment::Documentation.matches("/** a **/"));
        assert!(Comment::Documentation.matches("/// a [[b]] c"));
        assert!(!Comment::Documentation.matches("/// a ]] c"));
        assert!(!Comment::Comment.is_terminal());
    }
}
