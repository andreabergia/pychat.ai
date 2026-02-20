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
    vec![
        FunctionDeclaration {
            name: "list_globals".to_string(),
            description: "List currently defined Python globals and their type names".to_string(),
            parameters_json_schema: json!({
                "type": "object",
                "properties": {}
            }),
        },
        FunctionDeclaration {
            name: "inspect".to_string(),
            description: "Inspect a Python expression and return structured type/shape/sample/callable details".to_string(),
            parameters_json_schema: expr_schema(),
        },
        FunctionDeclaration {
            name: "eval_expr".to_string(),
            description: "Evaluate a Python expression and return value/stdout/stderr".to_string(),
            parameters_json_schema: expr_schema(),
        },
    ]
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
                thought_signature: None,
            }
        })
        .collect()
}

fn expr_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "expr": {"type": "string"}
        },
        "required": ["expr"]
    })
}

fn dispatch_one<C: CapabilityProvider>(capabilities: &C, call: &FunctionCallSpec) -> Value {
    match call.name.as_str() {
        "list_globals" => dispatch_list_globals(capabilities, call),
        "inspect" => dispatch_inspect(capabilities, call),
        "eval_expr" => dispatch_eval_expr(capabilities, call),
        _ => error_response(
            "unknown_function",
            format!("unknown function: {}", call.name),
            json!({}),
        ),
    }
}

fn dispatch_list_globals<C: CapabilityProvider>(
    capabilities: &C,
    call: &FunctionCallSpec,
) -> Value {
    if let Err(err) = expect_empty_args(call) {
        return err;
    }

    match capabilities.list_globals() {
        Ok(globals) => ok_response(json!({
            "globals": globals
                .into_iter()
                .map(|entry| json!({
                    "name": entry.name,
                    "type_name": entry.type_name,
                }))
                .collect::<Vec<_>>()
        })),
        Err(err) => map_capability_error(err),
    }
}

fn dispatch_inspect<C: CapabilityProvider>(capabilities: &C, call: &FunctionCallSpec) -> Value {
    let expr = match expect_expr_arg(call) {
        Ok(expr) => expr,
        Err(err) => return err,
    };

    match capabilities.inspect(expr) {
        Ok(info) => ok_response(info.value),
        Err(err) => map_capability_error(err),
    }
}

fn dispatch_eval_expr<C: CapabilityProvider>(capabilities: &C, call: &FunctionCallSpec) -> Value {
    let expr = match expect_expr_arg(call) {
        Ok(expr) => expr,
        Err(err) => return err,
    };

    match capabilities.eval_expr(expr) {
        Ok(info) => ok_response(json!({
            "value_repr": info.value_repr,
            "stdout": info.stdout,
            "stderr": info.stderr,
        })),
        Err(err) => map_capability_error(err),
    }
}

fn expect_empty_args(call: &FunctionCallSpec) -> Result<(), Value> {
    if call.args_json.is_null() || call.args_json.as_object().is_some_and(|obj| obj.is_empty()) {
        return Ok(());
    }

    Err(error_response(
        "invalid_args",
        format!("{} does not accept arguments", call.name),
        json!({ "args": call.args_json }),
    ))
}

fn expect_expr_arg(call: &FunctionCallSpec) -> Result<&str, Value> {
    let Some(args) = call.args_json.as_object() else {
        return Err(error_response(
            "invalid_args",
            format!("{} expects object args with expr", call.name),
            json!({ "args": call.args_json }),
        ));
    };

    let Some(expr) = args.get("expr") else {
        return Err(error_response(
            "invalid_args",
            format!("{} requires string field expr", call.name),
            json!({ "args": call.args_json }),
        ));
    };

    let Some(expr) = expr.as_str() else {
        return Err(error_response(
            "invalid_args",
            format!("{} requires expr to be a string", call.name),
            json!({ "args": call.args_json }),
        ));
    };

    Ok(expr)
}

