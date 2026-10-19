// @lfy def/mcp/read.lfy:problems
//! The tools that read the program: its problems, where a name is declared, everything
//! known about it, where it is used, the declarations of one file, and the grammar.

use super::*;

/// The kind of an entity, as its symbol names it.
fn kind_of(model: &Model, entity: EntityId) -> String {
    model.entities[entity]
        .symbol
        .map_or_else(|| "entity".to_string(), |symbol| model.symbols[symbol].kind.as_str().to_string())
}

/// The range of the token that spells an entity's name, or of the first token of its
/// declaration; an empty range when it has neither.
fn identifier_range(model: &Model, entity: EntityId) -> Range {
    let record = &model.entities[entity];
    let Some(node) = record.node else { return Range::default() };
    let tokens = model.tokens(node.file);
    let named = record
        .symbol
        .and_then(|symbol| model.symbols[symbol].name_token)
        .and_then(|index| tokens.get(index));
    let token = named.or_else(|| model.first_token(node));
    match token {
        Some(token) => Range {
            file: token.file.to_string(),
            start: Position::new(token.line, token.column),
            end: Position::new(token.line, token.column + token.raw.chars().count()),
        },
        None => Range::default(),
    }
}

/// The file and line an entity is declared at, as `file:line`.
fn place_of(model: &Model, entity: EntityId) -> String {
    let range = identifier_range(model, entity);
    if range.file.is_empty() {
        "unplaced".to_string()
    } else {
        format!("{}:{}", range.file, range.start.line)
    }
}

/// The text of one line of a file, from its tokens.
fn line_text(model: &Model, file: FileId, line: usize) -> String {
    let text: String = model.tokens(file).iter().map(|token| token.raw.as_str()).collect();
    text.lines().nth(line.saturating_sub(1)).unwrap_or("").trim().to_string()
}

/// The text of a criterion: its situation when it has one, then its behavior and side
/// effects.
fn criterion_text(criterion: &Criterion) -> String {
    let mut text = String::new();
    if let Some(situation) = &criterion.situation {
        text.push_str(&format!("when {}: ", situation.join(" ")));
    }
    if let Some(behavior) = &criterion.behavior {
        text.push_str(&behavior.join("\n  "));
    }
    if let Some(side_effects) = &criterion.side_effects {
        if !text.is_empty() {
            text.push_str("; ");
        }
        text.push_str(&format!("side effects: {}", side_effects.join("\n  ")));
    }
    text
}

/// An error saying a file is not in the program and listing the files that are.
fn not_in_program(workspace: &Workspace, file: &str) -> String {
    let mut text = format!("{file} is not in the program; the files are:");
    for known in &workspace.files {
        text.push_str(&format!("\n  {}", known.path));
    }
    text
}

/// The text when nothing is declared with a name.
fn nothing_named(name: &str) -> String {
    format!("nothing is declared as {name}; elfie_outline lists the declarations of one file")
}

// ---- the tools --------------------------------------------------------------------

/// The problems of the program, or of one file: what the compiler, an editor, or a check
/// would report. One line per diagnostic reading the file, the line, the column, the
/// stage, the severity, and the message; the text says so when there are none.
// @lfy def/mcp/read.lfy:problems#problems:problems:c8c8b536f1269d0fb4bc9adb64fbce292df0715fa51d5d524a18f16685af7bf6
pub fn problems(session: &Session, file: Option<&str>) -> Result<String, String> {
    let workspace = &session.workspace;
    let diagnostics = query::diagnostics_of(workspace, file); // @lfy def/mcp/read.lfy:problems#problems:problems:c8c8b536f1269d0fb4bc9adb64fbce292df0715fa51d5d524a18f16685af7bf6
    if diagnostics.is_empty() {
        // Decision: the criterion answers this situation with text, not a failure, and it
        // names no exception for a file outside the program, so a name that matches
        // nothing is answered the same way; elfie_outline is the tool that lists the files.
        if let Some(file) = file {
            return Ok(format!("no problems in {file}")); // @lfy def/mcp/read.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
        }
        return Ok("no problems".to_string()); // @lfy def/mcp/read.lfy:problems#problems:problems:4e726dce93d9d233aedec39235ecea647b64e1f851fe518f6992aea5add1da3c
    }
    let lines: Vec<String> = diagnostics
        .iter()
        .map(|diagnostic| {
            diagnostic_line(
                &diagnostic.range.file,
                diagnostic.range.start.line,
                diagnostic.range.start.column,
                &diagnostic.stage.to_string(),
                &diagnostic.severity.to_string(),
                &diagnostic.message,
            )
        })
        .collect();
    Ok(lines.join("\n"))
}

