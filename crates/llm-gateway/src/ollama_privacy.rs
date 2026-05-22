use serde_json::Value;

pub fn force_ollama_think_false(body: &mut Value) -> bool {
    let Some(object) = body.as_object_mut() else {
        return false;
    };
    let changed = object.get("think") != Some(&Value::Bool(false));
    object.insert("think".to_string(), Value::Bool(false));
    changed
}

pub fn strip_ollama_thinking(value: &mut Value) -> bool {
    match value {
        Value::Object(object) => {
            let mut stripped = object.remove("thinking").is_some();
            for value in object.values_mut() {
                stripped |= strip_ollama_thinking(value);
            }
            stripped
        }
        Value::Array(values) => values.iter_mut().fold(false, |stripped, value| {
            strip_ollama_thinking(value) || stripped
        }),
        _ => false,
    }
}

pub fn strip_ollama_thinking_from_ndjson_chunk(chunk: &str) -> (String, bool) {
    let mut output = String::new();
    let mut stripped = false;
    let trailing_newline = chunk.ends_with('\n');

    for line in chunk.lines() {
        if line.trim().is_empty() {
            output.push_str(line);
            output.push('\n');
            continue;
        }
        match serde_json::from_str::<Value>(line.trim_end_matches('\r')) {
            Ok(mut value) => {
                let line_stripped = strip_ollama_thinking(&mut value);
                stripped |= line_stripped;
                if line_stripped {
                    output.push_str(&value.to_string());
                } else {
                    output.push_str(line);
                }
            }
            Err(_) => output.push_str(line),
        }
        output.push('\n');
    }

    if !trailing_newline && output.ends_with('\n') {
        output.pop();
    }
    (output, stripped)
}
