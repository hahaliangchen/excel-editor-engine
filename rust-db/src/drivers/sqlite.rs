use super::*;
use anyhow::Context;
use futures_util::TryStreamExt;
use sqlx::{
    Column as _, Connection, Executor, Row, SqliteConnection, TypeInfo, ValueRef,
    sqlite::{SqliteConnectOptions, SqliteRow},
};
use std::{
    ffi::{CStr, c_char, c_int, c_void},
    time::{Duration, Instant},
};

pub struct Sqlite {
    file: crate::workspace::SqliteFile,
    read_only: bool,
}
impl Sqlite {
    pub fn new(workspace: &Workspace, profile: &Profile) -> Result<Self> {
        let file = workspace.sqlite(
            profile
                .file_path
                .as_deref()
                .context("missing SQLite path")?,
            !profile.read_only,
        )?;
        Ok(Self {
            file,
            read_only: profile.read_only,
        })
    }
    async fn connect(&self, read: bool) -> Result<SqliteConnection> {
        let read = read || self.read_only;
        let name = self
            .file
            .path
            .file_name()
            .and_then(|p| p.to_str())
            .context("invalid SQLite filename")?;
        self.file.validate(name, !read)?;
        let mut conn = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&self.file.path)
                .read_only(read)
                .create_if_missing(!read)
                .busy_timeout(Duration::from_secs(5))
                .pragma("trusted_schema", "OFF")
                .pragma("query_only", if read { "ON" } else { "OFF" }),
        )
        .await?;
        let deadline = Instant::now() + Duration::from_secs(25);
        let mut handle = conn.lock_handle().await?;
        handle.set_progress_handler(10_000, move || Instant::now() < deadline);
        // The sqlx worker is paused while the handle is locked. The callback owns no Rust data.
        unsafe {
            let raw = handle.as_raw_handle().as_ptr();
            libsqlite3_sys::sqlite3_limit(
                raw,
                libsqlite3_sys::SQLITE_LIMIT_LENGTH,
                MAX_DATABASE_VALUE as c_int,
            );
            ensure!(
                libsqlite3_sys::sqlite3_set_authorizer(
                    raw,
                    Some(if read {
                        readonly_authorizer
                    } else {
                        write_authorizer
                    }),
                    std::ptr::null_mut()
                ) == libsqlite3_sys::SQLITE_OK,
                "could not install SQLite authorizer"
            );
        }
        drop(handle);
        Ok(conn)
    }
}
fn auth(read: bool, action: c_int, a: *const c_char, b: *const c_char) -> c_int {
    use libsqlite3_sys::*;
    let text = |s: *const c_char| {
        if s.is_null() {
            String::new()
        } else {
            // SQLite supplies NUL-terminated arguments for the duration of this callback.
            unsafe { CStr::from_ptr(s) }
                .to_string_lossy()
                .to_ascii_lowercase()
        }
    };
    let allowed = match action {
        SQLITE_ATTACH | SQLITE_DETACH | SQLITE_CREATE_VTABLE | SQLITE_DROP_VTABLE => false,
        SQLITE_FUNCTION => crate::sql_guard::safe_function(&text(b)),
        SQLITE_PRAGMA => crate::sql_guard::SAFE_PRAGMAS.contains(&text(a).as_str()),
        SQLITE_SELECT | SQLITE_READ | SQLITE_RECURSIVE => true,
        _ => !read,
    };
    if allowed { SQLITE_OK } else { SQLITE_DENY }
}
extern "C" fn readonly_authorizer(
    _: *mut c_void,
    action: c_int,
    a: *const c_char,
    b: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    auth(true, action, a, b)
}
extern "C" fn write_authorizer(
    _: *mut c_void,
    action: c_int,
    a: *const c_char,
    b: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    auth(false, action, a, b)
}

