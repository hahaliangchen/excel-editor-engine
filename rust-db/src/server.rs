use crate::service::{Output, Service};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::SocketAddr};

pub fn router(state: Service) -> Router {
    Router::new()
        .route(
            "/api/health",
            get(|| async {
                Json(json!({"ok":true,"engine":"rust-db","drivers":["sqlite","mysql","postgres"]}))
            }),
        )
        .route("/api/connections", get(list).post(save))
        .route("/api/connections/test", post(test))
        .route("/api/connections/{id}", axum::routing::delete(delete))
        .route("/api/schema", get(schema))
        .route("/api/sqlite/attachment", post(attach))
        .route("/api/query", post(query))
        .route("/api/execute", post(execute))
        .route("/api/explain", post(explain))
        .route("/api/xlsx/export", post(export))
        .route("/api/xlsx/import", post(import))
        .layer(DefaultBodyLimit::max(46 * 1024 * 1024))
        .layer(middleware::from_fn(require_local_browser))
        .with_state(state)
}
async fn require_local_browser(req: Request, next: Next) -> Response {
    if req
        .headers()
        .get("sec-fetch-site")
        .is_some_and(|v| v == "cross-site")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(req).await
}
async fn attach(State(s): State<Service>, headers: axum::http::HeaderMap, body: Bytes) -> Response {
    if headers
        .get(header::CONTENT_TYPE)
        .is_none_or(|t| t != "application/octet-stream")
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    response(s.attach(&body).map(Output::Json))
}
fn response(answer: anyhow::Result<Output>) -> Response {
    match answer {
        Ok(Output::Json(v)) => ([(header::CACHE_CONTROL, "no-store")], Json(v)).into_response(),
        Ok(Output::Xlsx(data)) => (
            [
                (
                    header::CONTENT_TYPE,
                    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
                ),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=\"query-result.xlsx\"",
                ),
                (header::CACHE_CONTROL, "no-store"),
            ],
            data,
        )
            .into_response(),
        Err(error) => {
            let message = format!("{error:#}").chars().take(1500).collect::<String>();
            (
                StatusCode::BAD_REQUEST,
                [(header::CACHE_CONTROL, "no-store")],
                Json(json!({"error":message})),
            )
                .into_response()
        }
    }
}
macro_rules! json_route {
    ($($path:ident=>$action:literal),*) => {$(async fn $path(State(s):State<Service>,Json(mut body):Json<Value>)->Response {
        let root=body.as_object_mut().and_then(|m|m.remove("workspace_root"));
        if root.as_ref().is_some_and(|r| !r.is_string()) { return response(Err(anyhow::anyhow!("workspace_root must be a string"))); }
        response(s.call($action,root.as_ref().and_then(Value::as_str),body,false).await)
    })*};
}
json_route!(save=>"save_connection",test=>"test_connection",query=>"query",execute=>"execute",explain=>"explain",export=>"export_xlsx",import=>"import_xlsx");
async fn from_query(s: Service, action: &str, mut q: BTreeMap<String, String>) -> Response {
    let root = q.remove("workspace_root");
    let pairs = q
        .into_iter()
        .map(|(k, v)| -> anyhow::Result<_> {
            let v = match k.as_str() {
                "offset" | "limit" | "fragment_offset" => json!(v.parse::<usize>()?),
                "include_ddl" | "include_workspace_sqlite" => json!(v.parse::<bool>()?),
                _ => json!(v),
            };
            Ok((k, v))
        })
        .collect::<anyhow::Result<serde_json::Map<String, Value>>>();
    response(match pairs {
        Ok(p) => {
            s.call(action, root.as_deref(), Value::Object(p), false)
                .await
        }
        Err(e) => Err(e),
    })
}
async fn list(State(s): State<Service>, Query(q): Query<BTreeMap<String, String>>) -> Response {
    from_query(s, "list_connections", q).await
}
async fn schema(State(s): State<Service>, Query(q): Query<BTreeMap<String, String>>) -> Response {
    from_query(s, "inspect_schema", q).await
}
async fn delete(
    State(s): State<Service>,
    Path(id): Path<String>,
    Query(q): Query<BTreeMap<String, String>>,
) -> Response {
    response(
        s.delete(q.get("workspace_root").map(String::as_str), &id)
            .map(Output::Json),
    )
}

pub async fn serve() -> anyhow::Result<()> {
    let raw = std::env::var("BIND").unwrap_or_else(|_| "127.0.0.1:41902".into());
    let address: SocketAddr = raw.parse()?;
    anyhow::ensure!(
        address.ip().is_loopback(),
        "BIND must be a loopback address"
    );
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("db-server listening on http://{}", listener.local_addr()?);
    axum::serve(listener, router(Service::new()?)).await?;
    Ok(())
}
