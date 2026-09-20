//! The trees being bound, indexed so that a [`NodeRef`] finds its node and a node its
//! reference, plus the small views of a node's children the binder pattern-matches on.

use std::collections::HashMap;

use crate::grammar::rules::expression::Expression as E;
use crate::grammar::rules::statement::Statement as S;
use crate::grammar::terminals::identifier::Identifier as I;
use crate::grammar::terminals::literal::Literal as L;
use crate::grammar::{Entity as Rule, GrammarRule};
use crate::lexer::Token;
use crate::parser::components::is_trivia;
use crate::parser::data::{Child, Node};

use super::data::{FileId, NodeInfo, NodeRef, Source};

/// One significant child of a node: a nested node or a token that is not trivia.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Part {
    Node(NodeRef),
    Token(usize),
}

pub(crate) struct Trees {
    pub sources: Vec<Source>,
    pub nodes: Vec<Vec<NodeInfo>>,
    pub keys: HashMap<(FileId, usize, usize, Rule), usize>,
}

impl Trees {
    pub fn new(sources: Vec<Source>) -> Trees {
        let mut nodes = Vec::with_capacity(sources.len());
        let mut keys = HashMap::new();
        for (file, source) in sources.iter().enumerate() {
            let mut infos = Vec::new();
            index(&source.tree.root, None, Vec::new(), &mut infos);
            for (i, info) in infos.iter().enumerate() {
                keys.entry((file, info.start, info.end, info.rule)).or_insert(i);
            }
            nodes.push(infos);
        }
        Trees { sources, nodes, keys }
    }

    pub fn node(&self, r: NodeRef) -> &Node {
        let mut node = &self.sources[r.file].tree.root;
        for &step in &self.nodes[r.file][r.index].path {
            node = match &node.children[step as usize] {
                Child::Node(child) => child,
                _ => unreachable!("node path leads to a token"),
            };
        }
        node
    }

    pub fn parent(&self, r: NodeRef) -> Option<NodeRef> {
        self.nodes[r.file][r.index].parent.map(|index| NodeRef { file: r.file, index })
    }

    pub fn rule(&self, r: NodeRef) -> Rule {
        self.nodes[r.file][r.index].rule
    }

    pub fn is<R: GrammarRule>(&self, r: NodeRef, rule: R) -> bool {
        self.rule(r) == rule.entity()
    }

    pub fn tokens(&self, file: FileId) -> &[Token] {
        &self.sources[file].tree.tokens
    }

    pub fn token(&self, file: FileId, index: usize) -> &Token {
        &self.sources[file].tree.tokens[index]
    }

    pub fn token_is<R: GrammarRule>(&self, file: FileId, index: usize, rule: R) -> bool {
        self.token(file, index).rule == Some(rule.entity())
    }

    pub fn raw(&self, r: NodeRef) -> String {
        let info = &self.nodes[r.file][r.index];
        self.sources[r.file].tree.raw(info.start, info.end)
    }

    /// The reference of a child node of `parent`.
    pub fn child_ref(&self, file: FileId, child: &Node) -> NodeRef {
        let index = self.keys[&(file, child.start, child.end, child.rule)];
        NodeRef { file, index }
    }

    /// The significant children: nested nodes and tokens that are not trivia, in order.
    pub fn parts(&self, r: NodeRef) -> Vec<Part> {
        let node = self.node(r);
        let tokens = self.tokens(r.file);
        node.children
            .iter()
            .filter_map(|child| match child {
                Child::Node(child) => {
                    if is_trivia(child.rule) {
                        None
                    } else {
                        Some(Part::Node(self.child_ref(r.file, child)))
                    }
                }
                Child::Error(_) => None,
                Child::Token(index) => {
                    let rule = tokens[*index].rule?;
                    (!is_trivia(rule)).then_some(Part::Token(*index))
                }
            })
            .collect()
    }

    /// Every child node, trivia included, in order.
    pub fn children(&self, r: NodeRef) -> Vec<NodeRef> {
        let node = self.node(r);
        node.children
            .iter()
            .filter_map(|child| match child {
                Child::Node(child) => Some(self.child_ref(r.file, child)),
                _ => None,
            })
            .collect()
    }

