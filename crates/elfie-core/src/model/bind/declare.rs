// @lfy def/model/main.lfy:bind
//! The declare pass: a scope, a symbol, and an entity for every declaration, and the
//! `use` statements that link one file to another.

use super::*;

impl Binder {

    /// Each SourceFile is a file scope whose current entity is an anonymous entity for
    /// the file. Its parent is settled by [`Binder::link_prelude`], once every file has
    /// been declared.
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/data.lfy:Scope#Scope:scoped:5b33fadf2e17c9e62dac551bbb66306b97e6eeb51c19a3096204c09869eb55f8
    // @lfy def/model/data.lfy:Scope#Scope:scoped:faae0c4ca7e0929fba6a41f28c0858693e9a2edaf78c8d5358a9d35f7e8f2578
    // @lfy def/model/data.lfy:Scope#Scope:scoped:7d4fd7cd3f3cbe120581b5c749bff52067f85aef53181c3045fea9599ac422c2
    pub fn declare_file(&mut self, file: FileId) {
        let trees = self.trees.clone();
        let root = NodeRef { file, index: 0 };
        let entity = self.new_entity(EntityKind::File, Some(root), None);
        let scope = self.new_scope(None, root, entity);
        self.model.entities[entity].scope = Some(scope);
        self.model.file_scopes.push(scope);
        self.model.file_entities.push(entity);
        for child in trees.children(root) {
            self.declare_node(child, scope);
        }
    }

    /// Every file has been declared by now, so the prelude's scope exists whatever order
    /// the files were given in. With no source of [`Origin::Prelude`] no file scope has a
    /// parent, and a name only the prelude would give is found nowhere.
    // @lfy def/model/data.lfy:Scope#Scope:Scope:63702a99fb7a3d155498fd0cfc8839b62e9c0eebe6543cb81ff763006d571f34
    // @lfy def/model/data.lfy:Scope#Scope:Scope:4402361d3767c258bf2e3e234fe5e7f7afa4b16ff000c9be247ce0ce346d91d9
    // @lfy def/model/data.lfy:Scope#Scope:Scope:88f6519881eecd7fc8282f1965f00635e2031400a6510eb901072f8d595e0812
    // @lfy def/model/data.lfy:Scope#Scope:Scope:d0d1d9f729ef5da8873af7dd76fe6f256b415cef6655f59c2d0b44144e696b3f
    // @lfy def/model/main.lfy:bind
    // @lfy def/model/main.lfy:bind
    pub fn link_prelude(&mut self) {
        let trees = self.trees.clone();
        let Some(prelude) = trees
            .sources
            .iter()
            .position(|source| source.origin == Origin::Prelude)
            .map(|file| self.model.file_scopes[file])
        else {
            return;
        };
        self.prelude = Some(prelude); // @lfy def/model/main.lfy:bind
        for (file, source) in trees.sources.iter().enumerate() {
            if source.origin != Origin::Program {
                continue;
            }
            let scope = self.model.file_scopes[file];
            self.model.scopes[scope].parent = Some(prelude);
        }
    }

