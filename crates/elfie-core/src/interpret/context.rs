// @lfy def/interpret/main.lfy:expand
//! What compile-time code adds to an entity's context: knowledge, commands, targets,
//! criteria, and tests.

use super::*;

/// One piece of knowledge as a value: the object an `add` call was given, read back.
// @lfy def/interpret/main.lfy:expand
fn knowledge_value(item: &Knowledge) -> Interim {
    Interim::Object(vec![
        ("topic".to_string(), Interim::String(item.topic.clone())),
        (
            "kind".to_string(),
            Interim::String(item.kind.value().to_string()),
        ),
        ("source".to_string(), Interim::String(item.source.clone())),
        (
            "quote".to_string(),
            item.quote.map_or(Interim::Undefined, Interim::Bool),
        ),
    ])
}

/// One command as a value, read back the same way.
// @lfy def/interpret/main.lfy:expand
fn command_value(item: &Command) -> Interim {
    Interim::Object(vec![
        (
            "operation".to_string(),
            Interim::String(item.operation.value().to_string()),
        ),
        (
            "line".to_string(),
            item.line.clone().map_or(Interim::Undefined, Interim::String),
        ),
    ])
}

impl Interpreter<'_> {
    /// Appends one item per argument to a member of an entity's context that holds items,
    /// and yields the member's items, so calls chain.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn add_item(
        &mut self,
        call: NodeRef,
        added: Added,
        read: NodeRef,
        arguments: &[NodeRef],
        env: &mut Env,
    ) -> Interim {
        let Some(owner) = self.added_owner(read, env) else {
            return Interim::Undefined;
        };
        for &argument in arguments {
            match added {
                // One piece of knowledge with the topic, kind, source, and quote of the
                // object argument, its contributor the entity whose body holds the call.
                // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
                Added::Knowledge => {
                    let item = self.knowledge_of(argument, env);
                    self.model.entities[owner].knowledge.push(item);
                }
                // One command with the operation and line of the object argument.
                // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
                Added::Commands => {
                    let item = self.command_of(argument, env);
                    self.model.entities[owner].commands.push(item);
                }
                Added::Targets => self.add_target(call, owner, argument),
                Added::Criteria => {}
            }
        }
        self.added_items(owner, added)
    }

    /// The entity whose member an `add` call appends to: the current entity for a bare
    /// accessor, and what the left side names otherwise. A body run for a receiver appends
    /// to the receiver, as a criterion of a trait's body attaches to it, so what a trait
    /// adds to its current entity reaches each entity the trait is applied to.
    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    pub(super) fn added_owner(&mut self, read: NodeRef, env: &mut Env) -> Option<EntityId> {
        let owner = match self.model.left(read) {
            Some(left) => match self.eval(left, env) {
                Interim::Entity(entity) => entity,
                _ => resolve_name_node(self.model, left)
                    .map(|symbol| self.model.symbols[symbol].entity)?,
            },
            None => env.current,
        };
        Some(if env.in_trait && owner == env.current {
            env.target
        } else {
            owner
        })
    }

    /// What a member of an entity's context that holds items reads as.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn added_items(&mut self, owner: EntityId, added: Added) -> Interim {
        match added {
            Added::Criteria => Interim::Criteria(owner),
            Added::Knowledge => Interim::List(
                self.model.entities[owner]
                    .knowledge
                    .iter()
                    .map(knowledge_value)
                    .collect(),
            ),
            Added::Commands => Interim::List(
                self.model.entities[owner]
                    .commands
                    .iter()
                    .map(command_value)
                    .collect(),
            ),
            Added::Targets => Interim::List(
                self.model.entities[owner]
                    .targets
                    .iter()
                    .map(|&target| Interim::Entity(target))
                    .collect(),
            ),
        }
    }

    /// The fields of the object given to `add`: each key, the node of its value when it is
    /// written out, and the value.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn object_fields(
        &mut self,
        argument: NodeRef,
        env: &mut Env,
    ) -> Vec<(String, Option<NodeRef>, Interim)> {
        if self.model.is(argument, E::Object) {
            let mut out = Vec::new();
            for key in self.model.children_of(argument, E::ObjectKey) {
                let Some(name) = self.model.declared_name(key) else {
                    continue;
                };
                let node = self
                    .model
                    .child_nodes(key)
                    .into_iter()
                    .find(|&child| !self.model.is(child, E::Declared));
                let value = match node {
                    Some(node) => self.eval(node, env),
                    None => Interim::Undefined,
                };
                out.push((name, node, value));
            }
            return out;
        }
        match self.eval(argument, env) {
            Interim::Object(pairs) => pairs
                .into_iter()
                .map(|(key, value)| (key, None, value))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// One piece of knowledge from the object given to `add` on an entity's knowledge.
    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    pub(super) fn knowledge_of(&mut self, argument: NodeRef, env: &mut Env) -> Knowledge {
        let mut item = Knowledge {
            topic: String::new(),
            kind: KnowledgeKind::Reference,
            source: String::new(),
            quote: None,
            contributor: env.contributor,
        };
        for (key, node, value) in self.object_fields(argument, env) {
            match key.as_str() {
                "topic" => item.topic = self.to_text(&value),
                "kind" => {
                    if let Some(kind) = self.knowledge_kind(node, &value) {
                        item.kind = kind;
                    }
                }
                "source" => item.source = self.to_text(&value),
                "quote" => item.quote = Some(value.is_truthy()),
                _ => {}
            }
        }
        item
    }

    /// One command from the object given to `add` on an entity's commands. The object holds
    /// an operation, as the criteria of `Command` say, so `build` only stands in for one
    /// written without.
    // @lfy def/interpret/main.lfy:expand#expand:expand:b4bd0b259ce526434077c564906e508670bdc503c3ba155c178e39db8f76bce6
    pub(super) fn command_of(&mut self, argument: NodeRef, env: &mut Env) -> Command {
        let mut item = Command {
            operation: Operation::Build,
            line: None,
            contributor: env.contributor,
        };
        for (key, node, value) in self.object_fields(argument, env) {
            match key.as_str() {
                "operation" => {
                    if let Some(operation) = self.operation_named(node, &value) {
                        item.operation = operation;
                    }
                }
                "line" => item.line = Some(self.to_text(&value)),
                _ => {}
            }
        }
        item
    }

    /// The kind a field names: the member of `KnowledgeKind` it reads by name, or the one
    /// whose value it holds.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn knowledge_kind(&self, node: Option<NodeRef>, value: &Interim) -> Option<KnowledgeKind> {
        let named = self.member_named(node);
        [
            ("reference", KnowledgeKind::Reference),
            ("example", KnowledgeKind::Example),
            ("definition", KnowledgeKind::Definition),
            ("tool", KnowledgeKind::Tool),
        ]
        .into_iter()
        .find(|&(name, kind)| {
            named.as_deref() == Some(name) || value.as_str() == Some(kind.value())
        })
        .map(|(_, kind)| kind)
    }

    /// The operation a field names, read the same way.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn operation_named(&self, node: Option<NodeRef>, value: &Interim) -> Option<Operation> {
        let named = self.member_named(node);
        [
            ("install", Operation::Install),
            ("add", Operation::Add),
            ("build", Operation::Build),
            ("test", Operation::Test),
            ("lint", Operation::Lint),
            ("format", Operation::Format),
            ("run", Operation::Run),
        ]
        .into_iter()
        .find(|&(name, operation)| {
            named.as_deref() == Some(name) || value.as_str() == Some(operation.value())
        })
        .map(|(_, operation)| operation)
    }

    /// The name a field's value reads after an accessor, such as the member of an enum.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn member_named(&self, node: Option<NodeRef>) -> Option<String> {
        node.and_then(|node| self.model.accessor_and_name(node))
            .and_then(|(_, name)| name)
    }

    /// `@targets.add(t)`: the entity of the `ace const` the argument names joins the
    /// entity's targets.
    // @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
    pub(super) fn add_target(&mut self, call: NodeRef, owner: EntityId, argument: NodeRef) {
        // A member, a parameter, and an enum member are built with their owner.
        // @lfy def/interpret/main.lfy:expand#expand:expand:52171aea8ab1474de54983df55c484fadc588bd0d6e58e36b25e18a182ea17bc
        if matches!(
            self.model.entities[owner].kind,
            EntityKind::Member | EntityKind::Parameter | EntityKind::EnumMember
        ) {
            self.problem(
                call,
                "this is built with its owner, so no target is added to it",
            );
            return;
        }
        let Some(target) = self.target_const(argument) else {
            // The argument names no `ace const` of the project holding a `Target`.
            // @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
            let text = self.model.raw(argument).trim().to_string();
            self.problem(argument, format!("{text} names no target"));
            return;
        };
        // The same target twice.
        // @lfy def/interpret/main.lfy:expand#expand:expand:8159e82000937ae0a26519fa34e1579b5f6a3bcd6325987ebc1729fd11812494
        if self.model.entities[owner].targets.contains(&target) {
            let name = self.model.entities[target]
                .identifier
                .clone()
                .unwrap_or_default();
            self.problem(call, format!("the target {name} was added twice"));
            return;
        }
        self.model.entities[owner].targets.push(target);
    }

    /// The entity of the `ace const` an expression names, when that const declares a
    /// target.
    // @lfy def/interpret/main.lfy:expand#expand:expand:c6432d249e97137625897a6c990e7fe0c2f5065e9cc65067d40f95223983a6df
    pub(super) fn target_const(&self, argument: NodeRef) -> Option<EntityId> {
        let symbol = resolve_name_node(self.model, argument)?;
        let entity = self.model.symbols[symbol].entity;
        self.declares_a_target(entity).then_some(entity)
    }

    // ---- Criteria and tests ---------------------------------------------------------

    /// One criterion from the object given to `add`.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn criterion_of(&mut self, argument: NodeRef, env: &mut Env) -> Criterion {
        let object = self.eval(argument, env);
        let mut criterion = Criterion {
            contributor: env.contributor,
            node: Some(argument),
            ..Criterion::default()
        };
        if let Interim::Object(pairs) = object {
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

    // @lfy def/interpret/main.lfy:expand
    pub(super) fn texts_of(&self, value: &Interim) -> Vec<String> {
        match value {
            Interim::List(items) => items.iter().map(|item| self.to_text(item)).collect(),
            other => vec![self.to_text(other)],
        }
    }

    /// One test from the object given to `test`.
    // @lfy def/interpret/main.lfy:expand
    pub(super) fn test_of(&self, argument: NodeRef) -> Test {
        let mut test = Test {
            input: None,
            expect: None,
            input_text: String::new(),
            expect_text: String::new(),
        };
        if !self.model.is(argument, E::Object) {
            return test;
        }
        for key in self.model.children_of(argument, E::ObjectKey) {
            let name = self.model.declared_name(key).unwrap_or_default();
            let value = self
                .model
                .child_nodes(key)
                .into_iter()
                .find(|&c| !self.model.is(c, E::Declared));
            let text = value
                .map(|v| self.model.raw(v).trim().to_string())
                .unwrap_or_default();
            match name.as_str() {
                "input" => {
                    test.input = value;
                    test.input_text = text;
                }
                "expect" => {
                    test.expect = value;
                    test.expect_text = text;
                }
                _ => {}
            }
        }
        test
    }
}
