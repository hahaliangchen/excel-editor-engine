mod mysql;
mod postgres;
mod sqlite;

use crate::{
    config::{Kind, Profile},
    workspace::Workspace,
};
use anyhow::{Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::future::Future;

pub const MAX_DATABASE_VALUE: usize = 4 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    #[serde(rename = "type")]
    pub data_type: String,
}
#[derive(Default, Serialize)]
pub struct Page {
    pub columns: Vec<Column>,
    pub rows: Vec<Vec<Value>>,
    pub truncated: bool,
}
#[derive(Clone, Debug)]
pub struct Command {
    pub sql: String,
    pub params: Vec<Value>,
    pub expect_one: bool,
}
#[derive(Clone, Copy)]
pub struct Window {
    pub limit: usize,
    pub skip: usize,
    pub bytes: usize,
}

pub trait DbDriver {
    fn test_conn(&self) -> impl Future<Output = Result<String>> + Send;
    fn list_schemas(&self) -> impl Future<Output = Result<Vec<String>>> + Send;
    fn list_tables(&self, schema: &str) -> impl Future<Output = Result<Vec<Value>>> + Send;
    fn describe_table(
        &self,
        schema: &str,
        table: &str,
        ddl: bool,
    ) -> impl Future<Output = Result<Value>> + Send;
    fn query_readonly(
        &self,
        sql: &str,
        params: &[Value],
        window: Window,
    ) -> impl Future<Output = Result<Page>> + Send;
    fn execute_write(&self, commands: &[Command]) -> impl Future<Output = Result<u64>> + Send;
    fn explain(
        &self,
        sql: &str,
        params: &[Value],
        analyze: bool,
        window: Window,
    ) -> impl Future<Output = Result<Page>> + Send;
}

pub enum Driver {
    Sqlite(sqlite::Sqlite),
    Mysql(mysql::Mysql),
    Postgres(postgres::PgDriver),
}
impl Driver {
    pub fn open(workspace: &Workspace, profile: &Profile) -> Result<Self> {
        Ok(match profile.driver {
            Kind::Sqlite => Self::Sqlite(sqlite::Sqlite::new(workspace, profile)?),
            Kind::Mysql => Self::Mysql(mysql::Mysql::new(profile)),
            Kind::Postgres => Self::Postgres(postgres::PgDriver::new(profile)),
        })
    }
}
macro_rules! dispatch {
    ($($method:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => { impl DbDriver for Driver { $(
        async fn $method(&self,$($arg:$ty),*) -> Result<$ret> {
            match self { Self::Sqlite(d)=>d.$method($($arg),*).await, Self::Mysql(d)=>d.$method($($arg),*).await, Self::Postgres(d)=>d.$method($($arg),*).await }
        }
    )* } };
}
dispatch! {
    test_conn() -> String;
    list_schemas() -> Vec<String>;
    list_tables(schema: &str) -> Vec<Value>;
    describe_table(schema: &str, table: &str, ddl: bool) -> Value;
    query_readonly(sql: &str, params: &[Value], window: Window) -> Page;
    execute_write(commands: &[Command]) -> u64;
    explain(sql: &str, params: &[Value], analyze: bool, window: Window) -> Page;
}
pub fn quote(kind: Kind, s: &str) -> Result<String> {
    ensure!(
        !s.is_empty() && s.len() <= 255 && !s.chars().any(char::is_control),
        "invalid SQL identifier"
    );
    let mark = if kind == Kind::Mysql { '`' } else { '"' };
    Ok(format!(
        "{mark}{}{mark}",
        s.replace(mark, &format!("{mark}{mark}"))
    ))
}
pub fn qualified(kind: Kind, schema: &str, table: &str) -> Result<String> {
    Ok(format!("{}.{}", quote(kind, schema)?, quote(kind, table)?))
}
pub fn placeholder(kind: Kind, n: usize) -> String {
    if kind == Kind::Postgres {
        format!("${n}")
    } else {
        "?".into()
    }
}
pub fn blob(value: Vec<u8>) -> Value {
    json!({"encoding":"base64","data":STANDARD.encode(value)})
}
pub fn bind_blob(value: &Value) -> Result<Option<Vec<u8>>> {
    if value.get("encoding").and_then(Value::as_str) != Some("base64") {
        return Ok(None);
    }
    Ok(Some(
        STANDARD.decode(
            value["data"]
                .as_str()
                .ok_or_else(|| anyhow!("invalid binary parameter"))?,
        )?,
    ))
}
pub fn safe_i64(value: i64) -> Value {
    if value.unsigned_abs() > 9_007_199_254_740_991 {
        json!(value.to_string())
    } else {
        json!(value)
    }
}
pub fn safe_u64(value: u64) -> Value {
    if value > 9_007_199_254_740_991 {
        json!(value.to_string())
    } else {
        json!(value)
    }
}
pub fn add_row(
    page: &mut Page,
    row: Vec<Value>,
    bytes: &mut usize,
    window: Window,
) -> Result<bool> {
    if page.rows.len() == window.limit {
        page.truncated = true;
        return Ok(false);
    }
    let size = serde_json::to_vec(&row)?.len() + 1;
    ensure!(size <= 8 * 1024 * 1024, "result row exceeds 8 MiB");
    if !page.rows.is_empty() && (*bytes).saturating_add(size) > window.bytes {
        page.truncated = true;
        return Ok(false);
    }
    *bytes += size;
    page.rows.push(row);
    Ok(true)
}
fn ddl_synthetic(
    kind: Kind,
    schema: &str,
    table: &str,
    columns: &[Value],
    keys: &[String],
    constraints: &[String],
) -> Result<String> {
    let mut defs = Vec::new();
    for col in columns {
        let name = quote(kind, col["name"].as_str().unwrap_or_default())?;
        let t = col["type"].as_str().unwrap_or_default();
        let null = if col["nullable"].as_bool() == Some(false) {
            " NOT NULL"
        } else {
            ""
        };
        let default = col["default"]
            .as_str()
            .map_or_else(String::new, |v| format!(" DEFAULT {v}"));
        defs.push(format!("{name} {t}{default}{null}"));
    }
    if !keys.is_empty() {
        defs.push(format!(
            "PRIMARY KEY ({})",
            keys.iter()
                .map(|x| quote(kind, x))
                .collect::<Result<Vec<_>>>()?
                .join(", ")
        ));
    }
    defs.extend(constraints.iter().cloned());
    Ok(format!(
        "CREATE TABLE {} (\n  {}\n);",
        qualified(kind, schema, table)?,
        defs.join(",\n  ")
    ))
}