/// Where things with a name are declared: one line per entity reading the kind, the
/// identifier, the file, the line, and the definition when there is one.
// @lfy def/mcp/read.lfy:find#find:find:8d5e1239e348d7ebcaa7e007e6bfc71fed3f9aa4abd903c5cc7f3160b0416367
pub fn find(session: &Session, name: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let model = &workspace.model;
    let entities = query::find(workspace, name);
    if entities.is_empty() {
        // The text says so, and suggests elfie_outline.
        // @lfy def/mcp/read.lfy:find#find:find:33784dd4ac5c7832aad69dab34430cacc1eb4b82930eb20f500fcfe9c89be26c
        // @lfy def/mcp/read.lfy:find#find:find:1a0733810793cda3b844ef9c1beb121fa974d605748755292d61aa511c80a1e2
        return Ok(nothing_named(name));
    }
    // @lfy def/mcp/read.lfy:find#find:find:8d5e1239e348d7ebcaa7e007e6bfc71fed3f9aa4abd903c5cc7f3160b0416367
    let lines: Vec<String> = entities
        .iter()
        .map(|&entity| {
            let mut line = format!("{} {} {}", kind_of(model, entity), identifier_of(model, entity), place_of(model, entity));
            // The line ends with the definition, after the kind, the identifier, the file,
            // and the line.
            // @lfy def/mcp/read.lfy:find#find:find:eb271c552e5c82df2d2018db45719c602a20801b3e7e3e1f948bf7c9f0108b1d
            if let Some(definition) = &model.entities[entity].definition {
                line.push_str(&format!(": {}", model::strip_references(definition)));
            }
            line
        })
        .collect();
    Ok(lines.join("\n"))
}

/// Everything known about the things with a name: for each entity, the hover fields with a
/// heading each, its traits, members, tests, references, and source.
// @lfy def/mcp/read.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
pub fn entity(session: &Session, name: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let model = &workspace.model;
    let entities = query::find(workspace, name);
    if entities.is_empty() {
        return Ok(nothing_named(name));
    }
    let mut sections = Vec::new();
    for &id in &entities {
        let record = &model.entities[id];
        let hover = query::hover_of(workspace, id);
        let mut out = String::new();
        // Each entity is headed by its file, so the caller can narrow the name with a
        // file path.
        // @lfy def/mcp/read.lfy:entity#entity:entity:0c69bca0860a743d038fb5c4f6c609c4ded6631ac9cf95f37cccba908854cbcd
        out.push_str(&format!("# {} {} ({})\n", hover.kind, hover.identifier, place_of(model, id)));
        // The fields of the hover, a heading each.
        // @lfy def/mcp/read.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
        if let Some(definition) = &hover.definition {
            out.push_str(&format!("\n## Definition\n{definition}\n"));
        }
        if let Some(ty) = &hover.ty {
            out.push_str(&format!("\n## Type\n{ty}\n"));
        }
        if let Some(owner) = &hover.owner {
            out.push_str(&format!("\n## Owner\n{owner}\n"));
        }
        if let Some(documentation) = &hover.documentation {
            out.push_str(&format!("\n## Documentation\n{documentation}\n"));
        }
        if !hover.criteria.is_empty() {
            out.push_str("\n## Criteria\n");
            for criterion in &hover.criteria {
                let from = if criterion.contributor == id {
                    String::new()
                } else {
                    format!(" (from {})", identifier_of(model, criterion.contributor))
                };
                out.push_str(&format!("- {}{from}\n", criterion_text(criterion)));
            }
        }
        // The identifier of each trait, as the hover spells them: every one of
        // `Entity.traits` in application order, with `builtin` among them for a native
        // thing.
        // @lfy def/mcp/read.lfy:entity#entity:entity:d43b0e556e705056317e1af4e3df190417f20a08435faad56d54fcafa63e524e
        if !hover.traits.is_empty() {
            out.push_str(&format!("\n## Traits\n{}\n", hover.traits.join(", ")));
        }
        let members = model.members(id);
        if !members.is_empty() {
            out.push_str("\n## Members\n");
            for symbol in members {
                let symbol = &model.symbols[symbol];
                let member = &model.entities[symbol.entity];
                let mut line = format!("- {}", symbol.name);
                if let Some(definition) = &member.definition {
                    line.push_str(&format!(": {}", model::strip_references(definition)));
                }
                if let Some(ty) = &member.ty {
                    line.push_str(&format!(" ({})", model::type_text(model, ty)));
                }
                out.push_str(&line);
                out.push('\n');
            }
        }
        if !record.tests.is_empty() {
            out.push_str("\n## Tests\n");
            for test in &record.tests {
                out.push_str(&format!("- input: {}\n  expect: {}\n", test.input_text.trim(), test.expect_text.trim()));
            }
        }
        if let Some(symbol) = record.symbol {
            let usages = model::usages_of(model, symbol);
            out.push_str(&format!("\n## References ({})\n", usages.len()));
            for usage in usages {
                let (file, line, _) = usage_place(model, usage);
                out.push_str(&format!("- {file}:{line}\n"));
            }
        }
        out.push_str(&format!("\n## Source\n```elfie\n{}\n```", query::source_of(workspace, id).trim_end()));
        sections.push(out);
    }
    // More than one entity: each is separated by a rule.
    // @lfy def/mcp/read.lfy:entity#entity:entity:0c69bca0860a743d038fb5c4f6c609c4ded6631ac9cf95f37cccba908854cbcd
    Ok(sections.join("\n\n---\n\n"))
}

