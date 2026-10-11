// @lfy def/interpret/main.lfy:lower
//! The program as generation sees it: runtime code only, with compile-time results in
//! place.

use super::*;

/// The expression rules a folded node never replaces, because their value is not all they
/// are: an assignment sets something, a spread spreads, an await waits, a cast reads a
/// value as a type, a definition attaches a description, and an inline function is a body.
// @lfy def/interpret/main.lfy:lower
fn never_folded(rule: Rule) -> bool {
    [
        E::Assignment.entity(),
        E::SpreadOperation.entity(),
        E::AwaitOperation.entity(),
        E::InOperation.entity(),
        E::OfOperation.entity(),
        E::FromOperation.entity(),
        E::Cast.entity(),
        E::Definition.entity(),
        E::InlineFunction.entity(),
        E::RangeOperation.entity(),
    ]
    .contains(&rule)
}

/// The rules that hold a name being declared or a type being written, rather than a value
/// being read: nothing inside one is folded, because what it stands for is its text.
// @lfy def/interpret/main.lfy:lower
fn declares_or_types(rule: Rule) -> bool {
    [
        E::Declared.entity(),
        E::Signature.entity(),
        E::Parameters.entity(),
        E::Parameter.entity(),
        E::SpreadParameter.entity(),
        E::TypeParameters.entity(),
        E::TypeParameter.entity(),
        E::TypeArguments.entity(),
        E::TypeExpression.entity(),
        E::TypeItem.entity(),
        E::TypeGroup.entity(),
        E::TypeValue.entity(),
        E::TypeKey.entity(),
        E::FunctionType.entity(),
        E::DefinitionClause.entity(),
        E::MemberName.entity(),
    ]
    .contains(&rule)
}

/// Lowering one workspace.
// @lfy def/interpret/main.lfy:lower
struct Lowering<'m> {
    interpreter: Interpreter<'m>,
    /// The entities whose criteria are the guidance of a target rather than criteria of the
    /// program.
    // @lfy def/interpret/main.lfy:lower
    guidance: HashSet<EntityId>,
    /// The local criteria of each entity, in order.
    criteria: HashMap<EntityId, Vec<LoweredCriterion>>,
    /// The local tests of each entity, in order.
    tests: HashMap<EntityId, Vec<LoweredTest>>,
    /// Every global criterion, in the order they were added.
    global_criteria: Vec<LoweredCriterion>,
    /// Every global test, in the order they were added.
    global_tests: Vec<LoweredTest>,
    /// Every problem lowering added.
    problems: Vec<Problem>,
    /// Where the project's own source lives, relative to the workspace root: what an added
    /// use's alias is named after.
    // @lfy def/interpret/main.lfy:lower
    source_directory: String,
    /// The name and directory of the package each file came from, by source index.
    // @lfy def/interpret/main.lfy:lower
    packages: HashMap<FileId, (String, String)>,
}

/// The uses one lowered file's text takes beyond the ones written in it, and the alias each
/// entity whose name would otherwise mean two things is spelled through.
// @lfy def/interpret/main.lfy:lower
#[derive(Default)]
struct AddedUses {
    /// One `Use` per file, in the order the members that need it are spelled.
    // @lfy def/interpret/main.lfy:lower
    uses: Vec<String>,
    /// The alias an entity is spelled through, for every entity of an aliased use.
    // @lfy def/interpret/main.lfy:lower
    aliases: HashMap<EntityId, String>,
}

/// The program as generation sees it: runtime code only, with compile-time results in place.
///
/// Every node whose [`phase_of`] is [`Phase::Runtime`], that is not inside a
/// [`Phase::Compile`] node or an `ace` function, that is not replaced by a folded node, and
/// that is not an expression statement whose expression is [`Phase::Compile`], is kept
/// with the same rule and its kept children in order; every lowered node's origin is the
/// parse node it came from, and a folded node's origin is the expression it replaced.
/// Every criterion and every test of every entity is lowered once, into a
/// [`LoweredCriterion`] or a [`LoweredTest`] with its id, and never into the tree's
/// children.
// @lfy def/interpret/main.lfy:lower
pub fn lower(mut workspace: Workspace) -> Program {
    let mut model = std::mem::take(&mut workspace.model);
    let mut files = Vec::new();
    let (criteria, tests, problems);
    {
        let mut lowering = Lowering::new(&mut model, &workspace);
        lowering.take_requirements();
        // One lowered file per file of the workspace, in the same order.
        // @lfy def/interpret/main.lfy:lower
        for (index, file) in workspace.files.iter().enumerate() {
            files.push(lowering.lower_file(index, file.source));
        }
        criteria = std::mem::take(&mut lowering.global_criteria);
        tests = std::mem::take(&mut lowering.global_tests);
        problems = std::mem::take(&mut lowering.problems);
    }
    workspace.model = model;
    Program {
        workspace,
        files,
        criteria,
        tests,
        problems,
    }
}

