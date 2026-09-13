//! Compiled from `def/grammar/tokens/comment.lfy`.

use super::super::{Forbidden, RuleTrait, grammar_rules};

grammar_rules! {
    /// Comment and documentation tokens.
    pub enum Comment {
        /// a block comment open; can be nested within other comment blocks
        CommentBlockOpen = r#""/*""#, // @lfy def/grammar/tokens/comment.lfy:5
        /// a block comment close
        CommentBlockClose = r#""*/""#, // @lfy def/grammar/tokens/comment.lfy:10
        /// contents of the comment
        CommentBlockBody = "(: UTF8_CHARACTER - ( [[CommentBlockOpen]] | [[CommentBlockClose]] ) :)", // @lfy def/grammar/tokens/comment.lfy:12
        /// an inline comment start
        CommentInlineStart = r#""//""#, // @lfy def/grammar/tokens/comment.lfy:14
        /// contents of the comment
        CommentInlineBody = "(: UTF8_CHARACTER - ( [[NewLine]] ) :)", // @lfy def/grammar/tokens/comment.lfy:16
        /// a block documentation open; can be nested within other documentation blocks and
        /// is not a match when followed by `/`
        DocumentationBlockOpen is [RuleTrait::NotAllowedPostfix(&[Forbidden::Text("/")])]
            = r#""/**""#, // @lfy def/grammar/tokens/comment.lfy:18
        /// a block documentation close
        DocumentationBlockClose = r#""**/""#, // @lfy def/grammar/tokens/comment.lfy:23
        /// contents of the comment
        DocumentationBlockBody = "(: UTF8_CHARACTER - ( [[DocumentationBlockOpen]] | [[DocumentationBlockClose]] | [[ReferenceOpenBoundary]] ) :)", // @lfy def/grammar/tokens/comment.lfy:25
        /// an inline documentation start
        DocumentationInlineStart = r#""///""#, // @lfy def/grammar/tokens/comment.lfy:27
        /// contents of the comment
        DocumentationInlineBody = "(: UTF8_CHARACTER - ( [[NewLine]] | [[ReferenceOpenBoundary]] | [[ReferenceCloseBoundary]] ) :)", // @lfy def/grammar/tokens/comment.lfy:29
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::EbnfSyntax;
    use super::*;

    // @lfy def/grammar/tokens/comment.lfy:18
    #[test]
    fn documentation_block_open_is_not_followed_by_a_slash() {
        assert_eq!(
            Comment::DocumentationBlockOpen.traits(),
            &[RuleTrait::NotAllowedPostfix(&[Forbidden::Text("/")])]
        );
        assert_eq!(Comment::DocumentationBlockOpen.longest_match("/**/"), None);
        assert_eq!(
            Comment::DocumentationBlockOpen.longest_match("/** x"),
            Some(3)
        );
        assert_eq!(Comment::CommentBlockOpen.longest_match("/**/"), Some(2));
        assert!(Comment::CommentBlockOpen.traits().is_empty());
        assert_eq!(
            Comment::CommentInlineStart.rule(),
            r#"CommentInlineStart = "//" ;"#
        );
    }

    // @lfy def/grammar/tokens/comment.lfy:12
    #[test]
    fn bodies_stop_before_their_delimiters() {
        assert_eq!(
            Comment::CommentBlockBody.longest_match("a /* b */"),
            Some(2)
        );
        assert_eq!(Comment::CommentBlockBody.longest_match("a **/"), Some(3));
        assert_eq!(
            Comment::CommentInlineBody.longest_match("a */ b\nc"),
            Some(6)
        );
        assert_eq!(
            Comment::DocumentationBlockBody.longest_match("a */ [[b"),
            Some(5)
        );
        assert_eq!(
            Comment::DocumentationBlockBody.longest_match("a /**/ **/"),
            Some(3)
        );
        assert_eq!(
            Comment::DocumentationInlineBody.longest_match("a ]] b"),
            Some(2)
        );
    }
}
