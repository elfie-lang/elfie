//! Compiled from `def/mcp/traits.lfy`: the `tool` trait.
//!
//! A trait emits nothing of its own; what it adds belongs to each fn that carries it. Every
//! fn of `def/mcp/main.lfy` that carries `tool` is an ordinary Rust fn there, and this
//! module holds what the trait's criteria describe for all of them at once: how a
//! [`Registered`] fn is listed as a [`Tool`], how a call's arguments are checked against its
//! parameters, and how the fn is reached and its failure reported.

use std::panic::{self, AssertUnwindSafe};

use rmcp::model::JsonObject;
use serde_json::{Value, json};

use crate::data::{Session, Tool, ToolArgument, ToolResult};

/// The arguments of one call, checked against the tool's parameters before the fn sees
/// them.
#[derive(Debug, Clone, Copy)]
pub struct Arguments<'a> {
    values: &'a JsonObject,
}

impl<'a> Arguments<'a> {
    pub fn new(values: &'a JsonObject) -> Arguments<'a> {
        Arguments { values }
    }

    /// A string argument that a call must give.
    pub fn string(&self, name: &str) -> Result<&'a str, String> {
        self.optional(name)
            .ok_or_else(|| format!("the argument {name} is missing"))
    }

    /// A string argument a call may leave out; `null` counts as left out.
    pub fn optional(&self, name: &str) -> Option<&'a str> {
        self.values.get(name).and_then(Value::as_str)
    }
}

/// The signature every tool fn is reached through: the session and the checked arguments.
pub type Call = fn(&Session, Arguments<'_>) -> Result<String, String>;

/// One fn carrying `tool`: how the server lists it and how a call reaches it.
// @lfy def/mcp/traits.lfy:8
#[derive(Debug, Clone)]
pub struct Registered {
    /// The tool as the server lists it.
    pub tool: Tool,
    /// The fn, called with the session and the call's arguments.
    pub call: Call,
}

impl Registered {
    /// The fn `identifier`, described by `definition`, with one argument per parameter that
    /// is not the session; it is listed as `elfie_` followed by the identifier.
    // @lfy def/mcp/traits.lfy:9
    pub fn new(identifier: &str, definition: &str, arguments: Vec<ToolArgument>, call: Call) -> Registered {
        Registered {
            tool: Tool {
                name: format!("elfie_{identifier}"),
                description: definition.to_string(),
                arguments,
            },
            call,
        }
    }
}

/// The JSON schema of a tool's arguments: an object with one property per argument, typed
/// as the parameter is and described by the fn's documentation line for it, and the
/// arguments a call must give as `required`.
// @lfy def/mcp/traits.lfy:9
pub fn input_schema(tool: &Tool) -> JsonObject {
    let mut properties = serde_json::Map::new();
    let mut required = Vec::new();
    for argument in &tool.arguments {
        properties.insert(
            argument.name.clone(),
            json!({ "type": argument.ty, "description": argument.description }), // @lfy def/mcp/traits.lfy:10
        );
        if argument.required {
            required.push(Value::String(argument.name.clone()));
        }
    }
    let mut schema = serde_json::Map::new();
    schema.insert("type".to_string(), Value::String("object".to_string()));
    schema.insert("properties".to_string(), Value::Object(properties));
    schema.insert("required".to_string(), Value::Array(required));
    schema
}

/// Every registered fn as the protocol lists a tool.
// @lfy def/mcp/traits.lfy:9
pub fn list(tools: &[Registered]) -> Vec<rmcp::model::Tool> {
    tools
        .iter()
        .map(|registered| {
            rmcp::model::Tool::new(
                registered.tool.name.clone(),
                registered.tool.description.clone(),
                input_schema(&registered.tool),
            )
        })
        .collect()
}

/// The registered fn a call names, when one does.
// @lfy def/mcp/traits.lfy:11
pub fn find<'t>(tools: &'t [Registered], name: &str) -> Option<&'t Registered> {
    tools.iter().find(|registered| registered.tool.name == name)
}