impl<'m> Lowering<'m> {
    // @lfy def/interpret/main.lfy:lower
    fn new(model: &'m mut Model, workspace: &Workspace) -> Lowering<'m> {
        let guidance = guidance_entities(workspace);
        let mut interpreter = Interpreter::new(model);
        interpreter.runs_functions = false;
        interpreter.writes = false;
        Lowering {
            interpreter,
            guidance,
            criteria: HashMap::new(),
            tests: HashMap::new(),
            global_criteria: Vec::new(),
            global_tests: Vec::new(),
            problems: Vec::new(),
            source_directory: workspace.source_directory.clone(),
            packages: workspace
                .files
                .iter()
                .filter_map(|file| {
                    let package = workspace.packages.get(file.package?)?;
                    Some((
                        file.source,
                        (package.identifier.clone(), package.root.clone()),
                    ))
                })
                .collect(),
        }
    }

    // @lfy def/interpret/main.lfy:lower
    fn model(&self) -> &Model {
        self.interpreter.model
    }

    /// Lowers every criterion and every test of every entity, once, and sorts each into the
    /// place it belongs: a global one into the program, a local one under its entity.
    // @lfy def/interpret/main.lfy:lower#lower:lower:958d30dcc98b23d0e9373cc5d9ee9503e4e30d4587d485a2f01b9a25cdb0a2d8
    fn take_requirements(&mut self) {
        let global = self.model().global;
        let mut criteria: Vec<LoweredCriterion> = Vec::new();
        let mut tests: Vec<LoweredTest> = Vec::new();
        for entity in 0..self.model().entities.len() {
            let is_global = entity == global;
            let receiver = receiver_name(self.model(), Some(entity));
            let receiver = if is_global {
                receiver_name(self.model(), None)
            } else {
                receiver
            };
            let own = self.model().entities[entity].acceptance_criteria.clone();
            // A criterion the declaration of a target holds, its own or from a layer of the
            // target, is guidance for that target and neither a local nor a global
            // requirement: every request of the target already carries it as guidance.
            // @lfy def/interpret/main.lfy:lower#lower:lower:3032db4bcf2ffe2031d9fceb6ceced6ed34b2271c3cc994fa6cfb219215ef323
            if !self.guidance.contains(&entity) {
                for criterion in own {
                    criteria.push(self.lowered_criterion(entity, is_global, &receiver, &criterion));
                }
            }
            let cases = self.model().entities[entity].tests.clone();
            for case in cases {
                tests.push(self.lowered_test(entity, is_global, &receiver, &case));
            }
        }
        LoweredCriterion::number_repeats(&mut criteria);
        LoweredTest::number_repeats(&mut tests);
        // Two entities may share a name, so two requirements of different entities may still
        // share an id; they are numbered the same way, so that one id names one requirement.
        // @lfy def/interpret/main.lfy:lower
        let numbered = number_collisions(criteria.iter().map(|c| c.id.clone()).collect());
        for (criterion, id) in criteria.iter_mut().zip(numbered) {
            criterion.id = id;
        }
        let numbered = number_collisions(tests.iter().map(|case| case.id.clone()).collect());
        for (case, id) in tests.iter_mut().zip(numbered) {
            case.id = id;
        }
        for criterion in criteria {
            match criterion.scope {
                // A global criterion is in the program once, with global in its id and no
                // entity, and in no node or file.
                // @lfy def/interpret/main.lfy:lower#lower:lower:8b72d813b6781ac6a5965eeefda77688fb7586238258b0ef7bbfaf22db866dd6
                RequirementScope::Global => self.global_criteria.push(criterion),
                RequirementScope::Local => {
                    let entity = criterion.entity.expect("a local criterion is for an entity");
                    self.criteria.entry(entity).or_default().push(criterion);
                }
            }
        }
        for case in tests {
            match case.scope {
                // A global test is in the program once, with global in its id and no
                // entity, and in no node or file.
                // @lfy def/interpret/main.lfy:lower#lower:lower:699284343b9ef81cb11db5668663aac649e418507ad5cea495ddceee254e4d43
                RequirementScope::Global => self.global_tests.push(case),
                RequirementScope::Local => {
                    let entity = case.entity.expect("a local test is for an entity");
                    self.tests.entry(entity).or_default().push(case);
                }
            }
        }
    }

    // @lfy def/interpret/main.lfy:lower
    fn lowered_criterion(
        &mut self,
        entity: EntityId,
        is_global: bool,
        receiver: &str,
        criterion: &Criterion,
    ) -> LoweredCriterion {
        let contributor = receiver_name(self.model(), Some(criterion.contributor));
        let origin = criterion
            .node
            .or(self.model().entities[criterion.contributor].node)
            .unwrap_or(SYNTHETIC);
        let references = self.references_of(criterion, origin);
        let rendered = Criterion {
            situation: strip_each(&criterion.situation),
            behavior: strip_each(&criterion.behavior),
            side_effects: strip_each(&criterion.side_effects),
            contributor: criterion.contributor,
            node: criterion.node,
        };
        LoweredCriterion {
            id: LoweredCriterion::id(receiver, &contributor, &rendered),
            scope: if is_global {
                RequirementScope::Global
            } else {
                RequirementScope::Local
            },
            entity: (!is_global).then_some(entity),
            origin,
            references,
            contributor: criterion.contributor,
            situation: rendered.situation,
            behavior: rendered.behavior,
            side_effects: rendered.side_effects,
        }
    }

    // @lfy def/interpret/main.lfy:lower
    fn lowered_test(
        &mut self,
        entity: EntityId,
        is_global: bool,
        receiver: &str,
        case: &Test,
    ) -> LoweredTest {
        let contributor = receiver;
        let origin = case
            .input
            .or(case.expect)
            .or(self.model().entities[entity].node)
            .unwrap_or(SYNTHETIC);
        let input_value = self.evaluated_or_undefined(case.input);
        let expect_value = self.evaluated_or_undefined(case.expect);
        LoweredTest {
            id: LoweredTest::id(receiver, contributor, case),
            scope: if is_global {
                RequirementScope::Global
            } else {
                RequirementScope::Local
            },
            entity: (!is_global).then_some(entity),
            origin,
            input_value,
            expect_value,
            input: case.input,
            expect: case.expect,
            input_text: case.input_text.clone(),
            expect_text: case.expect_text.clone(),
        }
    }

    /// A test's input or expectation, evaluated; what only runtime could answer is
    /// undefined.
    // @lfy def/interpret/main.lfy:lower
    fn evaluated_or_undefined(&mut self, node: Option<NodeRef>) -> Evaluated {
        node.and_then(|node| self.interpreter.evaluate_node(node))
            .unwrap_or(Evaluated::Value(Value::Undefined))
    }

    /// Every entity a criterion names with a bracketed reference, in order, kept as links
    /// rather than rendered names.
    // @lfy def/interpret/main.lfy:lower
    fn references_of(&self, criterion: &Criterion, origin: NodeRef) -> Vec<EntityId> {
        let scope = if origin == SYNTHETIC {
            self.model()
                .entities[criterion.contributor]
                .scope
                .unwrap_or(0)
        } else {
            self.model().enclosing_scope(origin)
        };
        let mut out = Vec::new();
        for text in [
            &criterion.situation,
            &criterion.behavior,
            &criterion.side_effects,
        ]
        .into_iter()
        .flatten()
        .flatten()
        {
            for name in bracketed(text) {
                if let Some(entity) = self.reference_entity(scope, &name)
                    && !out.contains(&entity)
                {
                    out.push(entity);
                }
            }
        }
        out
    }

    /// The entity a bracketed name stands for: its head looked up in the scope the
    /// criterion was written in, then each part after a dot as a member of the one before.
    // @lfy def/interpret/main.lfy:lower
    fn reference_entity(&self, scope: ScopeId, name: &str) -> Option<EntityId> {
        let mut parts = name.split('.');
        let head = parts.next()?;
        let symbol = lookup(self.model(), scope, head.trim())?;
        let mut entity = self.model().symbols[symbol].entity;
        for part in parts {
            let member = member_symbol(self.model(), entity, part.trim())?;
            entity = self.model().symbols[member].entity;
        }
        Some(entity)
    }

    // ---- The tree ------------------------------------------------------------------

    /// One file as generation sees it.
    // @lfy def/interpret/main.lfy:lower
    fn lower_file(&mut self, index: FileId, source: FileId) -> LoweredFile {
        let root = NodeRef {
            file: source,
            index: 0,
        };
        // A file that holds only compile-time statements lowers to an empty SourceFile.
        // @lfy def/interpret/main.lfy:lower
        let lowered = self.lower_node(root).unwrap_or(LoweredNode {
            rule: F::SourceFile.entity(),
            children: Vec::new(),
            origin: root,
            entity: self.model().file_entities.get(source).copied(),
            value: None,
            criteria: Vec::new(),
            tests: Vec::new(),
        });
        // A member's type may name an entity of a file this one does not use, because a
        // trait of another file added the member; the text takes a use of that file so the
        // name it spells means something.
        // @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
        // @lfy def/interpret/main.lfy:lower#lower:lower:3f6b1de42f55afc64a02297dcd69a75780d820b3b323c4b3cb98fe5fd662c66d
        let added = self.added_uses(source, &lowered);
        let text = render(self.model(), &lowered, &added.aliases);
        let text = with_uses(&text, &added.uses);
        let own = self.model().file_entities.get(source).copied();
        // A criterion or a test for the file's own entity joins the file, not a node.
        // @lfy def/interpret/main.lfy:lower
        // @lfy def/interpret/main.lfy:lower
        let criteria = own
            .and_then(|entity| self.criteria.remove(&entity))
            .unwrap_or_default();
        let tests = own
            .and_then(|entity| self.tests.remove(&entity))
            .unwrap_or_default();
        LoweredFile {
            file: index,
            root: lowered,
            text,
            criteria,
            tests,
        }
    }

    /// The uses a lowered file's text takes beyond the ones written in it: one per file a
    /// member's type names and the file does not use, however many members name it, in the
    /// order those members are spelled.
    ///
    /// Decision: a file of the library or the prelude is never added, because what it
    /// declares is in scope in every file already; a file of a package is added by the
    /// package's name, a slash, and its path under the package.
    // @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
    fn added_uses(&self, source: FileId, lowered: &LoweredNode) -> AddedUses {
        let mut added = AddedUses::default();
        let model = self.model();
        if model.sources[source].origin != Origin::Program {
            return added;
        }
        let mut named = Vec::new();
        member_type_entities(model, lowered, &mut named);
        // One use carries every entity of its file, so the file decides whether the names
        // it brings need an alias and each of them is spelled the same way.
        // @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
        let mut by_file: Vec<(FileId, Vec<EntityId>)> = Vec::new();
        for entity in named {
            let Some(node) = model.entities[entity].node else {
                continue;
            };
            let declared = node.file;
            if declared == source || declared >= model.sources.len() {
                continue;
            }
            if model.sources[declared].origin != Origin::Program
                || model.entities[entity].identifier.is_none()            {
                continue;
            }
            let path = &model.sources[declared].path;
            if model.sources[source]
                .uses
                .iter()
                .flatten()
                .any(|used| used == path)
            {
                continue;
            }
            match by_file.iter_mut().find(|(file, _)| *file == declared) {
                Some((_, entities)) => {
                    if !entities.contains(&entity) {
                        entities.push(entity);
                    }
                }
                None => by_file.push((declared, vec![entity])),
            }
        }
        let scope = model.file_scopes[source];
        for (declared, entities) in by_file {
            // The use takes an alias when the file already sees another entity by one of
            // the names it brings, because one name cannot mean two entities.
            // @lfy def/interpret/main.lfy:lower#lower:lower:3f6b1de42f55afc64a02297dcd69a75780d820b3b323c4b3cb98fe5fd662c66d
            let taken = entities.iter().any(|&entity| {
                let name = model.entities[entity].identifier.as_deref().unwrap_or("");
                model
                    .lookup(scope, name)
                    .is_some_and(|symbol| model.symbols[symbol].entity != entity)
            });
            // A file of a package is spelled by the package's name, never relative to the
            // owner's file.
            // @lfy def/interpret/main.lfy:lower#lower:lower:7e159bfaa7ce0d4765fa4fdbd906f7230afdf9fce2ae0297f29ee8f15f2af929
            // @lfy def/interpret/main.lfy:lower#lower:lower:28987d915312dda8a1c80b32f3c8d343bef0cf0d86b3f288a863238a9a9837fa
            let path = match self.packages.get(&declared) {
                Some((name, root)) => package_use_path(name, root, &model.sources[declared].path),
                None => use_path(&model.sources[source].path, &model.sources[declared].path),
            };
            if !taken {
                added.uses.push(format!("use \"{path}\";"));
                continue;
            }
            let base = alias_of(&self.source_directory, &model.sources[declared].path);
            let mut alias = base.clone();
            let mut number = 0usize;
            while model.lookup(scope, &alias).is_some()
                || added.aliases.values().any(|taken| *taken == alias)
            {
                number += 1;
                alias = format!("{base}{number}");
            }
            added.uses.push(format!("use \"{path}\" as {alias};"));
            for entity in entities {
                added.aliases.insert(entity, alias.clone());
            }
        }
        added
    }

    /// One node of the program as generation sees it, or nothing when it runs at compile
    /// time.
    // @lfy def/interpret/main.lfy:lower
    fn lower_node(&mut self, r: NodeRef) -> Option<LoweredNode> {
        // A node whose phase is compile has no lowered node, and neither does anything
        // inside it; nor does an expression statement whose expression runs at compile time.
        // @lfy def/interpret/main.lfy:lower#lower:lower:3dd593ebd9e4eded0e675a008bfbb128a421d899ec79d8273982fb718148de70
        if self.is_dropped(r) {
            return None;
        }
        self.check_statement(r);
        if let Some(value) = self.fold(r) {
            // A folded node's origin is the expression it replaced.
            // @lfy def/interpret/main.lfy:lower
            return Some(LoweredNode {
                rule: self.model().rule_of(r),
                children: Vec::new(),
                origin: r,
                entity: None,
                value: Some(value),
                criteria: Vec::new(),
                tests: Vec::new(),
            });
        }
        let mut children: Vec<LoweredChild> = Vec::new();
        for child in self.model().node(r).children.clone() {
            match child {
                Child::Token(index) => {
                    let token = self.model().token_at(r.file, index).clone();
                    children.push(LoweredChild::Token(token));
                }
                // Trivia is kept as the tokens it is written with, so that every lowered
                // node of a tree stands for something the program says.
                // @lfy def/interpret/main.lfy:lower
                Child::Node(node) if is_trivia(node.rule) => {
                    for index in node.token_indices() {
                        let token = self.model().token_at(r.file, index).clone();
                        children.push(LoweredChild::Token(token));
                    }
                }
                Child::Node(node) => {
                    let Some(child) = self.model().node_ref(r.file, &node) else {
                        continue;
                    };
                    match self.lower_node(child) {
                        Some(lowered) => children.push(LoweredChild::Node(lowered)),
                        // What is dropped takes the space written before it with it, so the
                        // text of a kept declaration reads as though the clause or the body
                        // had never been written.
                        // @lfy def/interpret/main.lfy:lower
                        None => while_trivia_pop(&mut children),
                    }
                }
                Child::Error(_) => {}
            }
        }
        let entity = self.model().symbol_of(r).map(|s| self.model().symbols[s].entity);
        let entity = entity.or_else(|| {
            self.model()
                .is(r, F::SourceFile)
                .then(|| self.model().file_entities[r.file])
        });
        // A kept declaration of an entity with members has one child node per member, in
        // member order, after its other kept children.
        // @lfy def/interpret/main.lfy:lower#lower:lower:5531392df6668cb6852deb49090314e7229171d099b764df568cd918c1eeb2c0
        if let Some(entity) = entity {
            for member in self.member_nodes(entity, r) {
                children.push(LoweredChild::Node(member));
            }
        }
        // A criterion or a test for the entity a node declares joins that node.
        // @lfy def/interpret/main.lfy:lower#lower:lower:8ae967486c3764064704fd7f57756f5f55bfb6c3e2550eab46a24b06e02c3040
        // @lfy def/interpret/main.lfy:lower#lower:lower:8b962cee67dcd0e481fc64461f7e97a28f49d8736c64ea75d7bf8661cbbaeb0b
        let is_file = self.model().is(r, F::SourceFile);
        let criteria = match entity {
            Some(entity) if !is_file => self.criteria.remove(&entity).unwrap_or_default(),
            _ => Vec::new(),
        };
        let tests = match entity {
            Some(entity) if !is_file => self.tests.remove(&entity).unwrap_or_default(),
            _ => Vec::new(),
        };
        // A node that is kept keeps its rule, and its kept children in the order they were
        // written.
        // @lfy def/interpret/main.lfy:lower#lower:lower:780cd0f400942687d110a357f183ca0c8d29ee46a09884327d3627fb3e2676ac
        Some(LoweredNode {
            rule: self.model().rule_of(r),
            children,
            // Every lowered node's origin is the parse node it came from.
            // @lfy def/interpret/main.lfy:lower
            origin: r,
            entity,
            value: None,
            criteria,
            tests,
        })
    }

    /// Whether a node has no lowered node: it runs at compile time, or it is an expression
    /// statement whose expression runs at compile time — an `add` on an entity's criteria, a
    /// `test` on its context, an `apply` on a trait — so nothing of it is left to keep and
    /// the statement goes with its expression rather than staying as a bare semicolon.
    // @lfy def/interpret/main.lfy:lower
    fn is_dropped(&self, r: NodeRef) -> bool {
        if phase_of(self.model(), r) == Phase::Compile {
            return true;
        }
        self.model().is(r, S::ExpressionStatement)
            && self
                .model()
                .child_nodes(r)
                .first()
                .is_some_and(|&expression| phase_of(self.model(), expression) == Phase::Compile)
    }

    /// One lowered node per member of an entity, in member order; a member the declaration
    /// still spells itself, such as a key of an enum or of a type, is already kept.
    // @lfy def/interpret/main.lfy:lower
    fn member_nodes(&mut self, entity: EntityId, declaration: NodeRef) -> Vec<LoweredNode> {
        let members: Vec<SymbolId> = self
            .model()
            .members(entity)
            .into_iter()
            .filter(|&symbol| {
                let record = &self.model().symbols[symbol];
                if !matches!(record.kind, SymbolKind::Member | SymbolKind::EnumMember) {
                    return false;
                }
                !(within(self.model(), record.node, declaration)
                    && phase_of(self.model(), record.node) == Phase::Runtime)
            })
            .collect();
        let mut out = Vec::new();
        for member in members {
            let declared = self.model().symbols[member].node;
            // The origin is the statement that declared the member, which for a member a
            // trait added is the statement in the trait's file.
            // @lfy def/interpret/main.lfy:lower#lower:lower:5531392df6668cb6852deb49090314e7229171d099b764df568cd918c1eeb2c0
            // @lfy def/interpret/main.lfy:lower#lower:lower:dfcda962889085ca36762f22b94e05974c5cc36a48a79b45284c3938ccfe0bb5
            let origin = statement_of(self.model(), declared);
            let held = self.model().symbols[member].entity;
            // A member written as a key of a type or of an enum is that key; one written as
            // a statement of a body is that statement.
            let rule = if self.model().is(declared, E::TypeKey)
                || self.model().is(declared, E::ObjectKey)
            {
                self.model().rule_of(declared)
            } else {
                self.model().rule_of(origin)
            };
            out.push(LoweredNode {
                rule,
                children: Vec::new(),
                origin,
                entity: Some(held),
                value: None,
                criteria: self.criteria.remove(&held).unwrap_or_default(),
                tests: self.tests.remove(&held).unwrap_or_default(),
            });
        }
        out
    }

    /// The problems a kept statement or call adds.
    // @lfy def/interpret/main.lfy:lower
    fn check_statement(&mut self, r: NodeRef) {
        // A file-level `let` is not allowed, and it is kept.
        // @lfy def/interpret/main.lfy:lower#lower:lower:c89173885cbe173a76e8a9a59310d5e69174bce617247c866b7d675aeeb308e2
        if self.model().is(r, S::VariableDeclaration)
            && self.model().has_token(r, K::LetKeyword)
            && self
                .model()
                .parent(r)
                .is_some_and(|up| self.model().is(up, F::SourceFile))
        {
            self.problems.push(Problem {
                node: r,
                message: "a file-level let is not allowed".to_string(),
                stage: crate::model::Stage::Binder,
            });
        }
        // A function that is not ace whose body holds statements, every one of which runs at
        // compile time: the statements run as they would anywhere else, none of them has a
        // lowered node, and the function is named, because its body is empty at runtime. A
        // function inside an `ace` never reaches here: the `ace` runs at compile time, so
        // nothing inside it is lowered at all.
        // @lfy def/interpret/main.lfy:lower#lower:lower:72779fb5f9a2ff6556d57709eb8eaebdb63e30288aa7374e2f904480042c28a7
        if self.model().is(r, S::FunctionDeclaration)
            && let Some(block) = self.model().child(r, S::Block)
        {
            let statements = self.model().child_nodes(block);
            if !statements.is_empty() && statements.iter().all(|&item| self.is_dropped(item)) {
                self.problems.push(Problem {
                    node: r,
                    message: "this function's body is empty at runtime, so it should be ace or \
                              hold runtime code"
                        .to_string(),
                    stage: crate::model::Stage::Binder,
                });
            }
        }
        // Runtime code that calls a function which runs wherever its call is written — an
        // ace function, or a function declared outside the program's runtime code — with an
        // argument no value can be taken for has nothing to call, because the function will
        // not exist at runtime.
        // @lfy def/interpret/main.lfy:lower#lower:lower:62d2d27335ac81028f0c9e82819680564bcc9debd24a3e002e179a810efaa49f
        if self.model().is(r, E::Call)
            && self.calls_a_compile_time_function(r)
            && self.interpreter.evaluate_node(r).is_none()
        {
            self.problems.push(Problem {
                node: r,
                message: "this calls a function that runs at compile time with an argument that \
                          has no value there, and the function will not exist at runtime"
                    .to_string(),
                stage: crate::model::Stage::Binder,
            });
        }
    }

    /// Whether a call names a written function that runs wherever its call is written: one
    /// declared inside an `ace`, and one declared outside the program's runtime code, such
    /// as a function of a package. A `fn` is left out: one carrying `builtin` is performed
    /// natively and one carrying none is generated, so either is there at runtime.
    // @lfy def/interpret/main.lfy:lower#lower:lower:62d2d27335ac81028f0c9e82819680564bcc9debd24a3e002e179a810efaa49f
    fn calls_a_compile_time_function(&self, call: NodeRef) -> bool {
        let Some(callee) = self.model().left(call) else {
            return false;
        };
        let Some(symbol) = resolve_name_node(self.model(), callee) else {
            return false;
        };
        let entity = self.model().symbols[symbol].entity;
        if !matches!(
            self.model().entities[entity].kind,
            EntityKind::Fn { agent: false, .. }
        ) {
            return false;
        }
        self.model().entities[entity]
            .node
            .is_some_and(|node| self.interpreter.runs_wherever_called(node))
    }

    /// The value a folded node holds, when the node is one.
    // @lfy def/interpret/main.lfy:lower
    fn fold(&mut self, r: NodeRef) -> Option<Evaluated> {
        let rule = self.model().rule_of(r);
        let expression =
            rule.is_primary() || rule.is_prefix() || rule.is_infix() || rule.is_postfix();
        if !expression || never_folded(rule) {
            return None;
        }
        // Nothing inside a declared name or a written type is folded: what it stands for is
        // its text.
        if self.no_fold_region(r) {
            return None;
        }
        // A call is folded only when it is a call of like, or of a function that runs
        // wherever its call is written and so will not exist at runtime.
        // @lfy def/interpret/main.lfy:lower
        if rule == E::Call.entity() {
            let mut env = self.interpreter.file_env(r.file);
            if let Some(prompted) = self.interpreter.prompted_of(r, &mut env) {
                return Some(Evaluated::Prompted(prompted));
            }
            if !self.calls_a_compile_time_function(r) {
                return None;
            }
        }
        self.interpreter.evaluate_node(r)
    }

    /// Whether a node stands inside a declared name or a written type.
    // @lfy def/interpret/main.lfy:lower
    fn no_fold_region(&self, r: NodeRef) -> bool {
        let mut at = Some(r);
        while let Some(node) = at {
            let rule = self.model().rule_of(node);
            if declares_or_types(rule) {
                return true;
            }
            if rule.is_statement() || rule == F::SourceFile.entity() {
                return false;
            }
            at = self.model().parent(node);
        }
        false
    }
}

/// Every entity whose criteria are the guidance of a target: the declaration of each one,
/// which carries its own criteria and those of each of its layers.
///
/// [`Target::declaration`]: crate::workspace::Target
// @lfy def/interpret/main.lfy:lower
fn guidance_entities(workspace: &Workspace) -> HashSet<EntityId> {
    workspace
        .targets
        .iter()
        .map(|target| target.declaration)
        .collect()
}

/// Numbers the ids that still repeat: the first keeps its id and the second and later ones
/// end with a colon and their position among those, counting from 2.
// @lfy def/interpret/main.lfy:lower
fn number_collisions(ids: Vec<String>) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    ids.into_iter()
        .map(|id| {
            let position = seen.entry(id.clone()).or_insert(0);
            *position += 1;
            if *position == 1 {
                id
            } else {
                format!("{id}:{position}")
            }
        })
        .collect()
}