/// The file, line, and column of a usage: of the token that spells the name, or of the
/// first token of the using node.
fn usage_place(model: &Model, usage: usize) -> (String, usize, usize) {
    let usage = &model.usages[usage];
    let tokens = model.tokens(usage.node.file);
    let token = usage
        .token
        .and_then(|index| tokens.get(index))
        .or_else(|| model.first_token(usage.node));
    match token {
        Some(token) => (token.file.to_string(), token.line, token.column),
        None => (model.sources[usage.node.file].path.clone(), 0, 0),
    }
}

/// Every place things with a name are used: for each entity, one line per usage of its
/// symbol reading the file, the line, the column, and the text of that line.
// @lfy def/mcp/read.lfy:references#references:references:172a06e97d4c30625ada08fa53113f0fdd6c084862a2db5f64451f15c9853456
pub fn references(session: &Session, name: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    let model = &workspace.model;
    let entities = query::find(workspace, name);
    if entities.is_empty() {
        // Decision: the definition says nothing about a name that matches no entity;
        // the answer of elfie_find is given so the caller learns the same thing.
        return Ok(nothing_named(name));
    }
    let mut lines = Vec::new();
    for &id in &entities {
        let Some(symbol) = model.entities[id].symbol else { continue };
        for usage in model::usages_of(model, symbol) {
            let (file, line, column) = usage_place(model, usage); // @lfy def/mcp/read.lfy:references#references:references:172a06e97d4c30625ada08fa53113f0fdd6c084862a2db5f64451f15c9853456
            let text = line_text(model, model.usages[usage].node.file, line);
            lines.push(format!("{file}:{line}:{column}: {text}"));
        }
    }
    if lines.is_empty() {
        // Decision: no usages is a plain answer, not an error.
        return Ok(format!("{name} is never used"));
    }
    Ok(lines.join("\n"))
}

/// One outline and its children as an indented list: two spaces per level, then the
/// kind, the name, and the line.
// @lfy def/mcp/read.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
fn render_outline(items: &[Outline], depth: usize, out: &mut Vec<String>) {
    for item in items {
        // Decision: the line is the identifier's, so a documented declaration is listed
        // where its name is, not where its documentation begins.
        out.push(format!("{}{} {} {}", "  ".repeat(depth), item.kind, item.name, item.selection_range.start.line));
        render_outline(&item.children, depth + 1, out);
    }
}

/// The declarations of one file, nested as written.
// @lfy def/mcp/read.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
pub fn outline(session: &Session, file: &str) -> Result<String, String> {
    let workspace = &session.workspace;
    if workspace.file(file).is_none() {
        return Err(not_in_program(workspace, file)); // @lfy def/mcp/read.lfy:outline#outline:outline:a024fe62db1e31cd5f20fef57d6d0cc38073b0724bb0ecf3e828e91bf7d495d3
    }
    let items = query::outline_of(workspace, file);
    if items.is_empty() {
        return Ok(format!("no declarations in {file}"));
    }
    let mut lines = Vec::new();
    render_outline(&items, 0, &mut lines); // @lfy def/mcp/read.lfy:outline#outline:outline:f4a001d0d002913edf84c20318c9fdac89e274849e6c553e36e790f938bb6027
    Ok(lines.join("\n"))
}

/// The grammar of Elfie: every terminal, a blank line, then every rule as EBNF.
// @lfy def/mcp/read.lfy:grammar#grammar:grammar:51bcd7292055364b6387726881c5443879b09b36eae8e1534b8d84a84effc72f
pub fn grammar(_session: &Session) -> Result<String, String> {
    Ok(format!(
        "{}\n\n{}",
        elfie_core::grammar::terminal_document(),
        elfie_core::grammar::grammar_document()
    )) // @lfy def/mcp/read.lfy:grammar#grammar:grammar:51bcd7292055364b6387726881c5443879b09b36eae8e1534b8d84a84effc72f
}
