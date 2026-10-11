// @lfy def/interpret/main.lfy:evaluate
//! What the interpret step hands back: the value of an expression, computed at compile
//! time.

use super::*;

impl Interpreter<'_> {
    /// A value as the definition names one: the kinds a body needs only while it runs are
    /// not among them, so a type, a set of criteria, and a tester fold into nothing.
    // @lfy def/interpret/main.lfy:evaluate
    pub(super) fn value_of(&self, value: &Interim) -> Option<Value> {
        Some(match value {
            Interim::Undefined => Value::Undefined,
            Interim::Null => Value::Null,
            Interim::Bool(boolean) => Value::Boolean(*boolean),
            Interim::Number(number) => Value::Number(*number),
            Interim::String(text) => Value::String(text.clone()),
            Interim::List(items) => Value::List(
                items
                    .iter()
                    .map(|item| self.value_of(item))
                    .collect::<Option<Vec<Value>>>()?,
            ),
            Interim::Object(pairs) => Value::Object(
                pairs
                    .iter()
                    .map(|(key, value)| self.value_of(value).map(|value| (key.clone(), value)))
                    .collect::<Option<BTreeMap<String, Value>>>()?,
            ),
            Interim::Entity(entity) => Value::Entity(*entity),
            Interim::Scope(scope) => Value::Scope(*scope),
            Interim::Function(node, scope) => Value::Function(*node, *scope),
            Interim::Closure(node, _) => {
                Value::Function(*node, self.model.enclosing_scope(*node))
            }
            // A type names a declaration or it names none; only the first is a value.
            Interim::Type(ty) => match &**ty {
                TypeRef::Entity(entity) => Value::Entity(*entity),
                _ => return None,
            },
            Interim::Criteria(_) | Interim::Tester(_) | Interim::Like(_) => return None,
        })
    }

    /// The [`Prompted`] a call of `like` on an entity's context stands for, when the node is
    /// one.
    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:1b8998ed21283dddfecd045cb1ee1d2c46c6a9ab9d55452d0805a7b6da098491
    pub(super) fn prompted_of(&mut self, r: NodeRef, env: &mut Env) -> Option<Prompted> {
        if !self.model.is(r, E::Call) {
            return None;
        }
        let callee = self.model.left(r)?;
        let (accessor, name) = self.model.accessor_and_name(callee)?;
        if !self.model.token_is(callee.file, accessor, P::ContextAccessor) || name.as_deref()? != LIKE
        {
            return None;
        }
        let receiver = match self.model.left(callee) {
            Some(left) => self.eval(left, env),
            None => Interim::Entity(env.current),
        };
        let value_type = match &receiver {
            Interim::Entity(entity) => Some(*entity),
            Interim::Type(ty) => self.base_data(ty),
            Interim::String(_) => self.prelude_data("String"),
            Interim::Number(_) => self.prelude_data("Number"),
            Interim::Bool(_) => self.prelude_data("Boolean"),
            Interim::List(_) => self.prelude_data("List"),
            _ => None,
        }?;
        let argument = self.model.arguments(r).first().copied()?;
        let prompt = self.text_of(argument, env);
        Some(Prompted { value_type, prompt })
    }

    /// The value of an expression at compile time, in the scope it is written in.
    // @lfy def/interpret/main.lfy:evaluate#evaluate:evaluate:721fecc6bbf7d7e099e34bedb0683a009eb68ac5cd6d2ab534899e76c11507a5
    pub(super) fn evaluate_node(&mut self, node: NodeRef) -> Option<Evaluated> {
        let scope = self.model.enclosing_scope(node);
        let current = self.model.scopes[scope].current;
        let mut env = Env {
            vars: Vec::new(),
            current,
            target: current,
            contributor: current,
            scope,
            in_trait: false,
            dry: true,
            ace: false,
            ret: None,
        };
        // A call of like on an entity's context is a value only the compiler can choose.
        // @lfy def/interpret/main.lfy:evaluate
        self.runtime = false;
        if let Some(prompted) = self.prompted_of(node, &mut env)
            && !self.runtime
        {
            return Some(Evaluated::Prompted(prompted));
        }
        self.runtime = false;
        let value = self.eval(node, &mut env);
        // What only has a value at runtime is nothing here, and no problem of its own.
        // @lfy def/interpret/main.lfy:evaluate
        if self.runtime {
            return None;
        }
        self.value_of(&value).map(Evaluated::Value)
    }
}

/// The value of an expression, computed at compile time when something asks for it.
///
/// The node is evaluated in the scope it is written in, against the model as [`expand`]
/// left it, and the same node evaluated twice gives the same value. Nothing comes back when
/// the node depends on a parameter or a variable that only has a value at runtime, and no
/// problem is added for that; the caller decides whether it is one. A value that needs its
/// own, directly or through other nodes, gives nothing back and adds a problem naming the
/// cycle.
// @lfy def/interpret/main.lfy:evaluate
pub fn evaluate(model: &mut Model, node: NodeRef) -> Option<Evaluated> {
    let mut interpreter = Interpreter::new(model);
    interpreter.runs_functions = false;
    interpreter.writes = false;
    interpreter.evaluate_node(node)
}