/// Whether a node is the given one or stands inside it.
// @lfy def/interpret/main.lfy:lower
pub(super) fn within(model: &Model, node: NodeRef, ancestor: NodeRef) -> bool {
    let mut at = Some(node);
    while let Some(current) = at {
        if current == ancestor {
            return true;
        }
        at = model.parent(current);
    }
    false
}

/// The nearest statement at or above a node, or the node itself when there is none.
// @lfy def/interpret/main.lfy:lower
fn statement_of(model: &Model, r: NodeRef) -> NodeRef {
    let mut at = Some(r);
    while let Some(node) = at {
        if model.rule_of(node).is_statement() {
            return node;
        }
        at = model.parent(node);
    }
    r
}

/// Drops the trivia tokens written just before something that was left out.
// @lfy def/interpret/main.lfy:lower
fn while_trivia_pop(children: &mut Vec<LoweredChild>) {
    while let Some(LoweredChild::Token(token)) = children.last() {
        let trivia = token.rule.is_some_and(is_trivia);
        if !trivia {
            return;
        }
        children.pop();
    }
}

/// `[[X]]` becomes `X` in each text of a criterion.
// @lfy def/interpret/main.lfy:lower
fn strip_each(texts: &Option<Vec<String>>) -> Option<Vec<String>> {
    texts
        .as_ref()
        .map(|texts| texts.iter().map(|text| strip_references(text)).collect())
}

