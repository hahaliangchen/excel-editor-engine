use crate::{
    codec,
    formula::recalculate,
    model::Workbook,
    ops::{self, Operation},
    sample,
};
use anyhow::{Context, Result, ensure};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::DefaultBodyLimit,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::net::SocketAddr;
use tower_http::cors::CorsLayer;

pub fn bind_address(bind: Option<&str>, port: Option<&str>) -> Result<SocketAddr> {
    if let Some(bind) = bind.filter(|s| !s.is_empty()) {
        if let Ok(a) = bind.parse::<SocketAddr>() {
            ensure!(a.ip().is_loopback(), "BIND must be loopback");
            return Ok(a);
        }
    }
    let port = port
        .unwrap_or("8083")
        .parse::<u16>()
        .context("invalid PORT")?;
    let host = bind.filter(|s| !s.is_empty()).unwrap_or("127.0.0.1");
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let ip: std::net::IpAddr = if host == "localhost" {
        "127.0.0.1".parse()?
    } else {
        host.parse().context("invalid BIND")?
    };
    ensure!(ip.is_loopback(), "BIND must be loopback");
    Ok(SocketAddr::new(ip, port))
}

struct ApiError(anyhow::Error);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error":self.0.to_string()})),
        )
            .into_response()
    }
}
async fn task<T: Send + 'static>(
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(e.into()))?
        .map_err(ApiError)
}
fn file_response(bytes: Vec<u8>) -> Response {
    Response::builder()
        .header(
            header::CONTENT_TYPE,
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        )
        .header(
            header::CONTENT_DISPOSITION,
            "attachment; filename=workbook.xlsx",
        )
        .body(Body::from(bytes))
        .expect("static headers")
}
async fn parse(bytes: Bytes) -> Result<Json<Workbook>, ApiError> {
    task(move || codec::decode(&bytes)).await.map(Json)
}
async fn recalc(Json(mut workbook): Json<Workbook>) -> Result<Json<Workbook>, ApiError> {
    task(move || {
        workbook.validate()?;
        recalculate(&mut workbook);
        Ok(workbook)
    })
    .await
    .map(Json)
}
async fn export(Json(workbook): Json<Workbook>) -> Result<Response, ApiError> {
    task(move || codec::encode(&workbook))
        .await
        .map(file_response)
}
#[derive(Deserialize)]
struct OperationRequest {
    workbook: Workbook,
    operation: Operation,
}
async fn operate(Json(mut req): Json<OperationRequest>) -> Result<Json<Workbook>, ApiError> {
    task(move || {
        req.workbook.validate()?;
        ops::apply(&mut req.workbook, req.operation)?;
        Ok(req.workbook)
    })
    .await
    .map(Json)
}
pub fn router() -> Router {
    let xlsx = Router::new()
        .route("/sample", get(|| async { Json(sample::workbook()) }))
        .route("/parse", post(parse))
        .route("/recalc", post(recalc))
        .route("/export", post(export))
        .route(
            "/create",
            get(|| async { Json(Workbook::blank()) }).post(export),
        )
        .route("/operate", post(operate));
    Router::new()
        .route(
            "/api/health",
            get(|| async { Json(serde_json::json!({"ok":true,"engine":"rust-excel"})) }),
        )
        .nest("/api/xlsx", xlsx.clone())
        .nest("/api", xlsx)
        .layer(DefaultBodyLimit::max(96 * 1024 * 1024))
        .layer(
            CorsLayer::new()
                .allow_origin(tower_http::cors::AllowOrigin::predicate(|origin, _| {
                    origin
                        .to_str()
                        .ok()
                        .and_then(|s| s.split_once("://"))
                        .is_some_and(|(scheme, authority)| {
                            matches!(scheme, "http" | "https")
                                && authority
                                    .parse::<axum::http::uri::Authority>()
                                    .is_ok_and(|a| {
                                        matches!(a.host(), "localhost" | "127.0.0.1" | "[::1]")
                                    })
                        })
                }))
                .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
                .allow_headers([header::CONTENT_TYPE]),
        )
}
pub async fn serve() -> Result<()> {
    let address = bind_address(
        std::env::var("BIND").ok().as_deref(),
        std::env::var("PORT").ok().as_deref(),
    )?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    eprintln!(
        "excel-server listening on http://{}",
        listener.local_addr()?
    );
    axum::serve(listener, router()).await?;
    Ok(())
}
