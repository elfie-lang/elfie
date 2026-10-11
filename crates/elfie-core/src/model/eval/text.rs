// @lfy def/model/main.lfy:bind
//! The criteria and tests a body records, and the text a value, a type, and a template
//! render to.

use super::*;

impl Binder {
    // ---- Criteria and tests -------------------------------------------------------

    /// One criterion from the object given to `add`.
    pub(super) fn criterion_of(&mut self, argument: NodeRef, env: &mut Env) -> Criterion {
        let object = self.eval(argument, env);
        let mut criterion = Criterion {
            contributor: env.contributor,
            node: Some(argument),
            ..Criterion::default()
        };
        if let Value::Object(pairs) = object {
            for (key, value) in pairs {
                let texts = self.texts_of(&value);
                match key.as_str() {
                    "situation" => criterion.situation = Some(texts),
                    "behavior" => criterion.behavior = Some(texts),
                    "sideEffects" => criterion.side_effects = Some(texts),
                    _ => {}
                }
            }
        }
        criterion
    }

    fn texts_of(&self, value: &Value) -> Vec<String> {
        match value {
            Value::List(items) => items.iter().map(|i| self.to_text(i)).collect(),
            other => vec![self.to_text(other)],
        }
    }

    /// One test from the object given to `@test`.
    pub(super) fn test_of(&self, argument: NodeRef) -> Test {
        let trees = &self.trees;
        let mut test = Test {
            input: None,
            expect: None,
            input_text: String::new(),
            expect_text: String::new(),
        };
        if trees.is(argument, E::Object) {
            for key in trees.children_of(argument, E::ObjectKey) {
                let name = trees
                    .child(key, E::Declared)
                    .and_then(|d| {
                        trees.child_token(
                            d,
                            crate::grammar::terminals::identifier::Identifier::Identifier,
                        )
                    })
                    .map(|i| trees.token(argument.file, i).value.clone())
                    .unwrap_or_default();
                let value = trees
                    .child_nodes(key)
                    .into_iter()
                    .find(|&c| !trees.is(c, E::Declared));
                match name.as_str() {
                    "input" => {
                        test.input = value;
                        test.input_text = value
                            .map(|v| trees.raw(v).trim().to_string())
                            .unwrap_or_default();
                    }
                    "expect" => {
                        test.expect = value;
                        test.expect_text = value
                            .map(|v| trees.raw(v).trim().to_string())
                            .unwrap_or_default();
                    }
                    _ => {}
                }
            }
        }
        test
    }

    // ---- Text -----------------------------------------------------------------------

    /// The text of a string or template expression, evaluated; other expressions give
    /// their value as text, or their source when they have none.
    pub(crate) fn text_of(&mut self, r: NodeRef, env: &Env) -> String {
        let mut env = env.clone();
        let value = self.eval(r, &mut env);
        match value {
            Value::Undefined => self.trees.raw(r).trim().to_string(),
            other => self.to_text(&other),
        }
    }

    pub(crate) fn to_text(&self, value: &Value) -> String {
        match value {
            Value::Undefined => "undefined".to_string(),
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => {
                if n.fract() == 0.0 {
                    format!("{}", *n as i64)
                } else {
                    n.to_string()
                }
            }
            Value::String(s) => s.clone(),
            Value::List(items) => items
                .iter()
                .map(|i| self.to_text(i))
                .collect::<Vec<_>>()
                .join(", "),
            Value::Object(pairs) => pairs
                .iter()
                .map(|(k, v)| format!("{k} = {}", self.to_text(v)))
                .collect::<Vec<_>>()
                .join(", "),
            Value::Entity(e) => self.model.entities[*e]
                .identifier
                .clone()
                .unwrap_or_else(|| "anonymous".to_string()),
            Value::Type(ty) => self.type_text(ty),
            Value::Scope(_) => "scope".to_string(),
            Value::Closure(..) | Value::Function(..) => "function".to_string(),
            Value::Criteria(_) => "acceptanceCriteria".to_string(),
            Value::Tester(_) => "test".to_string(),
            Value::Like(inner) => self.to_text(inner),
        }
    }

