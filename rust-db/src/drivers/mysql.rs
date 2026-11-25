use super::*;
use anyhow::Context;
use futures_util::TryStreamExt;
use sqlx::{
    Column as _, Connection, Executor, MySql, Row, TypeInfo, ValueRef,
    mysql::{MySqlArguments, MySqlConnectOptions, MySqlPoolOptions, MySqlRow, MySqlSslMode},
};
use std::time::Duration;

pub struct Mysql {
    pool: sqlx::Pool<MySql>,
}
impl Mysql {
    pub fn new(p: &Profile) -> Self {
        let ssl = match p.ssl_mode.as_deref() {
            Some("disabled") => MySqlSslMode::Disabled,
            Some("required") => MySqlSslMode::Required,
            Some("verify_ca") => MySqlSslMode::VerifyCa,
            Some("verify_full") => MySqlSslMode::VerifyIdentity,
            _ => MySqlSslMode::Preferred,
        };
        let options = MySqlConnectOptions::new()
            .host(p.host.as_deref().unwrap_or_default())
            .port(p.port.unwrap_or(3306))
            .username(p.username.as_deref().unwrap_or_default())
            .database(p.database.as_deref().unwrap_or_default())
            .ssl_mode(ssl);
        let options = if let Some(pass) = &p.password {
            options.password(pass)
        } else {
            options
        };

        Self {
            pool: MySqlPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(8))
                .connect_lazy_with(options),
        }
    }
}
fn query<'a>(
    sql: &'a str,
    params: &[Value],
) -> Result<sqlx::query::Query<'a, MySql, MySqlArguments>> {
    let mut q = sqlx::query(sql);
    for v in params {
        q = match v {
            Value::Null => q.bind(Option::<String>::None),
            Value::Bool(v) => q.bind(*v),
            Value::Number(v) => {
                if let Some(n) = v.as_i64() {
                    q.bind(n)
                } else if let Some(n) = v.as_u64() {
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
                    q.bind(sqlx::types::Json(v.clone()))
                }
            }
        };
    }
    Ok(q)
}
fn row_values(r: &MySqlRow) -> Result<Vec<Value>> {
    (0..r.len())
        .map(|i| {
            let raw = r.try_get_raw(i)?;
            if raw.is_null() {
                return Ok(Value::Null);
            }
            let t = r.columns()[i].type_info().name();

            Ok(match t {
                "BOOLEAN" => json!(r.try_get_unchecked::<bool, _>(i)?),
                "TINYINT UNSIGNED" | "SMALLINT UNSIGNED" | "INT UNSIGNED"
                | "MEDIUMINT UNSIGNED" | "BIGINT UNSIGNED" => safe_u64(r.try_get_unchecked(i)?),
                "TINYINT" | "SMALLINT" | "INT" | "MEDIUMINT" | "BIGINT" | "YEAR" => {
                    safe_i64(r.try_get_unchecked(i)?)
                }
                "FLOAT" => json!(r.try_get_unchecked::<f32, _>(i)?),
                "DOUBLE" => json!(r.try_get_unchecked::<f64, _>(i)?),
                "DECIMAL" => json!(r.try_get_unchecked::<String, _>(i)?),
                "DATE" => json!(r.try_get_unchecked::<chrono::NaiveDate, _>(i)?),
                "DATETIME" => json!(r.try_get_unchecked::<chrono::NaiveDateTime, _>(i)?),
                "TIMESTAMP" => json!(r.try_get_unchecked::<chrono::DateTime<chrono::Utc>, _>(i)?),
                "TIME" => json!(
                    r.try_get_unchecked::<sqlx::mysql::types::MySqlTime, _>(i)?
                        .to_string()
                ),
                "JSON" => r.try_get_unchecked::<sqlx::types::Json<Value>, _>(i)?.0,
                "BIT" | "BINARY" | "VARBINARY" | "TINYBLOB" | "BLOB" | "MEDIUMBLOB"
                | "LONGBLOB" | "GEOMETRY" => blob(r.try_get_unchecked(i)?),
                _ => json!(r.try_get_unchecked::<String, _>(i)?),
            })
        })
        .collect()
}
fn mysql_opt_text(r: &MySqlRow, i: usize) -> Result<Option<String>> {
    let raw = r.try_get_raw(i)?;
    if raw.is_null() {
        return Ok(None);
    }
    if let Ok(text) = r.try_get::<String, _>(i) {
        return Ok(Some(text));
    }
    let bytes: Vec<u8> = r.try_get(i)?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn mysql_text(r: &MySqlRow, i: usize) -> Result<String> {
    Ok(mysql_opt_text(r, i)?.unwrap_or_default())
}

impl DbDriver for Mysql {
    async fn test_conn(&self) -> Result<String> {
        let row = sqlx::query("SELECT VERSION()").fetch_one(&self.pool).await?;
        mysql_text(&row, 0)
    }
    async fn list_schemas(&self) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME NOT IN ('information_schema','performance_schema','mysql','sys') ORDER BY SCHEMA_NAME LIMIT 10001")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| mysql_text(r, 0)).collect()
    }
    async fn list_tables(&self, schema: &str) -> Result<Vec<Value>> {
        let rows = sqlx::query("SELECT TABLE_NAME,TABLE_TYPE,CAST(TABLE_ROWS AS UNSIGNED) FROM information_schema.TABLES WHERE TABLE_SCHEMA=? ORDER BY TABLE_NAME LIMIT 10001")
            .bind(schema)
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                let name = mysql_text(r, 0)?;
                let kind = if mysql_text(r, 1)? == "BASE TABLE" {
                    "table"
                } else {
                    "view"
                };
                let estimated_rows = r.try_get::<Option<u64>, _>(2)?.map(safe_u64);
                Ok(json!({"name": name, "kind": kind, "estimated_rows": estimated_rows}))
            })
            .collect()
    }
    async fn describe_table(&self, schema: &str, table: &str, ddl: bool) -> Result<Value> {
        let mut c = self.pool.acquire().await?;
        let table_kind_row = sqlx::query(
            "SELECT TABLE_TYPE FROM information_schema.TABLES WHERE TABLE_SCHEMA=? AND TABLE_NAME=?",
        )
        .bind(schema)
        .bind(table)
        .fetch_optional(&mut *c)
        .await?
        .context("table not found")?;
        let table_kind = mysql_text(&table_kind_row, 0)?;
        let kind = if table_kind == "BASE TABLE" {
            "table"
        } else {
            "view"
        };
        let cols = sqlx::query("SELECT COLUMN_NAME,COLUMN_TYPE,IS_NULLABLE,COLUMN_DEFAULT,EXTRA FROM information_schema.COLUMNS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY ORDINAL_POSITION")
            .bind(schema)
            .bind(table)
            .fetch_all(&mut *c)
            .await?;
        let key_rows = sqlx::query("SELECT COLUMN_NAME FROM information_schema.KEY_COLUMN_USAGE WHERE TABLE_SCHEMA=? AND TABLE_NAME=? AND CONSTRAINT_NAME='PRIMARY' ORDER BY ORDINAL_POSITION")
            .bind(schema)
            .bind(table)
            .fetch_all(&mut *c)
            .await?;
        let keys: Vec<String> = key_rows
            .iter()
            .map(|r| mysql_text(r, 0))
            .collect::<Result<_>>()?;
        let columns: Vec<Value> = cols
            .iter()
            .map(|r| {
                let name = mysql_text(r, 0)?;
                let col_type = mysql_text(r, 1)?;
                let nullable = mysql_text(r, 2)? == "YES";
                let default = mysql_opt_text(r, 3)?;
                let extra = mysql_text(r, 4)?;
                Ok(json!({
                    "name": name,
                    "type": col_type,
                    "nullable": nullable,
                    "default": default,
                    "primary_key": keys.contains(&name),
                    "generated": extra.contains("GENERATED"),
                }))
            })
            .collect::<Result<_>>()?;
        let indexes = sqlx::query("SELECT INDEX_NAME,CAST(NON_UNIQUE AS SIGNED),COLUMN_NAME FROM information_schema.STATISTICS WHERE TABLE_SCHEMA=? AND TABLE_NAME=? ORDER BY INDEX_NAME,SEQ_IN_INDEX")
            .bind(schema)
            .bind(table)
            .fetch_all(&mut *c)
            .await?;
        let mut grouped = std::collections::BTreeMap::<String, (bool, Vec<Option<String>>)>::new();
        for r in indexes {
            let idx_name = mysql_text(&r, 0)?;
            let non_unique = r.try_get::<i64, _>(1)? == 0;
            let col_name = mysql_opt_text(&r, 2)?;
            grouped
                .entry(idx_name)
                .or_insert_with(|| (non_unique, vec![]))
                .1
                .push(col_name);
        }
        let indexes = grouped
            .into_iter()
            .map(|(n, (u, c))| json!({"name":n,"unique":u,"columns":c}))
            .collect::<Vec<_>>();
        let fks = sqlx::query("SELECT k.CONSTRAINT_NAME,k.COLUMN_NAME,k.REFERENCED_TABLE_SCHEMA,k.REFERENCED_TABLE_NAME,k.REFERENCED_COLUMN_NAME,r.UPDATE_RULE,r.DELETE_RULE FROM information_schema.KEY_COLUMN_USAGE k JOIN information_schema.REFERENTIAL_CONSTRAINTS r ON r.CONSTRAINT_SCHEMA=k.CONSTRAINT_SCHEMA AND r.TABLE_NAME=k.TABLE_NAME AND r.CONSTRAINT_NAME=k.CONSTRAINT_NAME WHERE k.TABLE_SCHEMA=? AND k.TABLE_NAME=? AND k.REFERENCED_TABLE_NAME IS NOT NULL ORDER BY k.CONSTRAINT_NAME,k.ORDINAL_POSITION")
            .bind(schema)
            .bind(table)
            .fetch_all(&mut *c)
            .await?;
        let foreign: Vec<Value> = fks
            .iter()
            .map(|r| {
                Ok(json!({
                    "name": mysql_text(r, 0)?,
                    "column": mysql_text(r, 1)?,
                    "references_schema": mysql_text(r, 2)?,
                    "references_table": mysql_text(r, 3)?,
                    "references_column": mysql_text(r, 4)?,
                    "on_update": mysql_text(r, 5)?,
                    "on_delete": mysql_text(r, 6)?,
                }))
            })
            .collect::<Result<_>>()?;
        let create = if ddl {
            let stmt = format!(
                "SHOW CREATE {} {}",
                if kind == "table" { "TABLE" } else { "VIEW" },
                qualified(Kind::Mysql, schema, table)?
            );
            let row = sqlx::query(&stmt).fetch_one(&mut *c).await?;
            Some(mysql_text(&row, 1)?)
        } else {
            None
        };
        Ok(
            json!({"schema":schema,"table":table,"kind":kind,"columns":columns,"primary_key":keys,"indexes":indexes,"foreign_keys":foreign,"ddl":create}),
        )
    }
    async fn query_readonly(&self, sql: &str, params: &[Value], window: Window) -> Result<Page> {
        let mut c = self.pool.acquire().await?;
        let mut tx = c.begin_with("START TRANSACTION READ ONLY").await?;
        let desc = (&mut *tx).describe(sql).await?;
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
        let mut stream = query(sql, params)?.fetch(&mut *tx);
        let mut skipped = 0;
        let mut bytes = 0;
        while let Some(row) = stream.try_next().await? {
            if skipped < window.skip {
                skipped += 1;
                continue;
            }
            if !add_row(&mut page, row_values(&row)?, &mut bytes, window)? {
                break;
            }
        }
        drop(stream);
        tx.rollback().await?;
        Ok(page)
    }
    async fn execute_write(&self, commands: &[Command]) -> Result<u64> {
        let mut tx = self.pool.begin().await?;
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
        let prefix = if analyze {
            "EXPLAIN ANALYZE"
        } else {
            "EXPLAIN FORMAT=JSON"
        };
        self.query_readonly(&format!("{prefix} {sql}"), params, window)
            .await
    }
}
