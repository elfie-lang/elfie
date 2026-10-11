// @lfy def/interpret/main.lfy:expand
//! The text a value, a type, and a template render to.

use super::*;

impl Interpreter<'_> {
    /// The text of a string or template expression, evaluated; another expression gives its
    /// value as text, or its source when it has none.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn text_of(&mut self, r: NodeRef, env: &Env) -> String {
        let mut env = env.clone();
        let before = self.runtime;
        let value = self.eval(r, &mut env);
        self.runtime = before;
        match value {
            Interim::Undefined => self.model.raw(r).trim().to_string(),
            other => self.to_text(&other),
        }
    }

    // @lfy def/interpret/main.lfy:expand
    pub(super) fn to_text(&self, value: &Interim) -> String {
        match value {
            Interim::Undefined => "undefined".to_string(),
            Interim::Null => "null".to_string(),
            Interim::Bool(boolean) => boolean.to_string(),
            Interim::Number(number) => {
                if number.fract() == 0.0 {
                    format!("{}", *number as i64)
                } else {
                    number.to_string()
                }
            }
            Interim::String(text) => text.clone(),
            Interim::List(items) => items
                .iter()
                .map(|item| self.to_text(item))
                .collect::<Vec<_>>()
                .join(", "),
            Interim::Object(pairs) => pairs
                .iter()
                .map(|(key, value)| format!("{key} = {}", self.to_text(value)))
                .collect::<Vec<_>>()
                .join(", "),
            Interim::Entity(entity) => self.model.entities[*entity]
                .identifier
                .clone()
                .unwrap_or_else(|| "anonymous".to_string()),
            Interim::Type(ty) => self.type_text(ty),
            Interim::Scope(_) => "scope".to_string(),
            Interim::Closure(..) | Interim::Function(..) => "function".to_string(),
            Interim::Criteria(_) => ACCEPTANCE_CRITERIA.to_string(),
            Interim::Tester(_) => TEST.to_string(),
            Interim::Like(inner) => self.to_text(inner),
        }
    }

    // @lfy def/interpret/main.lfy:expand
    pub(super) fn type_text(&self, ty: &TypeRef) -> String {
        match ty {
            TypeRef::Entity(entity) => self.model.entities[*entity]
                .identifier
                .clone()
                .unwrap_or_else(|| "anonymous".to_string()),
            TypeRef::Primitive(primitive) => (*primitive).to_string(),
            TypeRef::Literal(value) => self.to_text(value),
            TypeRef::List(item) => format!("{}[]", self.type_text(item)),
            TypeRef::Union(items) => items
                .iter()
                .map(|item| self.type_text(item))
                .collect::<Vec<_>>()
                .join(" | "),
            TypeRef::Predicate(applied) => format!(
                "is {}",
                self.model.entities[*applied]
                    .identifier
                    .clone()
                    .unwrap_or_default()
            ),
            TypeRef::Function => "function".to_string(),
            TypeRef::Unknown(text) => text.trim().to_string(),
        }
    }

    /// A template's text: bodies as written, references rendered, executions evaluated.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn template_text(&mut self, r: NodeRef, env: &mut Env) -> String {
        let children: Vec<Child> = self.model.node(r).children.clone();
        let mut out = String::new();
        for child in &children {
            match child {
                Child::Token(index) => {
                    let token = self.model.token_at(r.file, *index);
                    if token.is(L::TemplateBody) {
                        let text = token.value.clone();
                        out.push_str(&text);
                    }
                }
                Child::Node(node) => {
                    let Some(child) = self.model.node_ref(r.file, node) else {
                        continue;
                    };
                    if self.model.is(child, E::TemplateReference) {
                        out.push_str("[[");
                        if let Some(reference) = self.model.child_nodes(child).into_iter().next() {
                            let text = self.reference_text(reference, env);
                            out.push_str(&text);
                        }
                        out.push_str("]]");
                    } else if self.model.is(child, E::TemplateExecution) {
                        match self.model.child_nodes(child).into_iter().next() {
                            Some(expression) => {
                                let value = self.eval(expression, env);
                                match value {
                                    Interim::Undefined => {
                                        let raw = self.model.raw(child);
                                        out.push_str(&raw);
                                    }
                                    other => {
                                        let text = self.to_text(&other);
                                        out.push_str(&text);
                                    }
                                }
                            }
                            None => {
                                let raw = self.model.raw(child);
                                out.push_str(&raw);
                            }
                        }
                    }
                }
                Child::Error(_) => {}
            }
        }
        out
    }

    /// How a reference inside a template renders: a declared name keeps its text, and a
    /// variable or parameter is replaced by what it holds.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn reference_text(&mut self, reference: NodeRef, env: &mut Env) -> String {
        let rule = self.model.rule_of(reference);
        if rule == E::Reference.entity() {
            return match self.model.child_nodes(reference).into_iter().next() {
                Some(inner) => self.reference_text(inner, env),
                None => String::new(),
            };
        }
        if rule == E::Name.entity() {
            let name = self.model.name(reference).unwrap_or_default();
            // Only a variable holding an entity is replaced; a string or a list keeps the
            // name as written, since it has no identifier.
            if let Some(Interim::Entity(entity)) = env.get(&name) {
                return self.model.entities[*entity]
                    .identifier
                    .clone()
                    .unwrap_or(name);
            }
            return name;
        }
        if rule == E::Dereference.entity() {
            return match self.model.child_nodes(reference).into_iter().next() {
                Some(operand) => self.reference_text(operand, env),
                None => String::new(),
            };
        }
        if rule == E::Current.entity() {
            let Some((accessor, name)) = self.model.accessor_and_name(reference) else {
                return self.model.raw(reference);
            };
            let head = self.model.entities[env.current]
                .identifier
                .clone()
                .unwrap_or_default();
            return match name {
                None => head,
                Some(name) => {
                    let raw = self.model.token_at(reference.file, accessor).raw.clone();
                    format!("{head}{raw}{name}")
                }
            };
        }
        if rule == E::Member.entity() {
            let Some(left) = self.model.left(reference) else {
                return self.model.raw(reference);
            };
            let Some((accessor, name)) = self.model.accessor_and_name(reference) else {
                return self.model.raw(reference);
            };
            let head = self.reference_text(left, env);
            let raw = self.model.token_at(reference.file, accessor).raw.clone();
            return match name {
                None => format!("{head}{raw}"),
                Some(name) => format!("{head}{raw}{name}"),
            };
        }
        self.model.raw(reference).trim().to_string()
    }
}
