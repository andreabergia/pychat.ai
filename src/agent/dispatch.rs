use serde_json::{Value, json};

use crate::llm::provider::{AssistantPart, FunctionDeclaration};
use crate::python::{CapabilityError, CapabilityProvider};

#[derive(Debug, Clone)]
pub struct FunctionCallSpec {
    pub id: Option<String>,
    pub name: String,
    pub args_json: Value,
}

pub fn tool_declarations() -> Vec<FunctionDeclaration> {
    vec![FunctionDeclaration {
        name: "list_globals".to_string(),
        description: "List currently defined Python globals and their type names".to_string(),
        parameters_json_schema: json!({
            "type": "object",
            "properties": {}
        }),
    }]
}

pub fn dispatch_calls<C: CapabilityProvider>(
    capabilities: &C,
    calls: &[FunctionCallSpec],
) -> Vec<AssistantPart> {
    calls
        .iter()
        .map(|call| {
            let response_json = dispatch_one(capabilities, call);
            AssistantPart::FunctionResponse {
                id: call.id.clone(),
                name: call.name.clone(),
                response_json,
            }
        })
        .collect()
}

fn dispatch_one<C: CapabilityProvider>(capabilities: &C, call: &FunctionCallSpec) -> Value {
    match call.name.as_str() {
        "list_globals" => dispatch_list_globals(capabilities, call),
        _ => json!({
            "ok": false,
            "error": {
                "code": "unknown_function",
                "message": format!("unknown function: {}", call.name),
                "details": {}
            }
        }),
    }
}

fn dispatch_list_globals<C: CapabilityProvider>(
    capabilities: &C,
    call: &FunctionCallSpec,
) -> Value {
    // Accept omitted or empty args for the no-arg tool.
    if !(call.args_json.is_null() || call.args_json.as_object().is_some_and(|obj| obj.is_empty())) {
        return json!({
            "ok": false,
            "error": {
                "code": "invalid_args",
                "message": "list_globals does not accept arguments",
                "details": {
                    "args": call.args_json
                }
            }
        });
    }

    match capabilities.list_globals() {
        Ok(globals) => json!({
            "ok": true,
            "result": {
                "globals": globals
                    .into_iter()
                    .map(|entry| json!({
                        "name": entry.name,
                        "type_name": entry.type_name,
                    }))
                    .collect::<Vec<_>>()
            }
        }),
        Err(err) => map_capability_error(err),
    }
}

fn map_capability_error(err: CapabilityError) -> Value {
    match err {
        CapabilityError::PythonException(exc) => json!({
            "ok": false,
            "error": {
                "code": "python_exception",
                "message": format!("{}: {}", exc.exc_type, exc.message),
                "details": {
                    "exc_type": exc.exc_type,
                    "message": exc.message,
                    "traceback": exc.traceback,
                }
            }
        }),
        CapabilityError::InvalidResultShape(msg) => json!({
            "ok": false,
            "error": {
                "code": "internal",
                "message": msg,
                "details": {}
            }
        }),
        CapabilityError::Internal(msg) => json!({
            "ok": false,
            "error": {
                "code": "internal",
                "message": msg,
                "details": {}
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::agent::dispatch::{FunctionCallSpec, dispatch_calls};
    use crate::python::PythonSession;

    #[test]
    fn dispatch_list_globals_returns_ok_envelope() {
        let session = PythonSession::initialize().expect("python");
        session.exec_code("x = 1").expect("seed");

        let responses = dispatch_calls(
            &session,
            &[FunctionCallSpec {
                id: Some("c1".to_string()),
                name: "list_globals".to_string(),
                args_json: json!({}),
            }],
        );

        let first = responses.first().expect("response");
        let AssistantPart::FunctionResponse { response_json, .. } = first else {
            panic!("expected function response part");
        };

        assert_eq!(response_json["ok"], json!(true));
        assert!(response_json["result"]["globals"].is_array());
    }

    use crate::llm::provider::AssistantPart;
}