fn query<'a>(
    sql: &'a str,
    params: &[Value],
) -> Result<sqlx::query::Query<'a, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'a>>> {
    let mut q = sqlx::query(sql);
    for v in params {
        q = match v {
            Value::Null => q.bind(Option::<String>::None),
            Value::Bool(v) => q.bind(*v),
            Value::Number(v) => {
                if let Some(n) = v.as_i64() {
                    q.bind(n)
                } else {
                    q.bind(v.as_f64().context("invalid number")?)
                }
            }
            Value::String(v) => q.bind(v.clone()),
            _ => {
                if let Some(data) = bind_blob(v)? {
                    q.bind(data)
                } else {
                    q.bind(v.to_string())
                }
            }
        };
    }
    Ok(q)
}
fn row_values(row: &SqliteRow) -> Result<Vec<Value>> {
    (0..row.len())
        .map(|i| {
            let raw = row.try_get_raw(i)?;
            if raw.is_null() {
                return Ok(Value::Null);
            }
            let t = raw.type_info();
            Ok(match t.name() {
                "INTEGER" | "INT" | "INT8" => safe_i64(row.try_get_unchecked(i)?),
                "REAL" => json!(row.try_get_unchecked::<f64, _>(i)?),
                "BOOLEAN" => json!(row.try_get_unchecked::<bool, _>(i)?),
                "BLOB" => blob(row.try_get_unchecked(i)?),
                _ => json!(row.try_get_unchecked::<String, _>(i)?),
            })
        })
        .collect()
}
impl DbDriver for Sqlite {
    async fn test_conn(&self) -> Result<String> {
        let mut c = self.connect(true).await?;
        sqlx::query_scalar("SELECT sqlite_version()")
            .fetch_one(&mut c)
            .await
            .map_err(Into::into)
    }
    async fn list_schemas(&self) -> Result<Vec<String>> {
        self.test_conn().await?;
        Ok(vec!["main".into()])
    }
    async fn list_tables(&self, schema: &str) -> Result<Vec<Value>> {
        ensure!(schema == "main", "unknown SQLite schema");
        let mut c = self.connect(true).await?;
        let rows=sqlx::query("SELECT name,type FROM main.sqlite_schema WHERE type IN ('table','view') AND name NOT GLOB 'sqlite_*' ORDER BY name LIMIT 10001").fetch_all(&mut c).await?;
        Ok(rows.iter().map(|r| json!({"name":r.get::<String,_>(0),"kind":r.get::<String,_>(1),"estimated_rows":null})).collect())
    }
    async fn describe_table(&self, schema: &str, table: &str, ddl: bool) -> Result<Value> {
        ensure!(schema == "main", "unknown SQLite schema");
        let mut c = self.connect(true).await?;
        let def=sqlx::query("SELECT type,sql FROM main.sqlite_schema WHERE name=? COLLATE NOCASE AND type IN ('table','view')").bind(table).fetch_optional(&mut c).await?.context("table not found")?;
        let encoded = quote(Kind::Sqlite, table)?;
        let cols = sqlx::query(&format!("PRAGMA main.table_xinfo({encoded})"))
            .fetch_all(&mut c)
            .await?;
        let mut keys = Vec::new();
        let columns=cols.iter().map(|r| {
            let name=r.get::<String,_>("name"); let pk=r.get::<i64,_>("pk");
            if pk>0 {keys.push((pk,name.clone()));}
            json!({"name":name,"type":r.get::<String,_>("type"),"nullable":r.get::<i64,_>("notnull")==0 && pk==0,"primary_key":pk>0,"default":r.get::<Option<String>,_>("dflt_value"),"generated":r.get::<i64,_>("hidden")!=0})
        }).collect::<Vec<_>>();
        keys.sort_by_key(|(i, _)| *i);
        let indexes = sqlx::query(&format!("PRAGMA main.index_list({encoded})"))
            .fetch_all(&mut c)
            .await?;
        let mut index_values = Vec::new();
        for r in indexes {
            let name = r.get::<String, _>("name");
            let ident = quote(Kind::Sqlite, &name)?;
            let cols = sqlx::query(&format!("PRAGMA main.index_info({ident})"))
                .fetch_all(&mut c)
                .await?;
            index_values.push(json!({"name":name,"unique":r.get::<i64,_>("unique")!=0,"columns":cols.iter().map(|r|r.get::<Option<String>,_>("name")).collect::<Vec<_>>() }));
        }
        let fk = sqlx::query(&format!("PRAGMA main.foreign_key_list({encoded})"))
            .fetch_all(&mut c)
            .await?;
        let foreign=fk.iter().map(|r|json!({"id":r.get::<i64,_>("id"),"column":r.get::<String,_>("from"),"references_table":r.get::<String,_>("table"),"references_column":r.get::<Option<String>,_>("to"),"on_update":r.get::<String,_>("on_update"),"on_delete":r.get::<String,_>("on_delete")})).collect::<Vec<_>>();
        Ok(
            json!({"schema":schema,"table":table,"kind":def.get::<String,_>(0),"columns":columns,"primary_key":keys.into_iter().map(|(_,key)|key).collect::<Vec<_>>(),"indexes":index_values,"foreign_keys":foreign,"ddl":if ddl {def.get::<Option<String>,_>(1)}else{None}}),
        )
    }
    async fn query_readonly(&self, sql: &str, params: &[Value], window: Window) -> Result<Page> {
        let mut c = self.connect(true).await?;
        let desc = c.describe(sql).await?;
        let mut page = Page {
            columns: desc
                .columns()
                .iter()
                .map(|c| Column {
                    name: c.name().into(),
                    data_type: c.type_info().name().into(),
                })
                .collect(),
            ..Page::default()
        };
        let mut stream = query(sql, params)?.fetch(&mut c);
        let mut skipped = 0;
        let mut bytes = 0;
        while let Some(row) = stream.try_next().await? {
            if skipped < window.skip {
                skipped += 1;
                continue;
            }
            for (col, actual) in page.columns.iter_mut().zip(row.columns()) {
                if col.data_type == "NULL" {
                    col.data_type = actual.type_info().name().into();
                }
            }
            if !add_row(&mut page, row_values(&row)?, &mut bytes, window)? {
                break;
            }
        }
        Ok(page)
    }
    async fn execute_write(&self, commands: &[Command]) -> Result<u64> {
        ensure!(!self.read_only, "connection is read_only");
        let mut c = self.connect(false).await?;
        let mut tx = c.begin().await?;
        let mut total = 0;
        for cmd in commands {
            let affected = query(&cmd.sql, &cmd.params)?
                .execute(&mut *tx)
                .await?
                .rows_affected();
            ensure!(
                !cmd.expect_one || affected == 1,
                "row changed since preview; no changes committed"
            );
            total += affected;
        }
        tx.commit().await?;
        Ok(total)
    }
    async fn explain(
        &self,
        sql: &str,
        params: &[Value],
        analyze: bool,
        window: Window,
    ) -> Result<Page> {
        ensure!(!analyze, "SQLite does not support EXPLAIN ANALYZE");
        self.query_readonly(&format!("EXPLAIN QUERY PLAN {sql}"), params, window)
            .await
    }
}