    /// Every scoped node creates a scope and every declaring node a symbol and an entity.
    // @lfy def/model/main.lfy:bind
    fn declare_node(&mut self, r: NodeRef, enclosing: ScopeId) {
        let trees = self.trees.clone();
        let rule = trees.rule(r);
        let mut scope = enclosing;
        // No holder found in token order leaves `name` None, and the node keeps the
        // entity without a symbol.
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:a9464b35fc899f8287687a1105286e1ef04f75d5b997736659fa6ec7bca03add
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:81d3a20048c5f4886d4a992c83bc0e3711c4074e5f07bdb89722c59a9b192386
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:8d0cf5398ed6d3b3a98cc71a4af12107d93aaa23fc5850b10ea90c054ccf7d50
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:34b33b3c8dab6e5ffb4549e43cafb001f75ee959a06683e75060b9257632065d
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:6b5e4c3087d5aa458cc0f5836829c911a7e2d0645508b8de26e83c208d6f6c1d
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:2197718e55a4ae3f24455c0448356bca9c870eb906da2fd479e312419fe2425a
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:65febce88467f650626d2a0c655fc980d19bc3049ba9770b2d414865af32dacd
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:5370440c016355ad286755235c0af95bdfbfe606949e6af5a6a01626beb04f5e
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:64dda75fb9cb28c56f0e80a3b8abe51b6e6f70fa2368619d2139db89695e4fd5
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:968d54cc304622e2808f357b59ae2ee858c6ae423b288b8e5f2c9c441ca2d4c1
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:c9d005561891014b54dad8a48ba934860047532b6c4ea623558b3dfe42c5ede4
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:34fd3b51ba5410b72c8a1f9bc67745547e454083794bbe3fd7d8d304166c2eb7
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:372fd5d028d950167830ef8999d3b07ef33035c0a4236f902d805afaf1f6e5d5
        // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:69aefc98b6f14acf7fc0ce74755daff8f2ac1d6e45de41a8dc8e5c7bfc454a71
        let declared = declaring(rule).map(|(holder, kind)| {
            let name_token = trees.declared_name_token(r, holder);
            let name = name_token.map(|i| trees.token(r.file, i).value.clone());
            let entity = self.new_entity(entity_kind(kind), Some(r), name.clone());
            (entity, kind, name, name_token)
        });
        // A FunctionType declares no name, but it is a function all the same: it owns an
        // anonymous entity whose parameters are declared in the scope it owns and whose
        // output is the type after its arrow.
        // @lfy def/model/main.lfy:bind
        // @lfy def/model/main.lfy:bind#bind:bind:472fbf0e076310689168fd35debec041a25da18b7e113a79f885c9ff8d7c0a2c
        let function_type = trees.is(r, E::FunctionType).then(|| {
            self.new_entity(
                EntityKind::Fn {
                    parameters: Vec::new(),
                    output: None,
                    agent: false,
                },
                Some(r),
                None,
            )
        });
        // @lfy def/model/data.lfy:Scope#Scope:scoped:ffe3dd6badfac2628979f78954a7e2783ef95830244b18355aa1bbbd30f47fea
        // @lfy def/model/data.lfy:Scope#Scope:scoped:8ce2f09cf066803ebd3227a999e5c15a21ad31f69a14fafe354a89737c771daf
        // @lfy def/model/data.lfy:Scope#Scope:scoped:0b217927a7f23182612f89821245fbeddb681e5234610006a0511197b568d8b0
        // @lfy def/model/data.lfy:Scope#Scope:scoped:2b1324a55d652c373d0949bc41dcfc57f3cffdd623d010cc11c1622d75c6c4dd
        // @lfy def/model/data.lfy:Scope#Scope:scoped:d16a848273a1afab364e7e0a65631450fb2e2a82330fe401838aed5606bed594
        // @lfy def/model/data.lfy:Scope#Scope:scoped:95b3eae967fd82eb2345a96e7cbbf4f724c6391c9ba87ded003e2782607da213
        // @lfy def/model/data.lfy:Scope#Scope:scoped:ff1601aaee04cb142660012c49e78cfcb6991a97449a565e22273b90b31f82ba
        // @lfy def/model/data.lfy:Scope#Scope:scoped:5f240eac9d92d86b8fe71959d8bf3ffa6dd12ed67e36638b801959262a3106bb
        // @lfy def/model/data.lfy:Scope#Scope:scoped:1d1c41f799d7cac1b8060ffd595536fca1ea3ad1a96ca719ba39f7c6b6d2761b
        // @lfy def/model/data.lfy:Scope#Scope:scoped:d372ed9640bfc269d009edad3978585ab8805a06b8877805fa2367fc6516f4ff
        // @lfy def/model/data.lfy:Scope#Scope:scoped:f9a2a494059fd7d7402cf602c76c5787fe1fcd1cbe5b4def26480fba2dccfe0d
        // @lfy def/model/data.lfy:Scope#Scope:scoped:2cb39b79ab2c022b7e444c50518995299c304ccebd186e9a8a5baa3d5f883822
        // @lfy def/model/data.lfy:Scope#Scope:scoped:a178cf5710cf5b8a20a035e2d15470284e1ab5fad764b7d9e881c7f7ec8f6cee
        // @lfy def/model/data.lfy:Scope#Scope:scoped:2afa6299b7f95a8efdf909dd52e7e852a9c0772f3c057a10e373b281aa144b52
        // @lfy def/model/data.lfy:Scope#Scope:scoped:6e73eabc8de2ab79f61f97213346be740e6b13a38a3691ee7c49709cd0d8d8d7
        // @lfy def/model/data.lfy:Scope#Scope:scoped:976c096ae30b38a2e03b990d516f2a39aaaaa09f67e1f9ec2cb3aa6434e0bbe0
        // @lfy def/model/data.lfy:Scope#Scope:scoped:eb3741706777fce6df9ebdd82d93beacb7e2e8e9094fc6d7c9fa2b410040d2b9
        // @lfy def/model/data.lfy:Scope#Scope:scoped:5dafe391d3fdf071e448f7c62fa5582ffaded16af1209fd355b78982f0a85847
        // @lfy def/model/data.lfy:Scope#Scope:scoped:08b9924e3864b23b67adb90edd7346f98bd9663a027a254da03335119edfd96f
        // @lfy def/model/data.lfy:Scope#Scope:scoped:cdabf2b5ccb27a01f4584e15c0053db351e761ceb1911acfb5a8f82ac7d05dc6
        // @lfy def/model/data.lfy:Scope#Scope:scoped:e22f517ab983739aac006065af87c8dc7710c4ded1b2202cffb1d08fbe796ac1
        // @lfy def/model/data.lfy:Scope#Scope:scoped:8a2fffacfbdab4b0f2c14ff86b1f98b59f7ecffe87c81007e3bab98a5dc74cb1
        // @lfy def/model/data.lfy:Scope#Scope:scoped:aab2abc11ae1ed5e82f59a4abcb37d3536fc1cbaebd1785b76924465af73804e
        // @lfy def/model/data.lfy:Scope#Scope:scoped:c717a2ec6d7bacf53673eb7be9a9ba25c8541e16556c5749156f645bde6a63f4
        // @lfy def/model/data.lfy:Scope#Scope:scoped:ee260a5bacdb04d0edcc7d54b7b2906eb89264dfc13c7ac5da7b07805b8562f6
        // @lfy def/model/data.lfy:Scope#Scope:scoped:a027726b19fd45ead1de834c3add1258885016aa19fdd5290d891394e07491b2
        // @lfy def/model/data.lfy:Scope#Scope:scoped:f6065fee62aada98517eb841d9692cea6edc82cc242f145364310c30e8eabb23
        // @lfy def/model/data.lfy:Scope#Scope:scoped:79f83f4d5819d07e411408b0d21db3a90ebd72c07afc53ad687af99e57bf724c
        // @lfy def/model/data.lfy:Scope#Scope:scoped:9b0f0ef2336b66991660ac21d522cea00b3a6e2c2b08bbc7f7c610a1865ff3d9
        // @lfy def/model/data.lfy:Scope#Scope:scoped:b71e445833abd065f5ece0797fb9ef964face7c30092e802148954c6450d41ae
        // @lfy def/model/data.lfy:Scope#Scope:scoped:6da3b9d6fa29747923b9953027c88aaa17f8911dc1bb6a9999fdb2d1918da7f4
        // @lfy def/model/data.lfy:Scope#Scope:scoped:98fb64983a7ca9affcd5927e9ff8bc124ae4b46f56016c5b2094e4e86a3eb7c2
        // @lfy def/model/data.lfy:Scope#Scope:scoped:9dc440245b5ab24009f26c5a2fcfecc41b456df6dce7634979b79f348bca24a3
        if is_scoped(rule) {
            // A For declares its loop variable, not itself: its current stays the parent's.
            let current = match (&declared, function_type) {
                (Some((entity, kind, _, _)), _) if *kind != SymbolKind::LoopVariable => *entity,
                (_, Some(entity)) => entity,
                _ => self.model.scopes[enclosing].current,
            };
            scope = self.new_scope(Some(enclosing), r, current);
            if let Some((entity, _, _, _)) = declared {
                self.model.entities[entity].scope = Some(scope);
            }
            if let Some(entity) = function_type {
                self.model.entities[entity].scope = Some(scope);
            }
        }
        if let Some((entity, kind, Some(name), name_token)) = declared {
            // A loop variable goes to the scope the For owns, so it is visible in the loop
            // body and nowhere else; the ForFrom inside it shares that same scope.
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:d1cb6dfeb78930d8176cc84610315739a1272e74a1ed02021e938d314efb28da
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:49fb925a55aff2d9207e35f0e321db962e913fa6bd393239d36905ea88280265
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:27c0428e3328fa9f3489e054d42a4c65ae7213e24f78355b57f57668fa6aa355
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:14e6f73840a36a30067d08e606b763de1425d6813b0e78a09b5f25a2c72ee742
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:3098afe39178a40cc88585e9fdf25a34479b999ec9848c235b29d32f105dde3d
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:c25d856290785e8bd3f4f502e63a2308e87afbe7b6433c8ae2e9b177aee5d354
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:f1f19923f1ddbd7ff5a3ee819645e9a99dbe4c7d2182a3eefd5943cf23050ddf
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:2ecf86a6fd171e9f8f361668fb0b36d56845b50188103f05e716ed9172a44273
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:f2a97ecff966e445b6e63be184325dbceb3d24507a5c042a87d8145b8ab27aa7
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:a0d2fbb2bd7f93d2d60c133f9028246334da1145041a63c75adcae5ce4011641
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:86131456bb359e55efb75cba730c906e75106b6177cbf87dcf3b79fd6bfdd64a
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:7759186210c35aea620a5b45ac5037b8b5d3174dd83982c7209cc062fbb5d23f
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:425817c8bd39a13ef2b2fe3609e32f4f476959a397381fc89fe7b04edcbb505b
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:81f8d8c649b56891b2ade873b30c6771bcdc057880906178ac2943e0277b99e5
            // @lfy def/model/data.lfy:Symbol#Symbol:loopVariable:9edfde86165cf2965669d7e06cea959b9f613f8d7c2e05747a7105010ef532ce
            // @lfy def/model/data.lfy:Symbol#Symbol:loopVariable:e47e3a505fed69039e44dae3c596b68801e202361b716594bb14376aeadd4ac1
            let target = if kind == SymbolKind::LoopVariable {
                if trees.is(r, S::ForFrom) {
                    enclosing
                } else {
                    scope
                }
            } else {
                enclosing
            };
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:dc48273c1920479724f94365767843e2e6412fa51d843be5e3dfd1acb0eceba3
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:53c55484519d974927b1961b29d6380c8be14ee06d8cb46254bc9ba444aa68b9
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:d1d7624dba044dde33d52c4f6838ac858093f763c52a5310f54baea1005c89a0
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:f0a0fba60c0678f24154b65fb5d3206a250f93c05c8be45f3620cbb89cdca789
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:ece2ac6b92fed8fc16e5dde4b47a7be28de5f806d1c0327507c598380313acb1
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:322f18b896575f973daff231201e0cebf6392f5f630dd2daf8b0534a150af23e
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:9d1186bd335039be7703606aa8bb1f610a416f90abb5d204f3ddfbd74c279e7e
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:96b1b5e5b9cfad013726db7f79f80b6dd69014bc79e20d132f6512e740218297
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:c0fa9c9eba5e9b2ddfc357fcb7d89e1e20989365dbe3804c961e36cb33d52899
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:f7bec303ca6bb8fb0439e2084bb8b4aedca70495c60bb1b39ac3e42e723236f6
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:b3213cf402da421d9f6512b3f867af2b9bdb7304f1478668c1823f68f085afff
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:0fc7c70bd4557dbcb74156d37d7b9c06e7ac1dd017b27b6e311e22b3f74c6d8e
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:13b2f037ea40a87000d76f67908535cb38fb45c8af5ddfecd1e6bc76e652b5c3
            // @lfy def/model/data.lfy:Symbol#Symbol:namedBy:618a77f6335d5eb69661b21713f6bcd40d0fc7e3b31921f0221b899ada5ac09d
            // @lfy def/model/data.lfy:Symbol#Symbol:loopVariable:095aaf1b84dc9453ea9253ffb2d6835e856760a2fa2d3b159d44b083e1857d95
            // @lfy def/model/data.lfy:Symbol#Symbol:loopVariable:14bcecc5f3fa3263ef1b4def4122e706136f7925c0b3c03591e1c5c6911133bc
            // @lfy def/model/data.lfy:Symbol#Symbol:loopVariable:802772f277410c4449932d083179d0e379cf952f973d0f5dfd712d05ed8bde90
            // @lfy def/model/data.lfy:Symbol#Symbol:loopVariable:e66572d2367aeff4ed6a30e9e611bfd4ffeb1bbe7e04bc3aed1ffe9e9e2a211b
            self.add_symbol(target, name, entity, kind, r, name_token);
        }
        // A member declared in a data or trait body.
        // @lfy def/model/main.lfy:bind
        if let Some(current) = trees.member_declaration(r) {
            let (_, name_token) = trees.accessor_and_name(current).expect("checked");
            let name_token = name_token.expect("checked");
            let name = trees.token(r.file, name_token).value.clone();
            let owner_scope = self.model.scopes[enclosing].parent.unwrap_or(enclosing);
            let entity = self.new_entity(EntityKind::Member, Some(r), Some(name.clone()));
            if let Some(symbol) = self.add_symbol(
                owner_scope,
                name,
                entity,
                SymbolKind::Member,
                r,
                Some(name_token),
            ) {
                self.model.symbol_by_node.insert(current, symbol);
            }
        }
        // An ObjectKey in the Object of an EnumDeclaration declares an enum member.
        // @lfy def/model/main.lfy:bind
        if trees.is(r, E::ObjectKey)
            && trees.parent(r).is_some_and(|p| trees.is(p, E::Object))
            && trees
                .parent(r)
                .and_then(|p| trees.parent(p))
                .is_some_and(|g| trees.is(g, S::EnumDeclaration))
            && let Some(declared) = trees.child(r, E::Declared)
            && let Some(token) = trees.child_token(
                declared,
                crate::grammar::terminals::identifier::Identifier::Identifier,
            )
        {
            let name = trees.token(r.file, token).value.clone();
            let entity = self.new_entity(EntityKind::EnumMember, Some(r), Some(name.clone()));
            self.add_symbol(
                enclosing,
                name,
                entity,
                SymbolKind::EnumMember,
                r,
                Some(token),
            );
        }
        for child in trees.children(r) {
            self.declare_node(child, scope);
        }
    }

