use crate::{
    config::{Config, Environment, Kind, Profile},
    drivers::{self, Command, DbDriver, Driver, Window},
    sql_guard,
    workspace::Workspace,
    xlsx_bridge,
};
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlparser::ast::Statement;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub const JSON_BUDGET: usize = 185 * 1024;
#[derive(Clone)]
pub struct Service {
    config: Config,
    default_root: PathBuf,
    attachments: Arc<Mutex<HashMap<uuid::Uuid, Arc<tempfile::TempDir>>>>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Target {
    pub connection_id: Option<String>,
    pub sqlite_path: Option<String>,
    attachment_id: Option<uuid::Uuid>,
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct Args {
    #[serde(flatten)]
    target: Target,
    schema: Option<String>,
    table: Option<String>,
    include_ddl: bool,
    include_workspace_sqlite: Option<bool>,
    test_connection: bool,
    sql: String,
    params: Vec<Value>,
    limit: Option<usize>,
    offset: usize,
    fragment_offset: usize,
    confirm_prod: bool,
    analyze: bool,
    output_path: Option<String>,
    sheet_name: Option<String>,
    max_rows: Option<usize>,
    overwrite: bool,
    xlsx_path: Option<String>,
    xlsx_base64: Option<String>,
    sheet: Option<String>,
    table_name: Option<String>,
    mode: Option<String>,
    patch: Vec<Patch>,
    preview: bool,
}
#[derive(Deserialize)]
struct Patch {
    column: String,
    keys: std::collections::BTreeMap<String, Value>,
    previous: Value,
    value: Value,
}
pub enum Output {
    Json(Value),
    Xlsx(Vec<u8>),
}
fn parse<T: DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|_| anyhow::anyhow!("invalid request fields or types"))
}
impl Service {
    pub fn new() -> Result<Self> {
        let default_root = std::env::var_os("DB_STUDIO_WORKSPACE_ROOT")
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir()?);
        Ok(Self {
            config: Config::from_env()?,
            default_root,
            attachments: Default::default(),
        })
    }
    pub fn attach(&self, bytes: &[u8]) -> Result<Value> {
        ensure!(
            bytes.len() <= 32 * 1024 * 1024 && bytes.starts_with(b"SQLite format 3\0"),
            "expected a SQLite database snapshot of at most 32 MiB"
        );
        let mut attachments = self
            .attachments
            .lock()
            .map_err(|_| anyhow::anyhow!("attachment store unavailable"))?;
        ensure!(
            attachments.len() < 16,
            "attachment limit reached; restart the local service to clear snapshots"
        );
        let dir = tempfile::tempdir()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
        }
        std::fs::write(dir.path().join("attachment.sqlite"), bytes)?;
        let id = uuid::Uuid::new_v4();
        attachments.insert(id, Arc::new(dir));
        Ok(json!({"attachment_id":id}))
    }
    fn workspace(&self, root: Option<&str>) -> Result<Workspace> {
        Workspace::open(
            root.unwrap_or(
                self.default_root
                    .to_str()
                    .context("workspace path is not UTF-8")?,
            ),
        )
    }
    fn profile(&self, w: &Workspace, target: &Target) -> Result<Profile> {
        match (&target.connection_id, &target.sqlite_path) {
            (Some(id), None) => self.config.get(&w.root, id),
            (None, Some(path)) => Ok(Profile {
                name: path.clone(),
                file_path: Some(path.clone()),
                ..Profile::default()
            }),
            _ => bail!("provide exactly one of connection_id or sqlite_path"),
        }
    }
    pub fn delete(&self, root: Option<&str>, id: &str) -> Result<Value> {
        self.config.delete(&self.workspace(root)?.root, id)?;
        Ok(json!({"deleted":id}))
    }
    pub async fn call(
        &self,
        action: &str,
        root: Option<&str>,
        input: Value,
        rpc: bool,
    ) -> Result<Output> {
        tokio::time::timeout(
            Duration::from_secs(55),
            self.dispatch(action, root, input, rpc),
        )
        .await
        .context("database operation timed out")?
    }
    async fn dispatch(
        &self,
        action: &str,
        root: Option<&str>,
        input: Value,
        rpc: bool,
    ) -> Result<Output> {
        let w = self.workspace(root)?;
        let args: Args = parse(input.clone())?;
        ensure!(args.offset <= 10_000_000, "offset exceeds 10000000");
        if action == "list_connections" {
            let profiles = self
                .config
                .list(&w.root)?
                .iter()
                .map(Profile::public)
                .collect::<Vec<_>>();
            let workspace_sqlite = if args.include_workspace_sqlite.unwrap_or(true) {
                w.discover()?
                    .into_iter()
                    .map(|p| json!({"path":p}))
                    .collect()
            } else {
                vec![]
            };
            if rpc {
                let items = profiles
                    .into_iter()
                    .map(|v| json!({"kind":"connection","item":v}))
                    .chain(
                        workspace_sqlite
                            .into_iter()
                            .map(|v| json!({"kind":"sqlite","item":v})),
                    )
                    .collect();
                let mut page = paginate(items, &args, true)?;
                page["connections"] = json!(
                    page["items"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|x| x["kind"] == "connection")
                        .map(|x| &x["item"])
                        .collect::<Vec<_>>()
                );
                page["workspace_sqlite"] = json!(
                    page["items"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|x| x["kind"] == "sqlite")
                        .map(|x| &x["item"])
                        .collect::<Vec<_>>()
                );
                if page.get("fragment").is_none() {
                    page.as_object_mut().unwrap().remove("items");
                }
                return Ok(Output::Json(page));
            }
            return Ok(Output::Json(
                json!({"connections":profiles,"workspace_sqlite":workspace_sqlite}),
            ));
        }
        if matches!(action, "save_connection" | "test_connection") {
            let requested: Profile = parse(input)?;
            let p = self.config.prepare(&w.root, requested)?;
            if p.driver == Kind::Sqlite {
                w.sqlite(p.file_path.as_deref().unwrap_or_default(), !p.read_only)?;
            }
            let test = if args.test_connection || action == "test_connection" {
                Some(match test(&w, &p).await {
                    Ok(version) => json!({"ok":true,"version":version}),
                    Err(e) => json!({"ok":false,"error":p.clean_error(e).to_string()}),
                })
            } else {
                None
            };
            let answer = if action == "save_connection" {
                let public = self.config.save(&w.root, p.clone())?.public();
                json!({"connection":public,"test":test})
            } else {
                test.unwrap_or(Value::Null)
            };
            return Ok(Output::Json(redact(answer, p.password.as_deref())));
        }
        // Preview is independent of any DB connection and cannot write.
        if action == "import_xlsx" && args.mode.as_deref().unwrap_or("preview") == "preview" {
            let book = read_import(&w, &args, rpc)?;
            let mut preview = book.preview();
            if rpc {
                preview["rows"] = paginate(
                    book.rows.into_iter().map(|r| json!(r)).collect(),
                    &args,
                    true,
                )?;
            }
            return Ok(Output::Json(preview));
        }
        let attachment = if let Some(id) = args.target.attachment_id {
            ensure!(
                !rpc && args.target.connection_id.is_none() && args.target.sqlite_path.is_none(),
                "invalid attachment target"
            );
            Some(
                self.attachments
                    .lock()
                    .map_err(|_| anyhow::anyhow!("attachment store unavailable"))?
                    .get(&id)
                    .cloned()
                    .context("attachment expired")?,
            )
        } else {
            None
        };
        let (p, attachment_root) = if let Some(dir) = &attachment {
            let private = Workspace::open(dir.path().to_str().context("invalid attachment path")?)?;
            (
                Profile {
                    name: "SQLite attachment".into(),
                    read_only: true,
                    file_path: Some("attachment.sqlite".into()),
                    ..Profile::default()
                },
                Some(private),
            )
        } else {
            (self.profile(&w, &args.target)?, None)
        };
        let result = self
            .on_driver(
                action,
                &w,
                attachment_root.as_ref().unwrap_or(&w),
                &p,
                &args,
                rpc,
            )
            .await
            .map_err(|e| p.clean_error(e))?;
        if action == "query" && !p.id.is_empty() {
            self.config.touch(&w.root, &p.id)?;
        }
        Ok(match result {
            Output::Json(v) => Output::Json(redact(v, p.password.as_deref())),
            other => other,
        })
    }
    async fn on_driver(
        &self,
        action: &str,
        w: &Workspace,
        driver_root: &Workspace,
        p: &Profile,
        a: &Args,
        rpc: bool,
    ) -> Result<Output> {
        let start = Instant::now();
        let d = Driver::open(driver_root, p)?;
        let default_schema = match p.driver {
            Kind::Sqlite => "main",
            Kind::Mysql => p.database.as_deref().context("missing database")?,
            Kind::Postgres => "public",
        };
        let schema = a.schema.as_deref().unwrap_or(default_schema);
        let limit = limit(a, 200)?;
        let bytes = if rpc {
            JSON_BUDGET - 8192
        } else {
            8 * 1024 * 1024
        };
        let window = Window {
            limit,
            skip: a.offset,
            bytes,
        };
        let json = match action {
            "inspect_schema" => match (&a.schema, &a.table) {
                (_, Some(table)) => {
                    let detail = d.describe_table(schema, table, a.include_ddl).await?;
                    if rpc && serde_json::to_vec(&detail)?.len() > JSON_BUDGET {
                        fragment(&detail, 0, a.fragment_offset)?
                    } else {
                        detail
                    }
                }
                (Some(schema), None) => {
                    let mut result = paginate(d.list_tables(schema).await?, a, rpc)?;
                    rename_items(&mut result, "tables");
                    result["schema"] = json!(schema);
                    result
                }
                (None, None) => {
                    let mut result = paginate(
                        d.list_schemas()
                            .await?
                            .into_iter()
                            .map(Value::String)
                            .collect(),
                        a,
                        rpc,
                    )?;
                    rename_items(&mut result, "schemas");
                    result["default_schema"] = json!(default_schema);
                    result
                }
            },
            "query" | "explain" => {
                let stmt = sql_guard::readonly(p.driver, &a.sql)?;
                let (page, editable) = if action == "explain" {
                    ensure!(
                        matches!(stmt, Statement::Query(_)),
                        "EXPLAIN requires SELECT"
                    );
                    ensure!(
                        !a.analyze || p.env != Environment::Prod,
                        "EXPLAIN ANALYZE is forbidden in prod"
                    );
                    (
                        d.explain(&stmt.to_string(), &a.params, a.analyze, window)
                            .await?,
                        None,
                    )
                } else {
                    let (sql, skip) = sql_guard::paged(&stmt, limit + 1, a.offset);
                    let page = d
                        .query_readonly(&sql, &a.params, Window { skip, ..window })
                        .await?;
                    let editable = if !p.read_only && !rpc {
                        if let Some((found, table)) = sql_guard::editable_source(&stmt, p.driver) {
                            let found = found.as_deref().unwrap_or(default_schema);
                            if let Ok(detail) = d.describe_table(found, &table, false).await {
                                let keys = detail["primary_key"]
                                    .as_array()
                                    .map(Vec::as_slice)
                                    .unwrap_or_default();
                                let columns = detail["columns"]
                                    .as_array()
                                    .map(Vec::as_slice)
                                    .unwrap_or_default();
                                let names = page
                                    .columns
                                    .iter()
                                    .map(|c| c.name.as_str())
                                    .collect::<Vec<_>>();
                                if detail["kind"] == "table"
                                    && !keys.is_empty()
                                    && keys
                                        .iter()
                                        .all(|k| k.as_str().is_some_and(|n| names.contains(&n)))
                                    && names.len() == columns.len()
                                    && names
                                        .iter()
                                        .zip(columns)
                                        .all(|(&name, c)| c["name"] == name)
                                {
                                    Some(
                                        json!({"schema":found,"table":table,"primary_key":keys,"columns":columns.iter().filter(|c|c["generated"]!=true && c["primary_key"]!=true).map(|c|&c["name"]).collect::<Vec<_>>() }),
                                    )
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    (page, editable)
                };
                let next = a.offset + page.rows.len();
                let first = page.rows.first();
                if rpc
                    && first.is_some_and(|r| {
                        serde_json::to_vec(r).is_ok_and(|x| x.len() > JSON_BUDGET - 8192)
                    })
                {
                    fragment(
                        &json!({"columns":page.columns,"row":first}),
                        a.offset,
                        a.fragment_offset,
                    )?
                } else {
                    ensure!(a.fragment_offset == 0, "invalid fragment_offset");
                    let mut answer = json!({"columns":page.columns,"rows":page.rows,"offset":a.offset,"limit":limit,"truncated":page.truncated,"next_offset":page.truncated.then_some(next),"elapsed_ms":start.elapsed().as_millis(),"editable_table":editable});
                    if rpc && serde_json::to_vec(&answer)?.len() > JSON_BUDGET {
                        answer = fragment(&answer, a.offset, a.fragment_offset)?;
                    }
                    answer
                }
            }
            "execute" => {
                if a.preview {
                    ensure!(!p.read_only, "connection is read_only");
                } else {
                    p.require_write(a.confirm_prod)?;
                }
                let commands = if a.patch.is_empty() {
                    vec![Command {
                        sql: sql_guard::write(p.driver, &a.sql)?,
                        params: a.params.clone(),
                        expect_one: false,
                    }]
                } else {
                    ensure!(a.sql.is_empty(), "SQL and patch cannot be combined");
                    patches(
                        &d,
                        p.driver,
                        schema,
                        a.table.as_deref().context("patch requires table")?,
                        &a.patch,
                    )
                    .await?
                };
                if a.preview {
                    json!({"statements":commands.iter().map(|c|json!({"sql":c.sql,"params":c.params})).collect::<Vec<_>>()})
                } else {
                    json!({"rows_affected":d.execute_write(&commands).await?,"elapsed_ms":start.elapsed().as_millis()})
                }
            }
            "export_xlsx" => {
                let stmt = sql_guard::readonly(p.driver, &a.sql)?;
                let max = a.max_rows.unwrap_or(50000);
                ensure!((1..=50000).contains(&max), "max_rows must be 1..50000");
                let (sql, skip) = sql_guard::paged(&stmt, max + 1, 0);
                let mut page = d
                    .query_readonly(
                        &sql,
                        &a.params,
                        Window {
                            limit: max,
                            skip,
                            bytes: 32 * 1024 * 1024,
                        },
                    )
                    .await?;
                if let Some(secret) = p.password.as_deref().filter(|s| !s.is_empty()) {
                    for col in &mut page.columns {
                        col.name = col.name.replace(secret, "[redacted]");
                    }
                    for row in &mut page.rows {
                        for v in row {
                            *v = redact(std::mem::take(v), Some(secret));
                        }
                    }
                }
                let answer =
                    xlsx_bridge::export(&page, a.sheet_name.as_deref().unwrap_or("Query result"))?;
                if let Some(path) = &a.output_path {
                    w.write_xlsx(path, &answer, a.overwrite)?;
                    json!({"path":path,"rows":page.rows.len(),"truncated":page.truncated,"elapsed_ms":start.elapsed().as_millis()})
                } else {
                    ensure!(!rpc, "output_path is required for tools");
                    return Ok(Output::Xlsx(answer));
                }
            }
            "import_xlsx" => {
                let mode = a.mode.as_deref().unwrap_or("preview");
                ensure!(
                    matches!(mode, "create_and_insert" | "append"),
                    "invalid import mode"
                );
                p.require_write(a.confirm_prod)?;
                let table = a.table_name.as_deref().context("table_name is required")?;
                let book = read_import(w, a, rpc)?;
                let cmds = book.commands(p.driver, schema, table, mode == "create_and_insert")?;
                if p.driver == Kind::Mysql && mode == "create_and_insert" {
                    // MySQL implicitly commits DDL. Roll back the inserts and drop a table created by us on failure.
                    d.execute_write(&cmds[..1]).await?;
                    if let Err(error) = d.execute_write(&cmds[1..]).await {
                        let cleanup = Command {
                            sql: format!(
                                "DROP TABLE {}",
                                drivers::qualified(p.driver, schema, table)?
                            ),
                            params: vec![],
                            expect_one: false,
                        };
                        d.execute_write(&[cleanup])
                            .await
                            .context("insert failed and new MySQL table could not be removed")?;
                        return Err(error);
                    }
                } else {
                    d.execute_write(&cmds).await?;
                }
                json!({"table":table,"schema":schema,"sheet":book.sheet,"rows_inserted":book.rows.len()})
            }
            _ => bail!("unknown operation"),
        };
        Ok(Output::Json(json))
    }
}
async fn test(w: &Workspace, p: &Profile) -> Result<String> {
    Driver::open(w, p)?.test_conn().await
}
fn read_import(w: &Workspace, a: &Args, rpc: bool) -> Result<xlsx_bridge::Import> {
    let bytes = match (&a.xlsx_path, &a.xlsx_base64) {
        (Some(path), None) => w.read_xlsx(path)?,
        (None, Some(encoded)) if !rpc => STANDARD.decode(encoded)?,
        _ => bail!("provide one workbook path or upload"),
    };
    xlsx_bridge::Import::read(&bytes, a.sheet.as_deref())
}
fn limit(a: &Args, default: usize) -> Result<usize> {
    let limit = a.limit.unwrap_or(default);
    ensure!((1..=1000).contains(&limit), "limit must be 1..1000");
    Ok(limit)
}
fn rename_items(v: &mut Value, key: &str) {
    if let Some(obj) = v.as_object_mut()
        && let Some(items) = obj.remove("items")
    {
        obj.insert(key.into(), items);
    }
}
fn paginate(items: Vec<Value>, a: &Args, rpc: bool) -> Result<Value> {
    let count = items.len();
    let mut output = Vec::new();
    let mut bytes = 0;
    for item in items.into_iter().skip(a.offset).take(limit(a, 200)?) {
        let size = serde_json::to_vec(&item)?.len() + 1;
        if rpc && bytes + size > JSON_BUDGET - 4096 {
            if output.is_empty() {
                return fragment(&item, a.offset, a.fragment_offset);
            }
            break;
        }
        bytes += size;
        output.push(item);
    }
    ensure!(a.fragment_offset == 0, "invalid fragment_offset");
    let next = a.offset + output.len();
    Ok(
        json!({"items":output,"offset":a.offset,"total":count,"truncated":next<count,"next_offset":(next<count).then_some(next)}),
    )
}
fn fragment(value: &Value, offset: usize, start: usize) -> Result<Value> {
    let encoded = serde_json::to_string(value)?;
    ensure!(
        start < encoded.len() && encoded.is_char_boundary(start),
        "invalid fragment_offset"
    );
    let mut end = (start + 28 * 1024).min(encoded.len());
    while !encoded.is_char_boundary(end) {
        end -= 1;
    }
    Ok(
        json!({"fragment":&encoded[start..end],"offset":offset,"next_fragment_offset":(end<encoded.len()).then_some(end),"next_offset":(end==encoded.len()).then_some(offset+1),"truncated":true}),
    )
}
fn redact(mut value: Value, secret: Option<&str>) -> Value {
    fn walk(v: &mut Value, s: &str) {
        match v {
            Value::String(v) => *v = v.replace(s, "[redacted]"),
            Value::Array(a) => {
                for x in a {
                    walk(x, s);
                }
            }
            Value::Object(o) => {
                for x in o.values_mut() {
                    walk(x, s);
                }
            }
            _ => {}
        }
    }
    if let Some(s) = secret.filter(|s| !s.is_empty()) {
        walk(&mut value, s);
    }
    value
}
async fn patches(
    d: &Driver,
    kind: Kind,
    schema: &str,
    table: &str,
    patches: &[Patch],
) -> Result<Vec<Command>> {
    ensure!(patches.len() <= 1000, "too many pending changes");
    let detail = d.describe_table(schema, table, false).await?;
    ensure!(detail["kind"] == "table", "patch requires a base table");
    let keys = detail["primary_key"]
        .as_array()
        .context("missing table key metadata")?;
    ensure!(!keys.is_empty(), "patch requires a primary key");
    let cols = detail["columns"]
        .as_array()
        .context("missing column metadata")?;
    let table = drivers::qualified(kind, schema, table)?;
    let mut cmds = Vec::new();
    for change in patches {
        ensure!(
            cols.iter().any(|col| col["name"] == change.column
                && col["generated"] != true
                && col["primary_key"] != true),
            "column is not editable"
        );
        ensure!(
            keys.len() == change.keys.len(),
            "all primary key values are required"
        );
        let col = drivers::quote(kind, &change.column)?;
        let assign = if change.value.is_null() {
            "NULL".to_owned()
        } else {
            drivers::placeholder(kind, 1)
        };
        let mut params = if change.value.is_null() {
            vec![]
        } else {
            vec![change.value.clone()]
        };
        let mut filters = Vec::new();
        for keyspec in keys {
            let key = keyspec.as_str().context("invalid key metadata")?;
            let value = change
                .keys
                .get(key)
                .filter(|v| !v.is_null())
                .context("invalid primary key value")?;
            params.push(value.clone());
            filters.push(format!(
                "{} = {}",
                drivers::quote(kind, key)?,
                drivers::placeholder(kind, params.len())
            ));
        }
        if change.previous.is_null() {
            filters.push(format!("{col} IS NULL"));
        } else {
            params.push(change.previous.clone());
            filters.push(format!(
                "{col} = {}",
                drivers::placeholder(kind, params.len())
            ));
        }
        cmds.push(Command {
            sql: format!(
                "UPDATE {table} SET {col} = {assign} WHERE {}",
                filters.join(" AND ")
            ),
            params,
            expect_one: true,
        });
    }
    Ok(cmds)
}
