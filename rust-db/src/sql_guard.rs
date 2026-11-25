use crate::config::Kind;
use anyhow::{Context, Result, bail, ensure};
use sqlparser::{
    ast::{Expr, ObjectName, Query, SelectItem, SetExpr, Statement, TableFactor, Visit, Visitor},
    dialect::{Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect},
    parser::Parser,
};
use std::ops::ControlFlow;

pub const SAFE_PRAGMAS: &[&str] = &[
    "table_info",
    "table_xinfo",
    "index_list",
    "index_info",
    "index_xinfo",
    "foreign_key_list",
    "database_list",
    "user_version",
    "schema_version",
    "table_list",
];
const UNSAFE_FUNCTIONS: &[&str] = &[
    "load_extension",
    "readfile",
    "writefile",
    "edit",
    "load_file",
    "get_lock",
    "release_lock",
    "nextval",
    "setval",
    "set_config",
    "pg_notify",
    "pg_sleep",
    "pg_sleep_for",
    "pg_sleep_until",
    "lo_import",
    "lo_export",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_ls_dir",
    "pg_stat_file",
];

fn parse(kind: Kind, sql: &str) -> Result<Statement> {
    ensure!(
        !sql.trim().is_empty() && sql.len() <= 128 * 1024 && !sql.contains('\0'),
        "expected SQL (max 128 KiB)"
    );
    // MySQL and MariaDB execute certain comments; keep those out of the guard grammar.
    if kind == Kind::Mysql {
        ensure!(
            !sql.contains("/*!") && !sql.contains("/*M!") && !sql.contains("/*m!"),
            "executable comments are forbidden"
        );
    }
    let dialect: &dyn Dialect = match kind {
        Kind::Sqlite => &SQLiteDialect {},
        Kind::Mysql => &MySqlDialect {},
        Kind::Postgres => &PostgreSqlDialect {},
    };
    let mut statements =
        Parser::parse_sql(dialect, sql).context("SQL parser rejected statement")?;
    ensure!(
        statements.len() == 1,
        "exactly one SQL statement is required"
    );
    Ok(statements.remove(0))
}
fn parts(name: &ObjectName) -> Option<Vec<String>> {
    name.0
        .iter()
        .map(|p| p.as_ident().map(|i| i.value.clone()))
        .collect()
}
fn body_allowed(body: &SetExpr) -> bool {
    match body {
        SetExpr::Select(s) => s.into.is_none() && s.optimizer_hints.is_empty(),
        SetExpr::Query(_) | SetExpr::Values(_) => true,
        SetExpr::SetOperation { left, right, .. } => body_allowed(left) && body_allowed(right),
        _ => false,
    }
}
struct ReadOnly {
    kind: Kind,
}
impl Visitor for ReadOnly {
    type Break = &'static str;
    fn pre_visit_query(&mut self, q: &Query) -> ControlFlow<Self::Break> {
        if !q.locks.is_empty()
            || q.for_clause.is_some()
            || q.settings.is_some()
            || q.format_clause.is_some()
            || !q.pipe_operators.is_empty()
            || !body_allowed(&q.body)
        {
            ControlFlow::Break("write-capable SELECT clause is forbidden")
        } else {
            ControlFlow::Continue(())
        }
    }
    fn pre_visit_statement(&mut self, stmt: &Statement) -> ControlFlow<Self::Break> {
        let allowed = match stmt {
            Statement::Query(_) => true,
            Statement::Explain {
                statement,
                analyze,
                options,
                ..
            } => !analyze && options.is_none() && matches!(statement.as_ref(), Statement::Query(_)),
            Statement::ExplainTable { .. }
            | Statement::ShowTables { .. }
            | Statement::ShowColumns { .. }
            | Statement::ShowCreate { .. }
            | Statement::ShowDatabases { .. }
            | Statement::ShowSchemas { .. }
            | Statement::ShowVariable { .. }
            | Statement::ShowStatus { .. }
            | Statement::ShowVariables { .. } => self.kind != Kind::Sqlite,
            Statement::Pragma { name, value, is_eq } if self.kind == Kind::Sqlite => {
                !is_eq
                    && parts(name).is_some_and(|p| {
                        p.last().is_some_and(|n| {
                            let n = n.to_ascii_lowercase();
                            SAFE_PRAGMAS.contains(&n.as_str())
                                && (value.is_none()
                                    || !matches!(
                                        n.as_str(),
                                        "user_version" | "schema_version" | "database_list"
                                    ))
                        })
                    })
            }
            _ => false,
        };
        if allowed {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(
                "only read-only SELECT, SHOW, DESCRIBE, EXPLAIN or safe PRAGMA is allowed",
            )
        }
    }
    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
        if let Expr::Function(f) = expr
            && parts(&f.name)
                .and_then(|p| p.last().map(|n| n.to_ascii_lowercase()))
                .is_some_and(|n| !safe_function(&n))
        {
            return ControlFlow::Break("side-effect or file access function is forbidden");
        }
        ControlFlow::Continue(())
    }
}
pub fn safe_function(n: &str) -> bool {
    !UNSAFE_FUNCTIONS.contains(&n) && !n.starts_with("dblink")
}
pub fn readonly(kind: Kind, sql: &str) -> Result<Statement> {
    let stmt = parse(kind, sql)?;
    if let ControlFlow::Break(reason) = stmt.visit(&mut ReadOnly { kind }) {
        bail!(reason);
    }
    Ok(stmt)
}
pub fn write(kind: Kind, sql: &str) -> Result<String> {
    let stmt = parse(kind, sql)?;
    ensure!(
        matches!(
            stmt,
            Statement::Insert(_)
                | Statement::Update(_)
                | Statement::Delete(_)
                | Statement::CreateTable(_)
                | Statement::AlterTable(_)
                | Statement::Drop { .. }
                | Statement::CreateIndex(_)
                | Statement::CreateView(_)
                | Statement::Truncate(_)
        ),
        "only INSERT, UPDATE, DELETE or supported DDL allowed"
    );
    Ok(stmt.to_string())
}
pub fn paged(stmt: &Statement, limit: usize, offset: usize) -> (String, usize) {
    let sql = stmt.to_string();
    if let Statement::Query(query) = stmt {
        // Keep top-level ordering, duplicate names and expression labels when we can append LIMIT directly.
        if query.limit_clause.is_none() && query.fetch.is_none() {
            return (format!("{sql} LIMIT {limit} OFFSET {offset}"), 0);
        }
    }
    // Existing limits and utility statements retain their SQL semantics. The driver stops consuming at the page boundary.
    (sql, offset)
}
pub fn editable_source(stmt: &Statement, kind: Kind) -> Option<(Option<String>, String)> {
    let Statement::Query(query) = stmt else {
        return None;
    };
    if query.with.is_some() {
        return None;
    }
    let select = query.body.as_select()?;
    if select.from.len() != 1
        || !select.from[0].joins.is_empty()
        || select.distinct.is_some()
        || select.having.is_some()
        || select.qualify.is_some()
        || !matches!(&select.group_by, sqlparser::ast::GroupByExpr::Expressions(list, modifiers) if list.is_empty() && modifiers.is_empty())
        || !select
            .projection
            .iter()
            .all(|p| matches!(p, SelectItem::Wildcard(_)))
    {
        return None;
    }
    let TableFactor::Table {
        name, args: None, ..
    } = &select.from[0].relation
    else {
        return None;
    };
    let mut p = parts(name)?;
    if kind == Kind::Postgres {
        // An unqualified name obeys the remote search_path, which may not be public.
        if p.len() != 2 {
            return None;
        }
        for (part, component) in p.iter_mut().zip(&name.0) {
            if component.as_ident()?.quote_style.is_none() {
                part.make_ascii_lowercase();
            }
        }
    }
    match p.as_slice() {
        [table] => Some((None, table.clone())),
        [schema, table] => Some((Some(schema.clone()), table.clone())),
        _ => None,
    }
}