/// The names a text holds in brackets, in order.
// @lfy def/interpret/main.lfy:lower
fn bracketed(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        rest = &rest[open + 2..];
        let Some(close) = rest.find("]]") else { break };
        out.push(rest[..close].to_string());
        rest = &rest[close + 2..];
    }
    out
}

/// Every entity the type of a lowered member node names, in the order the members are
/// spelled, each once.
// @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
fn member_type_entities(model: &Model, node: &LoweredNode, out: &mut Vec<EntityId>) {
    if is_member_node(model, node)
        && let Some(entity) = node.entity
        && let Some(ty) = &model.entities[entity].ty
    {
        type_entities(ty, out);
    }
    for child in node.nodes() {
        member_type_entities(model, child, out);
    }
}

/// Every entity a written type names, in order, each once.
// @lfy def/interpret/main.lfy:lower
fn type_entities(ty: &TypeRef, out: &mut Vec<EntityId>) {
    match ty {
        TypeRef::Entity(entity) | TypeRef::Predicate(entity) => {
            if !out.contains(entity) {
                out.push(*entity);
            }
        }
        TypeRef::List(item) => type_entities(item, out),
        TypeRef::Union(items) => items.iter().for_each(|item| type_entities(item, out)),
        _ => {}
    }
}

/// The path a use of one file is written with from another: the file's path without its
/// extension, relative to the directory of the file the use is written in.
// @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
fn use_path(from: &str, to: &str) -> String {
    let stem = to.strip_suffix(".lfy").unwrap_or(to);
    let here: Vec<&str> = from.split('/').collect();
    let here = &here[..here.len().saturating_sub(1)];
    let there: Vec<&str> = stem.split('/').collect();
    let shared = here
        .iter()
        .zip(there.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<&str> = vec![".."; here.len() - shared];
    parts.extend(there[shared..].iter().copied());
    let path = parts.join("/");
    if path.starts_with('.') {
        path
    } else {
        format!("./{path}")
    }
}

/// The alias an added use takes: the declaring file's path relative to the project's source
/// directory, without its extension and with each slash replaced by an underscore.
// @lfy def/interpret/main.lfy:lower#lower:lower:3f6b1de42f55afc64a02297dcd69a75780d820b3b323c4b3cb98fe5fd662c66d
pub(super) fn alias_of(source_directory: &str, path: &str) -> String {
    let stem = path.strip_suffix(".lfy").unwrap_or(path);
    let prefix = format!("{}/", source_directory.trim_end_matches('/'));
    let relative = stem.strip_prefix(&prefix).unwrap_or(stem);
    relative
        .chars()
        .map(|c| if unicode_ident::is_xid_continue(c) { c } else { '_' })
        .collect()
}

/// The path a use of a file of a package is written with: the package's name, a slash, and
/// the file's path relative to the package's directory, without its extension.
// @lfy def/interpret/main.lfy:lower#lower:lower:7e159bfaa7ce0d4765fa4fdbd906f7230afdf9fce2ae0297f29ee8f15f2af929
pub(super) fn package_use_path(name: &str, root: &str, path: &str) -> String {
    let stem = path.strip_suffix(".lfy").unwrap_or(path);
    let prefix = format!("{}/", root.trim_end_matches('/'));
    let relative = stem.strip_prefix(&prefix).unwrap_or(stem);
    format!("{name}/{relative}")
}

/// The text of a lowered file with the uses it takes written after the uses written in it,
/// or at its head when it writes none.
// @lfy def/interpret/main.lfy:lower#lower:lower:80853ddbab06f702bb5005757c1b4ec768c77cb9b15b1be8a9713a493900dd40
fn with_uses(text: &str, uses: &[String]) -> String {
    if uses.is_empty() {
        return text.to_string();
    }
    let added = uses.join("\n");
    let mut after = None;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("use ") {
            after = Some(at + line.len());
        }
        at += line.len();
    }
    match after {
        Some(at) => {
            let (head, tail) = text.split_at(at);
            let head = if head.ends_with('\n') {
                head.to_string()
            } else {
                format!("{head}\n")
            };
            format!("{head}{added}\n{tail}")
        }
        None => format!("{added}\n{text}"),
    }
}