fn ok_response(result: Value) -> Value {
    json!({
        "ok": true,
        "result": result,
    })
}

fn error_response(code: &str, message: String, details: Value) -> Value {
    json!({
        "ok": false,
        "error": {
            "code": code,
            "message": message,
            "details": details,
        }
    })
}

fn map_capability_error(err: CapabilityError) -> Value {
    match err {
        CapabilityError::PythonException(exc) => error_response(
            "python_exception",
            format!("{}: {}", exc.exc_type, exc.message),
            json!({
                "exc_type": exc.exc_type,
                "message": exc.message,
                "traceback": exc.traceback,
            }),
        ),
        CapabilityError::InvalidResultShape(msg) => error_response("internal", msg, json!({})),
        CapabilityError::Internal(msg) => error_response("internal", msg, json!({})),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::agent::dispatch::{FunctionCallSpec, dispatch_calls, tool_declarations};
    use crate::llm::provider::AssistantPart;
    use crate::python::PythonSession;

    #[test]
    fn tool_declarations_include_minimal_tools() {
        let tools = tool_declarations();
        let names = tools.into_iter().map(|t| t.name).collect::<Vec<_>>();
        assert_eq!(names, vec!["list_globals", "inspect", "eval_expr"]);
    }

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

    #[test]
    fn dispatch_inspect_returns_structured_result() {
        let session = PythonSession::initialize().expect("python");
        let responses = dispatch_calls(
            &session,
            &[FunctionCallSpec {
                id: Some("c2".to_string()),
                name: "inspect".to_string(),
                args_json: json!({ "expr": "[1, 2, 3]" }),
            }],
        );

        let AssistantPart::FunctionResponse { response_json, .. } =
            responses.first().expect("response")
        else {
            panic!("expected function response part");
        };

        assert_eq!(response_json["ok"], json!(true));
        assert_eq!(response_json["result"]["kind"], json!("sequence"));
        assert_eq!(response_json["result"]["size"]["len"], json!(3));
    }

    #[test]
    fn dispatch_eval_expr_returns_value_and_streams() {
        let session = PythonSession::initialize().expect("python");
        let responses = dispatch_calls(
            &session,
            &[FunctionCallSpec {
                id: Some("c3".to_string()),
                name: "eval_expr".to_string(),
                args_json: json!({ "expr": "1 + 2" }),
            }],
        );

        let AssistantPart::FunctionResponse { response_json, .. } =
            responses.first().expect("response")
        else {
            panic!("expected function response part");
        };

        assert_eq!(response_json["ok"], json!(true));
        assert_eq!(response_json["result"]["value_repr"], json!("3"));
    }

    #[test]
    fn dispatch_invalid_args_returns_error_envelope() {
        let session = PythonSession::initialize().expect("python");
        let responses = dispatch_calls(
            &session,
            &[FunctionCallSpec {
                id: Some("c4".to_string()),
                name: "inspect".to_string(),
                args_json: json!({ "expr": 123 }),
            }],
        );

        let AssistantPart::FunctionResponse { response_json, .. } =
            responses.first().expect("response")
        else {
            panic!("expected function response part");
        };

        assert_eq!(response_json["ok"], json!(false));
        assert_eq!(response_json["error"]["code"], json!("invalid_args"));
    }

    #[test]
    fn dispatch_removed_tool_returns_unknown_function() {
        let session = PythonSession::initialize().expect("python");
        let responses = dispatch_calls(
            &session,
            &[FunctionCallSpec {
                id: Some("c5".to_string()),
                name: "get_repr".to_string(),
                args_json: json!({ "expr": "1" }),
            }],
        );

        let AssistantPart::FunctionResponse { response_json, .. } =
            responses.first().expect("response")
        else {
            panic!("expected function response part");
        };

        assert_eq!(response_json["ok"], json!(false));
        assert_eq!(response_json["error"]["code"], json!("unknown_function"));
    }
}
