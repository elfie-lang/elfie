// @lfy def/generation/main.lfy:interfaceOf
//! What dependents of a unit may rely on.

use super::*;

/// What dependents of a unit may rely on, as one text whose hash tells whether it changed.
///
/// One line per entity of the unit in file order — its identifier, its kind, its
/// definition, its type, and for a fn its parameters with their types and its output —
/// and, after the line of a data, type, trait, or enum, one line per member or enum
/// member it declares, since a dependent relies on those as much as on the declaration
/// itself. A type line spells an entity of the `elfie` package by its identifier alone,
/// such as `List` or `Path`, never by a path or a module name. Nothing else is written: a
/// change that leaves every line the same, such as a new criterion or a moved line, leaves
/// the text the same, so dependents are not regenerated for it.
// @lfy def/generation/main.lfy:interfaceOf#interfaceOf:interfaceOf:df415af10d739698174e3358d1293c5c82973190a3202adf8f50300ab0f4f3d5
pub fn interface_of(workspace: &Workspace, unit: &Unit) -> String {
    let model = &workspace.model;
    let mut out = String::new();
    for &entity in &unit.entities {
        // @lfy def/generation/main.lfy:interfaceOf
        write_interface_entity(&mut out, model, entity, "");
        // @lfy def/generation/main.lfy:interfaceOf
        for member in members_of(model, entity) {
            write_interface_entity(&mut out, model, member, "  ");
        }
    }
    out
}