    /// The first significant child node satisfying `rule`.
    pub fn child<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Option<NodeRef> {
        self.parts(r).into_iter().find_map(|part| match part {
            Part::Node(child) if self.is(child, rule) => Some(child),
            _ => None,
        })
    }

    /// Every significant child node satisfying `rule`.
    pub fn children_of<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Vec<NodeRef> {
        self.parts(r)
            .into_iter()
            .filter_map(|part| match part {
                Part::Node(child) if self.is(child, rule) => Some(child),
                _ => None,
            })
            .collect()
    }

    /// The significant child nodes, in order.
    pub fn child_nodes(&self, r: NodeRef) -> Vec<NodeRef> {
        self.parts(r)
            .into_iter()
            .filter_map(|part| match part {
                Part::Node(child) => Some(child),
                _ => None,
            })
            .collect()
    }

    /// The first significant child token satisfying `rule`.
    pub fn child_token<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Option<usize> {
        let rule = rule.entity();
        self.parts(r).into_iter().find_map(|part| match part {
            Part::Token(index) if self.token(r.file, index).rule == Some(rule) => Some(index),
            _ => None,
        })
    }

    pub fn has_token<R: GrammarRule>(&self, r: NodeRef, rule: R) -> bool {
        self.child_token(r, rule).is_some()
    }

    /// Whether the node is a statement rule.
    pub fn is_statement(&self, r: NodeRef) -> bool {
        self.rule(r).is_statement()
    }

    /// The value of the string a `StringLiteral` node holds.
    pub fn string_value(&self, r: NodeRef) -> Option<String> {
        let inner = self.child_nodes(r).into_iter().next()?;
        let tokens = self.tokens(r.file);
        let node = self.node(inner);
        let body = node
            .children
            .iter()
            .filter_map(Child::as_token)
            .find(|&i| tokens[i].is(L::SingleQuoteBody) || tokens[i].is(L::DoubleQuoteBody));
        Some(body.map_or_else(String::new, |i| tokens[i].value.clone()))
    }

    /// The identifier token of a `Name` node.
    pub fn name_token(&self, r: NodeRef) -> Option<usize> {
        self.child_token(r, I::Identifier)
    }

    /// The text of a `Name` node.
    pub fn name(&self, r: NodeRef) -> Option<String> {
        self.name_token(r).map(|i| self.token(r.file, i).value.clone())
    }

    /// Whether a token can be a member name: an identifier or a keyword.
    pub fn is_member_name_token(&self, file: FileId, index: usize) -> bool {
        let token = self.token(file, index);
        token.rule.is_some_and(|rule| rule == I::Identifier.entity() || rule.is_keyword())
    }

    /// The accessor token and the name token, if any, of a `Current` or `Member` node.
    pub fn accessor_and_name(&self, r: NodeRef) -> Option<(usize, Option<usize>)> {
        let parts = self.parts(r);
        let mut iter = parts.iter().copied();
        let accessor = if self.is(r, E::Member) {
            iter.next();
            iter.next()
        } else {
            iter.next()
        };
        let Some(Part::Token(accessor)) = accessor else {
            return None;
        };
        // The token must be an accessor.
        super::components::accessor_layer(self.token(r.file, accessor).rule?)?;
        let name = match iter.next() {
            Some(Part::Token(name)) if self.is_member_name_token(r.file, name) => Some(name),
            _ => None,
        };
        Some((accessor, name))
    }

    /// The left operand of an infix or postfix node.
    pub fn left(&self, r: NodeRef) -> Option<NodeRef> {
        match self.parts(r).first() {
            Some(Part::Node(left)) => Some(*left),
            _ => None,
        }
    }

    /// The last child node of a node.
    pub fn last_node(&self, r: NodeRef) -> Option<NodeRef> {
        self.child_nodes(r).last().copied()
    }

    /// The expression nodes of an `Items` node, spread operations included as written.
    pub fn items(&self, r: NodeRef) -> Vec<NodeRef> {
        self.child_nodes(r)
    }

