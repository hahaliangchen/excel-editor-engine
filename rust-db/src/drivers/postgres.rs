use super::*;
use anyhow::Context;
use futures_util::TryStreamExt;
use sqlx::{
    Column as _, Connection, Executor, Postgres, Row, TypeInfo, ValueRef,
    postgres::{PgArguments, PgConnectOptions, PgPoolOptions, PgRow, PgSslMode},
};
use std::time::Duration;

pub struct PgDriver {
    pool: sqlx::Pool<Postgres>,
}
impl PgDriver {
    pub fn new(p: &Profile) -> Self {
        let ssl = match p.ssl_mode.as_deref() {
            Some("disabled") => PgSslMode::Disable,
            Some("required") => PgSslMode::Require,
            Some("verify_ca") => PgSslMode::VerifyCa,
            Some("verify_full") => PgSslMode::VerifyFull,
            _ => PgSslMode::Prefer,
        };
        let options = PgConnectOptions::new()
            .host(p.host.as_deref().unwrap_or_default())
            .port(p.port.unwrap_or(5432))
            .username(p.username.as_deref().unwrap_or_default())
            .database(p.database.as_deref().unwrap_or_default())
            .ssl_mode(ssl);
        let options = if let Some(pass) = &p.password {
            options.password(pass)
        } else {
            options
        };
        let options = options.options([("statement_timeout", "25000"), ("lock_timeout", "5000")]);
        Self {
            pool: PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(8))
                .connect_lazy_with(options),
        }
    }
}
fn query<'a>(
    sql: &'a str,
    params: &[Value],
) -> Result<sqlx::query::Query<'a, Postgres, PgArguments>> {
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
                    q.bind(sqlx::types::Json(v.clone()))
                }
            }
        };
    }
    Ok(q)
}
fn row_values(r: &PgRow) -> Result<Vec<Value>> {
    (0..r.len())
        .map(|i| {
            let raw = r.try_get_raw(i)?;
            if raw.is_null() {
                return Ok(Value::Null);
            }
            let t = r.columns()[i].type_info().name();
            macro_rules! array {
                ($ty:ty) => {
                    json!(r.try_get_unchecked::<Vec<Option<$ty>>, _>(i)?)
                };
            }
            Ok(match t {
                "BOOL" => json!(r.try_get_unchecked::<bool, _>(i)?),
                "INT2" => json!(r.try_get_unchecked::<i16, _>(i)?),
                "INT4" => json!(r.try_get_unchecked::<i32, _>(i)?),
                "INT8" => safe_i64(r.try_get_unchecked(i)?),
                "FLOAT4" => json!(r.try_get_unchecked::<f32, _>(i)?),
                "FLOAT8" => json!(r.try_get_unchecked::<f64, _>(i)?),
                "NUMERIC" => json!(
                    r.try_get_unchecked::<rust_decimal::Decimal, _>(i)?
                        .to_string()
                ),
                "UUID" => json!(r.try_get_unchecked::<uuid::Uuid, _>(i)?),
                "DATE" => json!(r.try_get_unchecked::<chrono::NaiveDate, _>(i)?),
                "TIME" => json!(r.try_get_unchecked::<chrono::NaiveTime, _>(i)?),
                "TIMESTAMP" => json!(r.try_get_unchecked::<chrono::NaiveDateTime, _>(i)?),
                "TIMESTAMPTZ" => json!(r.try_get_unchecked::<chrono::DateTime<chrono::Utc>, _>(i)?),
                "JSON" | "JSONB" => r.try_get_unchecked::<sqlx::types::Json<Value>, _>(i)?.0,
                "BYTEA" => blob(r.try_get_unchecked(i)?),
                "BOOL[]" => array!(bool),
                "INT2[]" => array!(i16),
                "INT4[]" => array!(i32),
                "INT8[]" => json!(
                    r.try_get_unchecked::<Vec<Option<i64>>, _>(i)?
                        .into_iter()
                        .map(|x| x.map_or(Value::Null, safe_i64))
                        .collect::<Vec<_>>()
                ),
                "FLOAT4[]" => array!(f32),
                "FLOAT8[]" => array!(f64),
                "NUMERIC[]" => array!(rust_decimal::Decimal),
                "UUID[]" => array!(uuid::Uuid),
                "JSON[]" | "JSONB[]" => array!(Value),
                "TEXT[]" | "VARCHAR[]" | "BPCHAR[]" | "NAME[]" => array!(String),
                "DATE[]" => array!(chrono::NaiveDate),
                "TIMESTAMP[]" => array!(chrono::NaiveDateTime),
                "TIMESTAMPTZ[]" => array!(chrono::DateTime<chrono::Utc>),
                _ => json!(r.try_get_unchecked::<String, _>(i).map_err(|_| anyhow!(
                    "unsupported PostgreSQL result type {t}; cast it to text or JSON"
                ))?),
            })
        })
        .collect()
}
impl DbDriver for PgDriver {
    async fn test_conn(&self) -> Result<String> {
        Ok(sqlx::query_scalar("SELECT version()")
            .fetch_one(&self.pool)
            .await?)
    }
    async fn list_schemas(&self) -> Result<Vec<String>> {
        Ok(sqlx::query_scalar("SELECT schema_name FROM information_schema.schemata WHERE schema_name NOT IN ('pg_catalog','information_schema') AND schema_name NOT LIKE 'pg_toast%' ORDER BY schema_name LIMIT 10001").fetch_all(&self.pool).await?)
    }
    async fn list_tables(&self, schema: &str) -> Result<Vec<Value>> {
        let rows=sqlx::query("SELECT c.relname,c.relkind::text,CASE WHEN c.reltuples < 0 THEN NULL ELSE c.reltuples::bigint END FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relkind IN ('r','p','v','m','f') AND pg_catalog.has_table_privilege(c.oid,'SELECT') ORDER BY c.relname LIMIT 10001").bind(schema).fetch_all(&self.pool).await?;
        Ok(rows.iter().map(|r|json!({"name":r.get::<String,_>(0),"kind":match r.get::<String,_>(1).as_str(){"v"|"m"=>"view","f"=>"foreign_table",_=>"table"},"estimated_rows":r.get::<Option<i64>,_>(2).map(safe_i64)})).collect())
    }
    async fn describe_table(&self, schema: &str, table: &str, ddl: bool) -> Result<Value> {
        let mut c = self.pool.acquire().await?;
        let kind_raw=sqlx::query_scalar::<_,String>("SELECT c.relkind::text FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND c.relkind IN ('r','p','v','m','f') AND pg_catalog.has_table_privilege(c.oid,'SELECT')").bind(schema).bind(table).fetch_optional(&mut *c).await?.context("table not found or inaccessible")?;
        let kind = match kind_raw.as_str() {
            "v" | "m" => "view",
            "f" => "foreign_table",
            _ => "table",
        };
        let cols=sqlx::query("SELECT a.attname,pg_catalog.format_type(a.atttypid,a.atttypmod),a.attnotnull,pg_catalog.pg_get_expr(d.adbin,d.adrelid),a.attgenerated::text,a.attidentity::text FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid=a.attrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace LEFT JOIN pg_catalog.pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum WHERE n.nspname=$1 AND c.relname=$2 AND a.attnum>0 AND NOT a.attisdropped ORDER BY a.attnum").bind(schema).bind(table).fetch_all(&mut *c).await?;
        let keys:Vec<String>=sqlx::query_scalar("SELECT a.attname FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON c.oid=i.indrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(num,pos) JOIN pg_catalog.pg_attribute a ON a.attrelid=c.oid AND a.attnum=k.num WHERE n.nspname=$1 AND c.relname=$2 AND i.indisprimary AND k.pos<=i.indnkeyatts ORDER BY k.pos").bind(schema).bind(table).fetch_all(&mut *c).await?;
        let columns=cols.iter().map(|r|{let name=r.get::<String,_>(0); json!({"name":name,"type":r.get::<String,_>(1),"nullable":!r.get::<bool,_>(2),"default":r.get::<Option<String>,_>(3),"primary_key":keys.contains(&name),"generated":!r.get::<String,_>(4).is_empty() || !r.get::<String,_>(5).is_empty()})}).collect::<Vec<_>>();
        let idx=sqlx::query("SELECT ci.relname,i.indisunique,pg_catalog.pg_get_indexdef(i.indexrelid) FROM pg_catalog.pg_index i JOIN pg_catalog.pg_class c ON c.oid=i.indrelid JOIN pg_catalog.pg_class ci ON ci.oid=i.indexrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 ORDER BY ci.relname").bind(schema).bind(table).fetch_all(&mut *c).await?;
        let indexes=idx.iter().map(|r|json!({"name":r.get::<String,_>(0),"unique":r.get::<bool,_>(1),"ddl":r.get::<String,_>(2)})).collect::<Vec<_>>();
        let constraints=sqlx::query("SELECT k.conname,k.contype::text,pg_catalog.pg_get_constraintdef(k.oid) FROM pg_catalog.pg_constraint k JOIN pg_catalog.pg_class c ON c.oid=k.conrelid JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 ORDER BY k.conname").bind(schema).bind(table).fetch_all(&mut *c).await?;
        let foreign = constraints
            .iter()
            .filter(|r| r.get::<String, _>(1) == "f")
            .map(|r| json!({"name":r.get::<String,_>(0),"definition":r.get::<String,_>(2)}))
            .collect::<Vec<_>>();
        let create = if !ddl {
            None
        } else if kind == "view" {
            let view = qualified(Kind::Postgres, schema, table)?;
            let def: String =
                sqlx::query_scalar("SELECT pg_catalog.pg_get_viewdef($1::text::regclass,true)")
                    .bind(&view)
                    .fetch_one(&mut *c)
                    .await?;
            Some(format!(
                "CREATE {}VIEW {view} AS {def}",
                if kind_raw == "m" { "MATERIALIZED " } else { "" }
            ))
        } else {
            let defs = constraints
                .iter()
                .filter(|r| r.get::<String, _>(1) != "p")
                .map(|r| {
                    Ok(format!(
                        "CONSTRAINT {} {}",
                        quote(Kind::Postgres, &r.get::<String, _>(0))?,
                        r.get::<String, _>(2)
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Some(ddl_synthetic(
                Kind::Postgres,
                schema,
                table,
                &columns,
                &keys,
                &defs,
            )?)
        };
        Ok(
            json!({"schema":schema,"table":table,"kind":kind,"columns":columns,"primary_key":keys,"indexes":indexes,"foreign_keys":foreign,"ddl":create}),
        )
    }
    async fn query_readonly(&self, sql: &str, params: &[Value], window: Window) -> Result<Page> {
        let mut c = self.pool.acquire().await?;
        let mut tx = c.begin_with("BEGIN READ ONLY").await?;
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
            "EXPLAIN (ANALYZE TRUE, FORMAT JSON)"
        } else {
            "EXPLAIN (FORMAT JSON)"
        };
        self.query_readonly(&format!("{prefix} {sql}"), params, window)
            .await
    }
}
