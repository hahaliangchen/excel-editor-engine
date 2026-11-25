use db_studio_engine::{config::Kind, sql_guard, tool_rpc::MAX_OUT};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
use tempfile::TempDir;
struct Fixture {
    root: TempDir,
    private: TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            private: tempfile::tempdir().unwrap(),
        }
    }
    fn raw(&self, name: &str, mut args: Value) -> Value {
        let tool = format!("db__{name}");
        if !matches!(name, "save_connection" | "list_connections")
            && args.get("sqlite_path").is_none()
            && args.get("connection_id").is_none()
        {
            args["sqlite_path"] = json!("main.db");
        }
        let request = json!({"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":tool,"workspace_root":self.root.path(),"arguments":args}});
        let mut child = Command::new(env!("CARGO_BIN_EXE_db-server"))
            .args(["--tool", &tool])
            .env(
                "DB_STUDIO_DATA_DIR",
                self.private.path().join("credentials"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(child.stdin.take().unwrap(), "{request}").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stdout.len() <= MAX_OUT);
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["id"], 9);
        assert_eq!(response["jsonrpc"], "2.0");
        response
    }
    fn ok(&self, name: &str, args: Value) -> Value {
        let response = self.raw(name, args);
        assert!(response.get("error").is_none(), "{name}: {response}");
        response["result"].clone()
    }
    fn err(&self, name: &str, args: Value, contains: &str) {
        let response = self.raw(name, args);
        assert!(
            response["error"]["message"]
                .as_str()
                .is_some_and(|s| s.contains(contains)),
            "{response}"
        );
    }
    fn write(&self, sql: &str) {
        self.ok("execute", json!({"sql":sql}));
    }
}
#[test]
fn guard_has_dialect_and_nested_permissions() {
    for sql in [
        "SELECT 42",
        "WITH cte AS (SELECT 2) SELECT * FROM cte",
        "EXPLAIN SELECT 42",
        "PRAGMA table_info('x')",
        "SELECT ';' AS a; -- trailing",
    ] {
        assert!(sql_guard::readonly(Kind::Sqlite, sql).is_ok(), "{sql}");
    }
    for sql in [
        "DROP TABLE x",
        "DELETE FROM x",
        "UPDATE x SET a=1",
        "INSERT INTO x VALUES (1)",
        "SELECT 1; DROP TABLE x",
        "/* comment */ DELETE FROM x",
        "PRAGMA user_version(2)",
        "PRAGMA writable_schema=1",
        "ATTACH DATABASE 'a' AS b",
        "SELECT load_extension('a')",
    ] {
        assert!(sql_guard::readonly(Kind::Sqlite, sql).is_err(), "{sql}");
    }
    for sql in [
        "WITH c AS (DELETE FROM tests RETURNING id) SELECT * FROM c",
        "SELECT id INTO copied FROM tests",
        "SELECT id FROM tests FOR UPDATE",
        "EXPLAIN (ANALYZE TRUE) SELECT 1",
    ] {
        assert!(sql_guard::readonly(Kind::Postgres, sql).is_err(), "{sql}");
    }
    assert!(sql_guard::readonly(Kind::Mysql, "SELECT 1 /*!12345 UNION SELECT 2 */").is_err());
    assert!(sql_guard::readonly(Kind::Mysql, "SHOW TABLES").is_ok());
}
#[test]
fn sqlite_metadata_types_params_and_paging() {
    let f = Fixture::new();
    f.write("CREATE TABLE t (id INTEGER PRIMARY KEY, score REAL, label TEXT, payload BLOB)");
    f.write("CREATE INDEX idx_t_label ON t(label)");
    f.write("INSERT INTO t VALUES (1,1.5,'A',X'ABCD'),(2,2.5,NULL,NULL),(3,3.5,'C',X'00')");
    let page = f.ok(
        "query",
        json!({"sql":"SELECT * FROM t WHERE id >= ? ORDER BY id","params":[1],"limit":1}),
    );
    assert_eq!(
        page["rows"],
        json!([[1,1.5,"A",{"encoding":"base64","data":"q80="}]])
    );
    assert_eq!(page["next_offset"], 1);
    let next = f.ok(
        "query",
        json!({"sql":"SELECT * FROM t ORDER BY id LIMIT 2","limit":1,"offset":1}),
    );
    assert_eq!(next["rows"], json!([[2, 2.5, null, null]]));
    assert_eq!(next["truncated"], false);
    let schema = f.ok(
        "inspect_schema",
        json!({"schema":"main","table":"t","include_ddl":true}),
    );
    assert_eq!(schema["primary_key"], json!(["id"]));
    assert_eq!(schema["indexes"][0]["name"], "idx_t_label");
    assert!(schema["ddl"].as_str().unwrap().contains("CREATE TABLE t"));
    assert!(
        f.ok("explain", json!({"sql":"SELECT * FROM t WHERE label='A'"}))["rows"]
            .to_string()
            .contains("idx_t_label")
    );
    assert_eq!(
        f.ok("list_connections", json!({}))["workspace_sqlite"][0]["path"],
        "main.db"
    );
}
#[test]
fn credentials_prod_and_readonly() {
    let f = Fixture::new();
    f.write("CREATE TABLE t (id INTEGER PRIMARY KEY)");
    let remote=f.ok("save_connection",json!({"id":"remote","name":"remote","driver":"postgres","database":"app","host":"127.0.0.1","username":"svc","password":"hidden-value"}));
    assert_eq!(remote["connection"]["has_password"], true);
    assert!(remote["connection"].get("password").is_none());
    let updated=f.ok("save_connection",json!({"id":"remote","name":"remote","driver":"postgres","database":"app","host":"127.0.0.1","username":"svc","password":""}));
    assert_eq!(updated["connection"]["has_password"], true);
    for (id, env, ro) in [("prod", "prod", false), ("ro", "dev", true)] {
        f.ok("save_connection",json!({"id":id,"name":id,"driver":"sqlite","env":env,"read_only":ro,"file_path":"main.db"}));
    }
    f.err(
        "execute",
        json!({"connection_id":"prod","sql":"INSERT INTO t VALUES(1)"}),
        "confirm_prod",
    );
    f.err(
        "execute",
        json!({"connection_id":"ro","sql":"INSERT INTO t VALUES(1)","confirm_prod":true}),
        "read_only",
    );
    f.err(
        "explain",
        json!({"connection_id":"prod","sql":"SELECT * FROM t","analyze":true}),
        "forbidden in prod",
    );
    f.ok(
        "execute",
        json!({"connection_id":"prod","sql":"INSERT INTO t VALUES (1)","confirm_prod":true}),
    );
    f.ok(
        "query",
        json!({"connection_id":"prod","sql":"SELECT * FROM t"}),
    );
    let public = f.ok("list_connections", json!({}));
    assert_eq!(public["connections"][0]["id"], "prod");
    assert!(!public.to_string().contains("hidden-value"));
    let path = f.private.path().join("credentials/connections.json");
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("hidden-value")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(!f.root.path().join("connections.json").exists());
}
#[test]
fn xlsx_roundtrip_literal_values_and_paths() {
    let f = Fixture::new();
    f.write("CREATE TABLE t (id INTEGER, score REAL, label TEXT)");
    f.write("INSERT INTO t VALUES (7,2.5,'=1+1'),(8,3.5,'raw')");
    f.ok(
        "export_xlsx",
        json!({"sql":"SELECT * FROM t ORDER BY id","output_path":"result.xlsx"}),
    );
    let bytes = std::fs::read(f.root.path().join("result.xlsx")).unwrap();
    let workbook = excel_editor_engine::codec::decode(&bytes).unwrap();
    let sheet = &workbook.sheets[0];
    assert!(sheet.frozen && sheet.cells["A1"].style.bold);
    assert_eq!(sheet.cells["C2"].formula, None);
    assert_eq!(sheet.cells["C2"].value.text(), "=1+1");
    let summary = f.ok(
        "import_xlsx",
        json!({"xlsx_path":"result.xlsx","mode":"preview","table_name":"copy"}),
    );
    assert_eq!(summary["row_count"], 2);
    assert_eq!(summary["columns"][0]["type"], "BIGINT");
    f.ok(
        "import_xlsx",
        json!({"xlsx_path":"result.xlsx","mode":"create_and_insert","table_name":"copy"}),
    );
    assert_eq!(
        f.ok("query", json!({"sql":"SELECT * FROM copy ORDER BY id"}))["rows"],
        json!([[7, 2.5, "=1+1"], [8, 3.5, "raw"]])
    );
    f.err(
        "query",
        json!({"sqlite_path":"../../etc/passwd","sql":"SELECT 1"}),
        "workspace file extension",
    );
    f.err(
        "query",
        json!({"sqlite_path":"../outside.db","sql":"SELECT 1"}),
        "stay within workspace",
    );
    f.err(
        "export_xlsx",
        json!({"sql":"SELECT * FROM t","output_path":"../escape.xlsx"}),
        "stay within workspace",
    );
}
#[cfg(unix)]
#[test]
fn rejects_file_parent_and_sqlite_sidecar_symlinks() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    f.write("CREATE TABLE t (id INTEGER)");
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("foreign.db"), "outside").unwrap();
    symlink(
        outside.path().join("foreign.db"),
        f.root.path().join("linked.db"),
    )
    .unwrap();
    symlink(outside.path(), f.root.path().join("linked")).unwrap();
    f.err(
        "query",
        json!({"sqlite_path":"linked.db","sql":"SELECT 1"}),
        "symlinks",
    );
    f.err(
        "query",
        json!({"sqlite_path":"linked/foreign.db","sql":"SELECT 1"}),
        "symlink",
    );
    symlink(
        outside.path().join("foreign.db"),
        f.root.path().join("main.db-wal"),
    )
    .unwrap();
    f.err("query", json!({"sql":"SELECT 1"}), "symlinks");
    assert_eq!(
        std::fs::read_to_string(outside.path().join("foreign.db")).unwrap(),
        "outside"
    );
    assert!(Path::new(outside.path()).is_absolute());
}
#[test]
fn rpc_large_result_pages_and_fragments() {
    let f = Fixture::new();
    f.write("CREATE TABLE t (id INTEGER)");
    let query = "WITH RECURSIVE r(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM r WHERE n<200) SELECT n, hex(zeroblob(1500)) FROM r";
    let first = f.ok("query", json!({"sql":query}));
    assert_eq!(first["truncated"], true);
    let next = first["next_offset"].as_u64().unwrap();
    assert!(next > 1 && next < 200);
    assert_eq!(
        f.ok("query", json!({"sql":query,"offset":next,"limit":1}))["rows"][0][0],
        next + 1
    );
    let mut offset = 0;
    let mut value = String::new();
    let sql = "SELECT hex(zeroblob(110000)) AS big";
    loop {
        let result = f.ok("query", json!({"sql":sql,"fragment_offset":offset}));
        value.push_str(result["fragment"].as_str().unwrap());
        if let Some(next) = result["next_fragment_offset"].as_u64() {
            offset = next;
        } else {
            break;
        }
    }
    assert_eq!(
        serde_json::from_str::<Value>(&value).unwrap()["row"][0]
            .as_str()
            .unwrap()
            .len(),
        220000
    );
}
