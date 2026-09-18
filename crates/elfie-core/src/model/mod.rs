//! Compiled from `def/model`: the bound program.

pub mod data;

pub use data::*;

/// Build the model of a program from the resolved files of all of it.
// @lfy def/model/main.lfy:10
pub fn bind(sources: Vec<Source>) -> Model {
    let mut model = Model { sources, ..Model::default() };
    model.global = 0;
    model
}