    pub(crate) fn type_text(&self, ty: &TypeRef) -> String {
        match ty {
            TypeRef::Entity(e) => self.model.entities[*e]
                .identifier
                .clone()
                .unwrap_or_else(|| "anonymous".to_string()),
            TypeRef::Primitive(p) => (*p).to_string(),
            TypeRef::Literal(v) => self.to_text(v),
            // A union or a function type is wrapped in parentheses before the brackets,
            // so the text reads back as the list of that whole type.
            TypeRef::List(item) => {
                let text = self.type_text(item);
                if matches!(item.as_ref(), TypeRef::Union(_) | TypeRef::Function) {
                    format!("({text})[]")
                } else {
                    format!("{text}[]")
                }
            }
            TypeRef::Union(items) => items
                .iter()
                .map(|i| self.type_text(i))
                .collect::<Vec<_>>()
                .join(" | "),
            TypeRef::Predicate(t) => format!(
                "is {}",
                self.model.entities[*t]
                    .identifier
                    .clone()
                    .unwrap_or_default()
            ),
            TypeRef::Function => "function".to_string(),
            TypeRef::Unknown(text) => text.trim().to_string(),
        }
    }

    /// A template's text: bodies as written, references rendered, executions evaluated.
    pub(super) fn template_text(&mut self, r: NodeRef, env: &mut Env) -> String {
        let trees = self.trees.clone();
        let node = trees.node(r);
        let tokens = trees.tokens(r.file);
        let mut out = String::new();
        for child in &node.children {
            match child {
                crate::parser::data::Child::Token(index) => {
                    let token = &tokens[*index];
                    if token.is(L::TemplateBody) {
                        out.push_str(&token.value);
                    }
                }
                crate::parser::data::Child::Node(child) => {
                    let child_ref = trees.child_ref(r.file, child);
                    if trees.is(child_ref, E::TemplateReference) {
                        out.push_str("[[");
                        if let Some(reference) = trees.child_nodes(child_ref).into_iter().next() {
                            out.push_str(&self.reference_text(reference, env));
                        }
                        out.push_str("]]");
                    } else if trees.is(child_ref, E::TemplateExecution) {
                        match trees.child_nodes(child_ref).into_iter().next() {
                            Some(expression) => {
                                let value = self.eval(expression, env);
                                match value {
                                    Value::Undefined => out.push_str(&trees.raw(child_ref)),
                                    other => out.push_str(&self.to_text(&other)),
                                }
                            }
                            None => out.push_str(&trees.raw(child_ref)),
                        }
                    }
                }
                crate::parser::data::Child::Error(_) => {}
            }
        }
        out
    }

    /// How a reference inside a template renders: a declared name keeps its text, a
    /// variable or parameter is replaced by what it holds.
    // Decision: `[[Token.file]]` stays `Token.file`; `[[&rule]]` becomes the identifier
    // of the entity the loop variable holds.
    fn reference_text(&mut self, reference: NodeRef, env: &mut Env) -> String {
        let trees = self.trees.clone();
        let rule = trees.rule(reference);
        if rule == E::Reference.entity() {
            return match trees.child_nodes(reference).into_iter().next() {
                Some(inner) => self.reference_text(inner, env),
                None => String::new(),
            };
        }
        if rule == E::Name.entity() {
            let name = trees.name(reference).unwrap_or_default();
            // Only a variable holding an entity is replaced; a string or list keeps the
            // name as written, since it has no identifier.
            if let Some(Value::Entity(e)) = env.get(&name) {
                return self.model.entities[*e].identifier.clone().unwrap_or(name);
            }
            return name;
        }
        if rule == E::Dereference.entity() {
            let Some(operand) = trees.child_nodes(reference).into_iter().next() else {
                return String::new();
            };
            return self.reference_text(operand, env);
        }
        if rule == E::Current.entity() {
            let Some((accessor, name)) = trees.accessor_and_name(reference) else {
                return trees.raw(reference);
            };
            let head = self.model.entities[env.current]
                .identifier
                .clone()
                .unwrap_or_default();
            return match name {
                None => head,
                Some(name) => format!(
                    "{head}{}{}",
                    trees.token(reference.file, accessor).raw,
                    trees.token(reference.file, name).value
                ),
            };
        }
        if rule == E::Member.entity() {
            let Some(left) = trees.left(reference) else {
                return trees.raw(reference);
            };
            let Some((accessor, name)) = trees.accessor_and_name(reference) else {
                return trees.raw(reference);
            };
            let head = self.reference_text(left, env);
            return match name {
                None => format!("{head}{}", trees.token(reference.file, accessor).raw),
                Some(name) => format!(
                    "{head}{}{}",
                    trees.token(reference.file, accessor).raw,
                    trees.token(reference.file, name).value
                ),
            };
        }
        trees.raw(reference).trim().to_string()
    }
}
