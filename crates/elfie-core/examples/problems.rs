//! Loads a project and prints every problem: a development aid until the CLI exists.
use std::path::Path;

fn main() {
    let root = std::env::args().nth(1).unwrap_or_else(|| ".".to_string());
    let workspace = elfie_core::workspace::load(Path::new(&root));
    let model = &workspace.model;
    println!("files: {}  symbols: {}  entities: {}  usages: {}", workspace.files.len(), model.symbols.len(), model.entities.len(), model.usages.len());
    for problem in &workspace.problems {
        match problem {
            elfie_core::workspace::WorkspaceProblem::Load(p) => println!("load {:?}: {}", p.path, p.message),
            elfie_core::workspace::WorkspaceProblem::Bind(p) => {
                let token = model.first_token(p.node);
                let (file, line, col) = token.map_or(("?".to_string(), 0, 0), |t| (t.file.to_string(), t.line, t.column));
                println!("{file}:{line}:{col}: {} [{}]", p.message, model.raw(p.node).trim().chars().take(60).collect::<String>());
            }
        }
    }
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--entity") {
        let name = &args[pos + 1];
        for (id, entity) in model.entities.iter().enumerate() {
            if entity.identifier.as_deref() != Some(name.as_str()) { continue; }
            println!("#{id} {name} kind={:?}", entity.kind);
            println!("  definition: {:?}", entity.definition);
            println!("  type: {:?}", entity.ty.as_ref().map(|t| elfie_core::model::type_text(model, t)));
            for a in &entity.traits { println!("  trait {} values=[{}] source={:?}", model.entities[a.entity].identifier.clone().unwrap_or_default(), a.values.iter().map(|v| elfie_core::model::value_text(model, v)).collect::<Vec<_>>().join(" ; "), a.source); }
            for (k, v) in &entity.values { println!("  value {k} = {}", elfie_core::model::value_text(model, v)); }
            for s in model.members(id) { println!("  member {} ({})", model.symbols[s].name, model.symbols[s].kind); }
            for c in &entity.acceptance_criteria { println!("  criterion (from {}): situation={:?} behavior={:?} side={:?}", model.entities[c.contributor].identifier.clone().unwrap_or_default(), c.situation, c.behavior, c.side_effects); }
            for t in &entity.tests { println!("  test input={} expect={}", t.input_text.chars().take(80).collect::<String>(), t.expect_text.chars().take(80).collect::<String>()); }
        }
        return;
    }
    if std::env::args().any(|a| a == "--dump") {
        for (id, entity) in model.entities.iter().enumerate() {
            if let Some(identifier) = &entity.identifier {
                let traits: Vec<String> = entity.traits.iter().map(|a| model.entities[a.entity].identifier.clone().unwrap_or_default()).collect();
                println!("#{id} {identifier} kind={:?} traits=[{}] criteria={} tests={} def={:?}", std::mem::discriminant(&entity.kind), traits.join(","), entity.acceptance_criteria.len(), entity.tests.len(), entity.definition.as_deref().map(|d| d.chars().take(50).collect::<String>()));
            }
        }
    }
}

#[allow(dead_code)]
fn unused() {}