/// Whether a JSON value is of the type an argument declares.
fn is_of_type(value: &Value, ty: &str) -> bool {
    match ty {
        "string" => value.is_string(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        _ => false,
    }
}

/// The arguments checked against the parameters: an argument that is missing, unknown, or
/// of the wrong type is an error naming the argument. A `null` argument counts as left out.
// @lfy def/mcp/traits.lfy:12
pub fn check_arguments(tool: &Tool, arguments: &JsonObject) -> Result<(), String> {
    for name in arguments.keys() {
        if !tool.arguments.iter().any(|argument| &argument.name == name) {
            return Err(format!("{} takes no argument {name}", tool.name));
        }
    }
    for argument in &tool.arguments {
        match arguments.get(&argument.name) {
            None | Some(Value::Null) => {
                if argument.required {
                    return Err(format!("{} needs the argument {}", tool.name, argument.name));
                }
            }
            Some(value) if !is_of_type(value, &argument.ty) => {
                return Err(format!("the argument {} of {} must be a {}", argument.name, tool.name, argument.ty));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

/// One call of a registered fn: the arguments are checked, the fn is called with the
/// session and them, and what it returns is the result. A failure of the fn is an error
/// carrying the failure, and the server goes on.
// @lfy def/mcp/traits.lfy:11
pub fn call(registered: &Registered, session: &Session, arguments: &JsonObject) -> ToolResult {
    // @lfy def/mcp/traits.lfy:12
    if let Err(message) = check_arguments(&registered.tool, arguments) {
        return ToolResult::error(message);
    }
    // Decision: "the fn fails" covers a panic as well as an `Err`, so a panic inside a tool
    // is caught and reported as the call's error rather than ending the connection.
    // @lfy def/mcp/traits.lfy:13
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| (registered.call)(session, Arguments::new(arguments))));
    match outcome {
        Ok(result) => ToolResult::from(result),
        Err(payload) => {
            let reason = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_string()))
                .unwrap_or_else(|| "an unknown failure".to_string());
            ToolResult::error(format!("{} failed: {reason}", registered.tool.name))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo(_: &Session, arguments: Arguments<'_>) -> Result<String, String> {
        Ok(format!("{}/{}", arguments.string("name")?, arguments.optional("file").unwrap_or("-")))
    }

    fn sample() -> Registered {
        Registered::new(
            "sample",
            "A sample",
            vec![
                ToolArgument::new("name", "The name", "string", true),
                ToolArgument::new("file", "The file", "string", false),
            ],
            echo,
        )
    }

    // @lfy def/mcp/traits.lfy:9
    #[test]
    fn a_registered_fn_is_listed_with_a_schema() {
        let registered = sample();
        assert_eq!(registered.tool.name, "elfie_sample");
        let schema = input_schema(&registered.tool);
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(schema["properties"]["file"]["description"], "The file"); // @lfy def/mcp/traits.lfy:10
        assert_eq!(schema["required"], json!(["name"]));
        let listed = list(&[registered]);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "elfie_sample");
        assert_eq!(listed[0].description.as_deref(), Some("A sample"));
    }

    // @lfy def/mcp/traits.lfy:12
    #[test]
    fn arguments_are_checked_before_the_fn_is_called() {
        let tool = sample().tool;
        let missing = check_arguments(&tool, &serde_json::Map::new()).unwrap_err();
        assert!(missing.contains("name"), "{missing}");
        let unknown = check_arguments(&tool, json!({ "name": "a", "other": 1 }).as_object().unwrap()).unwrap_err();
        assert!(unknown.contains("other"), "{unknown}");
        let typed = check_arguments(&tool, json!({ "name": 3 }).as_object().unwrap()).unwrap_err();
        assert!(typed.contains("name") && typed.contains("string"), "{typed}");
        assert!(check_arguments(&tool, json!({ "name": "a", "file": null }).as_object().unwrap()).is_ok());
    }
}
