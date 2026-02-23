use anyhow::{Context, Result};
use serde_json::Value;

use crate::function::{FunctionDeclaration, JsonSchema};

/// Convert MCP tool schema to aichat FunctionDeclaration
pub fn mcp_tool_to_function(
    server_name: &str,
    tool_name: &str,
    tool_description: &str,
    input_schema: &Value,
) -> Result<FunctionDeclaration> {
    // Prefix the tool name with server name to avoid conflicts
    // Use double underscores around server name as sentinel markers
    let prefixed_name = format!("mcp__{}__{}", server_name, tool_name);

    // Convert the input schema to our JsonSchema format
    let parameters = convert_json_schema(input_schema)
        .with_context(|| format!("Failed to convert schema for tool {}", tool_name))?;

    Ok(FunctionDeclaration {
        name: prefixed_name,
        description: tool_description.to_string(),
        parameters,
        agent: false,
    })
}

/// Convert a JSON Schema object to our JsonSchema type
fn convert_json_schema(schema: &Value) -> Result<JsonSchema> {
    let mut json_schema = JsonSchema {
        type_value: schema
            .get("type")
            .and_then(|v| v.as_str())
            .map(String::from),
        description: schema
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
        properties: None,
        items: None,
        any_of: None,
        enum_value: None,
        default: schema.get("default").cloned(),
        required: None,
    };

    // Handle properties for object types
    if let Some(properties) = schema.get("properties").and_then(|v| v.as_object()) {
        let mut converted_props = indexmap::IndexMap::new();
        for (key, value) in properties {
            converted_props.insert(key.clone(), convert_json_schema(value)?);
        }
        json_schema.properties = Some(converted_props);
    }

    // Handle required fields
    if let Some(required) = schema.get("required").and_then(|v| v.as_array()) {
        json_schema.required = Some(
            required
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        );
    }

    // Handle array items
    if let Some(items) = schema.get("items") {
        json_schema.items = Some(Box::new(convert_json_schema(items)?));
    }

    // Handle anyOf
    if let Some(any_of) = schema.get("anyOf").and_then(|v| v.as_array()) {
        let mut converted = vec![];
        for item in any_of {
            converted.push(convert_json_schema(item)?);
        }
        json_schema.any_of = Some(converted);
    }

    // Handle enum
    if let Some(enum_values) = schema.get("enum").and_then(|v| v.as_array()) {
        json_schema.enum_value = Some(
            enum_values
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
        );
    }

    Ok(json_schema)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_schema_conversion() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "The name"
                }
            },
            "required": ["name"]
        });

        let result = convert_json_schema(&schema).unwrap();
        assert_eq!(result.type_value, Some("object".to_string()));
        assert!(result.properties.is_some());
        assert_eq!(result.required, Some(vec!["name".to_string()]));
    }

    #[test]
    fn test_mcp_tool_conversion() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path"
                }
            }
        });

        let func = mcp_tool_to_function("filesystem", "read_file", "Read a file", &schema).unwrap();
        assert_eq!(func.name, "mcp__filesystem__read_file");
        assert_eq!(func.description, "Read a file");
    }
}