    /// A Use refers to the Source its entry in Source.uses names.
    // @lfy def/model/main.lfy:bind
    pub fn link_uses(&mut self, file: FileId) {
        let trees = self.trees.clone();
        let _ = file;
        // Every Use of the file in preorder, as the workspace lists Source.uses.
        let uses: Vec<NodeRef> = (0..trees.nodes[file].len())
            .map(|index| NodeRef { file, index })
            .filter(|&r| trees.is(r, S::Use))
            .collect();
        for (i, use_node) in uses.into_iter().enumerate() {
            let Some(Some(path)) = trees.sources[file].uses.get(i) else {
                continue;
            };
            let Some(target) = trees.sources.iter().position(|s| &s.path == path) else {
                self.problem(use_node, format!("{path} is not in the program"));
                continue;
            };
            if trees.has_token(use_node, K::AsKeyword) {
                // The module symbol's entity is the used file's entity.
                // @lfy def/model/main.lfy:bind
                if let Some(symbol) = self.model.symbol_of(use_node) {
                    let file_entity = self.model.file_entities[target];
                    self.model.symbols[symbol].entity = file_entity;
                }
            } else {
                // The used file's own symbols, not its imports, are visible.
                // @lfy def/model/main.lfy:bind
                let own: Vec<SymbolId> = self.model.scopes[self.model.file_scopes[target]]
                    .declared()
                    .collect();
                let scope = self.model.file_scopes[file];
                for symbol in own {
                    if !self.model.scopes[scope].symbols.contains(&symbol) {
                        self.model.scopes[scope].symbols.push(symbol);
                        self.model.scopes[scope].imports.push(symbol);
                    }
                }
            }
        }
    }
}