    /// The argument nodes of a `Call` or `Arguments` node.
    pub fn arguments(&self, r: NodeRef) -> Vec<NodeRef> {
        match self.child(r, E::Items) {
            Some(items) => self.items(items),
            None => Vec::new(),
        }
    }

    /// The nearest ancestor (or the node itself) satisfying `rule`.
    pub fn ancestor<R: GrammarRule>(&self, r: NodeRef, rule: R) -> Option<NodeRef> {
        let mut current = Some(r);
        while let Some(node) = current {
            if self.is(node, rule) {
                return Some(node);
            }
            current = self.parent(node);
        }
        None
    }

    /// The first node satisfying `holder` found from `r` in token order without entering
    /// a nested `Parameters`, `Block`, `Object`, or `Type`, then the first `Identifier`
    /// token inside it found the same way.
    // @lfy def/model/traits.lfy:declaring
    pub fn declared_name_token(&self, r: NodeRef, holder: Rule) -> Option<usize> {
        let holder_node = if holder == I::Identifier.entity() {
            r
        } else {
            self.find_holder(r, holder, true)?
        };
        self.first_identifier(holder_node, true)
    }

    fn find_holder(&self, r: NodeRef, holder: Rule, top: bool) -> Option<NodeRef> {
        if !top && super::components::is_holder_barrier(self.rule(r)) {
            return None;
        }
        for child in self.children(r) {
            if self.rule(child) == holder {
                return Some(child);
            }
            if let Some(found) = self.find_holder(child, holder, false) {
                return Some(found);
            }
        }
        None
    }

    fn first_identifier(&self, r: NodeRef, top: bool) -> Option<usize> {
        if !top && super::components::is_holder_barrier(self.rule(r)) {
            return None;
        }
        let node = self.node(r);
        let tokens = self.tokens(r.file);
        for child in &node.children {
            match child {
                Child::Token(index) if tokens[*index].is(I::Identifier) => return Some(*index),
                Child::Node(child) => {
                    let child = self.child_ref(r.file, child);
                    if let Some(found) = self.first_identifier(child, false) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Whether the node is an `ExpressionStatement` in a data or trait body that begins
    /// with `Current` using the scope accessor: a member declaration.
    // @lfy def/model/main.lfy:bind
    pub fn member_declaration(&self, r: NodeRef) -> Option<NodeRef> {
        if !self.is(r, S::ExpressionStatement) {
            return None;
        }
        let block = self.parent(r)?;
        if !self.is(block, S::Block) {
            return None;
        }
        let owner = self.parent(block)?;
        if !(self.is(owner, S::DataDeclaration) || self.is(owner, S::TraitDeclaration)) {
            return None;
        }
        let expression = self.child_nodes(r).into_iter().next()?;
        let current = self.leftmost(expression);
        if !self.is(current, E::Current) {
            return None;
        }
        let (accessor, name) = self.accessor_and_name(current)?;
        if !self.token_is(r.file, accessor, crate::grammar::terminals::punctuation::Punctuation::ScopeAccessor) {
            return None;
        }
        name.map(|_| current)
    }

    /// The leftmost operand of a chain of infix and postfix operations.
    pub fn leftmost(&self, r: NodeRef) -> NodeRef {
        let mut current = r;
        loop {
            let rule = self.rule(current);
            if rule.is_infix() || rule.is_postfix() {
                match self.left(current) {
                    Some(left) => current = left,
                    None => return current,
                }
            } else {
                return current;
            }
        }
    }
}

fn index(node: &Node, parent: Option<usize>, path: Vec<u32>, out: &mut Vec<NodeInfo>) {
    let me = out.len();
    out.push(NodeInfo {
        rule: node.rule,
        start: node.start,
        end: node.end,
        parent,
        path: path.clone(),
    });
    for (i, child) in node.children.iter().enumerate() {
        if let Child::Node(child) = child {
            let mut child_path = path.clone();
            child_path.push(i as u32);
            index(child, Some(me), child_path, out);
        }
    }
}
