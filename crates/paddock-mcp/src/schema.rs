//! The small subset of JSON Schema that tool inputs use: enough to refuse a wrong call with a message that tells the model what to
//! write, nothing that could be abused (no `$ref`, no patterns, no remote anything).

use serde_json::Value;

/// Checks `value` against `schema`; the error names the path and what is expected. Supported keywords: `type` (a name or a
/// list), `enum`, `required`, `properties`, `additionalProperties: false`, `items`, `minItems`, `maxItems`, `minLength`, `maxLength`,
/// `minimum`, `maximum`. Others are ignored (documentation such as `description`, `default`).
pub fn validate(schema: &Value, value: &Value) -> Result<(), String> {
    check(schema, value, "arguments", 0)
}

const MAX_DEPTH: usize = 16;

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn type_matches(want: &str, v: &Value) -> bool {
    match want {
        "number" => v.is_number(),
        other => type_name(v) == other,
    }
}

fn check(schema: &Value, value: &Value, path: &str, depth: usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Err(format!("{path} is nested too deeply"));
    }
    let Some(schema) = schema.as_object() else {
        return Ok(());
    };
    if let Some(t) = schema.get("type") {
        let wanted: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !wanted.is_empty() && !wanted.iter().any(|w| type_matches(w, value)) {
            return Err(format!(
                "{path} must be {}, not {}",
                wanted.join(" or "),
                type_name(value)
            ));
        }
    }
    if let Some(Value::Array(options)) = schema.get("enum")
        && !options.contains(value)
    {
        let list: Vec<String> = options.iter().map(Value::to_string).collect();
        return Err(format!("{path} must be one of {}", list.join(", ")));
    }
    match value {
        Value::Object(obj) => {
            if let Some(Value::Array(req)) = schema.get("required") {
                for name in req.iter().filter_map(Value::as_str) {
                    if !obj.contains_key(name) {
                        return Err(format!("{path} needs \"{name}\""));
                    }
                }
            }
            let props = schema.get("properties").and_then(Value::as_object);
            for (k, v) in obj {
                match props.and_then(|p| p.get(k)) {
                    Some(s) => check(s, v, &format!("{path}.{k}"), depth + 1)?,
                    None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                        return Err(format!("{path} has an unknown field \"{k}\""));
                    }
                    None => {}
                }
            }
        }
        Value::Array(items) => {
            let len = items.len() as u64;
            if schema
                .get("minItems")
                .and_then(Value::as_u64)
                .is_some_and(|m| len < m)
            {
                return Err(format!(
                    "{path} needs at least {} items",
                    schema["minItems"]
                ));
            }
            if schema
                .get("maxItems")
                .and_then(Value::as_u64)
                .is_some_and(|m| len > m)
            {
                return Err(format!("{path} takes at most {} items", schema["maxItems"]));
            }
            if let Some(s) = schema.get("items") {
                for (i, v) in items.iter().enumerate() {
                    check(s, v, &format!("{path}[{i}]"), depth + 1)?;
                }
            }
        }
        Value::String(s) => {
            let len = s.chars().count() as u64;
            if schema
                .get("minLength")
                .and_then(Value::as_u64)
                .is_some_and(|m| len < m)
            {
                return Err(format!(
                    "{path} needs at least {} characters",
                    schema["minLength"]
                ));
            }
            if schema
                .get("maxLength")
                .and_then(Value::as_u64)
                .is_some_and(|m| len > m)
            {
                return Err(format!(
                    "{path} takes at most {} characters",
                    schema["maxLength"]
                ));
            }
        }
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if schema
                .get("minimum")
                .and_then(Value::as_f64)
                .is_some_and(|m| x < m)
            {
                return Err(format!("{path} must be at least {}", schema["minimum"]));
            }
            if schema
                .get("maximum")
                .and_then(Value::as_f64)
                .is_some_and(|m| x > m)
            {
                return Err(format!("{path} must be at most {}", schema["maximum"]));
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn err(schema: Value, v: Value) -> String {
        validate(&schema, &v).unwrap_err()
    }

    #[test]
    fn accepts_what_the_schema_describes() {
        let s = json!({"type":"object","required":["q"],"properties":{"q":{"type":"string","minLength":1,"maxLength":5},
            "n":{"type":"integer","minimum":1,"maximum":10},"tags":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":2},
            "mode":{"enum":["a","b"]},"x":{"type":["string","null"]},"f":{"type":"number"}},"additionalProperties":false});
        assert!(
            validate(
                &s,
                &json!({"q":"hi","n":3,"tags":["a"],"mode":"b","x":null,"f":1.5})
            )
            .is_ok()
        );
        assert!(
            validate(&s, &json!({"q":"hi","f":2})).is_ok(),
            "an integer is a number"
        );
        assert!(
            validate(&json!(true), &json!(1)).is_ok(),
            "a schema that is not an object accepts everything"
        );
        assert!(validate(&json!({"type": 3}), &json!(1)).is_ok());
    }

    #[test]
    fn refuses_with_a_message_that_says_what_to_write() {
        let s = json!({"type":"object","required":["q"],"properties":{"q":{"type":"string","minLength":2,"maxLength":3},
            "n":{"type":"integer","minimum":1,"maximum":10},"tags":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":1},
            "mode":{"enum":["a","b"]}},"additionalProperties":false});
        assert!(err(s.clone(), json!([])).contains("must be object, not array"));
        assert!(err(s.clone(), json!({})).contains("needs \"q\""));
        assert!(err(s.clone(), json!({"q":"ab","zz":1})).contains("unknown field \"zz\""));
        assert!(err(s.clone(), json!({"q":"a"})).contains("at least 2 characters"));
        assert!(err(s.clone(), json!({"q":"abcd"})).contains("at most 3 characters"));
        assert!(err(s.clone(), json!({"q":"ab","n":0})).contains("arguments.n must be at least 1"));
        assert!(err(s.clone(), json!({"q":"ab","n":11})).contains("at most 10"));
        assert!(err(s.clone(), json!({"q":"ab","n":1.5})).contains("must be integer, not number"));
        assert!(err(s.clone(), json!({"q":"ab","tags":[]})).contains("at least 1 items"));
        assert!(err(s.clone(), json!({"q":"ab","tags":["a","b"]})).contains("at most 1 items"));
        assert!(
            err(s.clone(), json!({"q":"ab","tags":[3]}))
                .contains("arguments.tags[0] must be string")
        );
        assert!(err(s, json!({"q":"ab","mode":"c"})).contains("one of \"a\", \"b\""));
    }

    #[test]
    fn deeply_nested_input_is_refused_not_recursed_into() {
        let mut schema = json!({"type":"string"});
        let mut value = json!("x");
        for _ in 0..40 {
            schema = json!({"type":"array","items":schema});
            value = json!([value]);
        }
        assert!(err(schema, value).contains("nested too deeply"));
    }
}
