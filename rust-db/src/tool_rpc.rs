use crate::service::{Output, Service};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
pub const MAX_OUT: usize = 200 * 1024;
const MAX_IN: u64 = 1024 * 1024;
pub const TOOLS: [&str; 9] = [
    "db__list_connections",
    "db__save_connection",
    "db__inspect_schema",
    "db__query",
    "db__explain",
    "db__execute",
    "db__export_xlsx",
    "db__import_xlsx",
    "db__focus_view",
];
fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":message.chars().take(1024).collect::<String>()}})
}
async fn request(service: &Service, v: Value, expected: Option<&str>) -> Option<Value> {
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    if v["jsonrpc"] != "2.0"
        || !matches!(id, Value::Null | Value::Number(_) | Value::String(_))
        || !v["method"].is_string()
    {
        return Some(error(Value::Null, -32600, "invalid JSON-RPC request"));
    }
    let response = if v["method"] != "tools/call" {
        error(id, -32601, "method not found")
    } else {
        let p = &v["params"];
        let tool = p["name"].as_str().unwrap_or_default();
        if !TOOLS.contains(&tool) || expected.is_some_and(|e| e != tool) {
            error(id, -32602, "unknown or mismatched tool name")
        } else if let Some(root) = p["workspace_root"].as_str() {
            let input = p.get("arguments").cloned().unwrap_or(json!({}));
            match service.call(&tool[4..], Some(root), input, true).await {
                Ok(Output::Json(result)) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                Ok(Output::Xlsx(_)) => error(id, -32602, "output_path required"),
                Err(e) => error(id, -32602, &format!("{e:#}")),
            }
        } else {
            error(id, -32602, "workspace_root is required")
        }
    };
    v.get("id").map(|_| response)
}
pub async fn serve(expected: Option<&str>) -> Result<()> {
    ensure!(
        expected.is_none_or(|n| TOOLS.contains(&n)),
        "unknown tool name"
    );
    let service = Service::new()?;
    let mut input = io::stdin().lock();
    let mut out = io::stdout().lock();
    loop {
        let mut line = Vec::new();
        let n = (&mut input).take(MAX_IN + 1).read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        let too_long = n as u64 > MAX_IN;
        let answer = if too_long {
            Some(error(Value::Null, -32600, "request exceeds 1 MiB"))
        } else {
            match serde_json::from_slice(&line) {
                Ok(v) => request(&service, v, expected).await,
                Err(_) => Some(error(Value::Null, -32700, "parse error")),
            }
        };
        if let Some(v) = answer {
            let mut bytes = serde_json::to_vec(&v)?;
            if bytes.len() + 1 > MAX_OUT {
                let id = v["id"]
                    .as_str()
                    .filter(|s| s.len() > 1024)
                    .map_or_else(|| v["id"].clone(), |_| Value::Null);
                bytes = serde_json::to_vec(&error(
                    id,
                    -32602,
                    "response exceeds 200 KiB; request a smaller page",
                ))?;
            }
            out.write_all(&bytes)?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
        if too_long {
            break;
        }
    }
    Ok(())
}