/// The lowered tree spelled as Elfie source: kept nodes as written, folded nodes as the
/// literal of their value, and member nodes as a member with its type and value.
// @lfy def/interpret/main.lfy:lower
fn render(model: &Model, node: &LoweredNode, aliases: &HashMap<EntityId, String>) -> String {
    if let Some(value) = &node.value {
        return spell(model, value);
    }
    if let Some(entity) = node.entity
        && node.children.is_empty()
        && matches!(
            model.entities[entity].kind,
            EntityKind::Member | EntityKind::EnumMember
        )
    {
        return spell_member(model, entity, aliases);
    }
    let (members, kept): (Vec<&LoweredChild>, Vec<&LoweredChild>) =
        node.children.iter().partition(|child| match child {
            LoweredChild::Node(child) => is_member_node(model, child),
            LoweredChild::Token(_) => false,
        });
    let mut out = String::new();
    for child in kept {
        match child {
            LoweredChild::Node(child) => out.push_str(&render(model, child, aliases)),
            LoweredChild::Token(token) => out.push_str(&token.raw),
        }
    }
    // A data and a fn take a block or a semicolon; the block of either is compile time, so
    // what is left ends in the members that are kept apart from it, or in a semicolon.
    // @lfy def/interpret/main.lfy:lower
    let bodied =
        node.rule == S::DataDeclaration.entity() || node.rule == S::AgentFunctionDeclaration.entity();
    if members.is_empty() {
        let trimmed = out.trim_end();
        if bodied && !trimmed.ends_with(';') && !trimmed.ends_with('}') {
            return format!("{trimmed};{}", &out[trimmed.len()..]);
        }
        return out;
    }
    let mut body = out.trim_end().to_string();
    body.push_str(" {\n");
    for child in members {
        if let LoweredChild::Node(child) = child {
            body.push_str("  ");
            body.push_str(&render(model, child, aliases));
            body.push('\n');
        }
    }
    body.push('}');
    body
}

/// Whether a lowered node is a member of its parent's entity.
// @lfy def/interpret/main.lfy:lower
pub(super) fn is_member_node(model: &Model, node: &LoweredNode) -> bool {
    node.value.is_none()
        && node.children.is_empty()
        && node.entity.is_some_and(|entity| {
            matches!(
                model.entities[entity].kind,
                EntityKind::Member | EntityKind::EnumMember
            )
        })
}

/// A member as Elfie spells one: its name, its description, and its type or value.
// @lfy def/interpret/main.lfy:lower
fn spell_member(model: &Model, entity: EntityId, aliases: &HashMap<EntityId, String>) -> String {
    let record: &Declared = &model.entities[entity];
    let name = record.identifier.clone().unwrap_or_default();
    let mut out = format!("${name}");
    if let Some(definition) = &record.definition {
        out.push_str(&format!(": `{definition}`"));
    }
    let written = record
        .ty
        .as_ref()
        .map(|ty| member_type_text(model, ty, aliases))
        .unwrap_or_default();
    if written.is_empty() {
        out.push(';');
    } else {
        out.push_str(&format!(" = {written};"));
    }
    out
}

/// The type of a member as Elfie spells one: [`crate::model::type_text_of`] of that type,
/// so what the model decides about brackets, type arguments, a predicate, and a type that
/// declares no name holds here too, with each entity an added `Use` takes an alias for
/// spelled through that alias.
// @lfy def/interpret/main.lfy:lower#lower:lower:1b8bbc7b6e2316e2fb3cfd378dd5fc01093dbefd96e6098d404519e88a6a68ae
fn member_type_text(model: &Model, ty: &TypeRef, aliases: &HashMap<EntityId, String>) -> String {
    let spelled = crate::model::type_text_of(model, Some(ty)).unwrap_or_default();
    // With no alias to write, the model's own spelling is the whole answer.
    // @lfy def/interpret/main.lfy:lower#lower:lower:1b8bbc7b6e2316e2fb3cfd378dd5fc01093dbefd96e6098d404519e88a6a68ae
    if aliases.is_empty() {
        return spelled;
    }
    match ty {
        // A list and a union are spelled from their parts, so an alias inside one reaches
        // the part that needs it; the brackets and the separator stay the model's.
        // @lfy def/interpret/main.lfy:lower#lower:lower:1b8bbc7b6e2316e2fb3cfd378dd5fc01093dbefd96e6098d404519e88a6a68ae
        TypeRef::List(item) => {
            let inner = member_type_text(model, item, aliases);
            if parenthesized_item(model, &spelled, item) {
                format!("({inner})[]")
            } else {
                format!("{inner}[]")
            }
        }
        TypeRef::Union(items) => items
            .iter()
            .map(|item| member_type_text(model, item, aliases))
            .collect::<Vec<_>>()
            .join(" | "),
        // An entity brought in by an aliased use is spelled through that alias, so the name
        // means one entity; its type arguments are spelled the same way.
        // @lfy def/interpret/main.lfy:lower#lower:lower:3f6b1de42f55afc64a02297dcd69a75780d820b3b323c4b3cb98fe5fd662c66d
        TypeRef::Entity(entity) if aliases.contains_key(entity) => {
            let alias = &aliases[entity];
            let name = model.entities[*entity].identifier.clone().unwrap_or_default();
            let arguments = model.type_arguments(*entity);
            if arguments.is_empty() {
                format!("{alias}.{name}")
            } else {
                let written = arguments
                    .iter()
                    .map(|argument| member_type_text(model, argument, aliases))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{alias}.{name}<{written}>")
            }
        }
        TypeRef::Predicate(entity) if aliases.contains_key(entity) => {
            let alias = &aliases[entity];
            let name = model.entities[*entity].identifier.clone().unwrap_or_default();
            format!("is {alias}.{name}")
        }
        // Nothing of this type is aliased, so the model's spelling of it stands.
        _ => spelled,
    }
}

/// Whether the model wrapped the item of a list in parentheses before its brackets, read
/// back from its own spelling of that list rather than decided a second time.
// @lfy def/interpret/main.lfy:lower#lower:lower:1b8bbc7b6e2316e2fb3cfd378dd5fc01093dbefd96e6098d404519e88a6a68ae
fn parenthesized_item(model: &Model, list: &str, item: &TypeRef) -> bool {
    let plain = crate::model::type_text_of(model, Some(item)).unwrap_or_default();
    list == format!("({plain})[]")
}

/// A folded value as Elfie spells it; a prompted value as the call that asked for it.
// @lfy def/interpret/main.lfy:lower
fn spell(model: &Model, value: &Evaluated) -> String {
    match value {
        Evaluated::Value(value) => value.spelled(model),
        Evaluated::Prompted(prompted) => {
            let name = model.entities[prompted.value_type]
                .identifier
                .clone()
                .unwrap_or_else(|| "anonymous".to_string());
            format!("{name}@like(`{}`)", prompted.prompt)
        }
    }
}
