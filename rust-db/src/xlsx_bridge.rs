use crate::{
    config::Kind,
    drivers::{self, Command, Page},
};
use anyhow::{Context, Result, ensure};
use excel_editor_engine::{
    codec,
    model::{Cell, Coord, Sheet, Style, Value as CellValue, Workbook},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

pub fn export(page: &Page, name: &str) -> Result<Vec<u8>> {
    ensure!(
        page.columns.len() <= 16384,
        "too many result columns for .xlsx"
    );
    let mut cells = BTreeMap::new();
    let mut widths = BTreeMap::new();
    for (col, c) in page.columns.iter().enumerate() {
        let col = col as u32 + 1;
        let mut cell = text_cell(c.name.clone())?;
        cell.style = Style {
            bold: true,
            color: Some("#FFFFFF".into()),
            fill: Some("#147852".into()),
            ..Default::default()
        };
        widths.insert(col, (c.name.chars().count() + 3).clamp(12, 48) as f64);
        cells.insert(Coord { row: 1, col }.a1(), cell);
    }
    for (row, values) in page.rows.iter().enumerate() {
        for (col, value) in values.iter().enumerate() {
            let col = col as u32 + 1;
            let cell = match value {
                Value::Null => continue,
                Value::Number(n)
                    if n.as_i64()
                        .is_some_and(|v| v.unsigned_abs() >= 1_000_000_000_000_000)
                        || n.as_u64().is_some_and(|v| v >= 1_000_000_000_000_000) =>
                {
                    text_cell(n.to_string())?
                }
                Value::Number(n) => Cell::from_raw(n.to_string()),
                Value::Bool(n) => Cell::from_raw(n.to_string()),
                Value::String(s) => text_cell(s.clone())?,
                v => text_cell(v.to_string())?,
            };
            let measured = cell.display.chars().count().saturating_add(2).min(48) as f64;
            widths.entry(col).and_modify(|w| *w = w.max(measured));
            cells.insert(
                Coord {
                    row: row as u32 + 2,
                    col,
                }
                .a1(),
                cell,
            );
        }
    }
    let book = Workbook {
        sheets: vec![Sheet {
            name: name.into(),
            cells,
            widths,
            frozen: true,
            ..Default::default()
        }],
        source: None,
    };
    let bytes = codec::encode(&book)?;
    ensure!(bytes.len() <= codec::MAX_FILE, "export exceeds 32 MiB");
    Ok(bytes)
}
fn text_cell(s: String) -> Result<Cell> {
    codec::ensure_text(&s)?;
    Ok(Cell {
        raw: format!("'{s}"),
        formula: None,
        value: CellValue::Text(s.clone()),
        display: s,
        ..Default::default()
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Type {
    Integer,
    Real,
    Boolean,
    Text,
}
impl Type {
    fn sql(self) -> &'static str {
        match self {
            Self::Integer => "BIGINT",
            Self::Real => "DOUBLE PRECISION",
            Self::Boolean => "BOOLEAN",
            Self::Text => "TEXT",
        }
    }
    fn detect(v: &Value) -> Self {
        if v.is_boolean() {
            Self::Boolean
        } else if v.is_i64() {
            Self::Integer
        } else if v.is_number() {
            Self::Real
        } else {
            Self::Text
        }
    }
    fn unify(self, other: Self) -> Self {
        if self == other {
            self
        } else if matches!(
            (self, other),
            (Self::Integer, Self::Real) | (Self::Real, Self::Integer)
        ) {
            Self::Real
        } else {
            Self::Text
        }
    }
}
pub struct Import {
    pub sheet: String,
    pub names: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    types: Vec<Type>,
}
impl Import {
    pub fn read(bytes: &[u8], target: Option<&str>) -> Result<Self> {
        let book = codec::decode(bytes)?;
        let sheet = &book.sheets[if let Some(name) = target {
            book.sheet_index(name)?
        } else {
            0
        }];
        let mut positions = BTreeMap::new();
        for (address, cell) in &sheet.cells {
            let pos = Coord::parse(address)?;
            if cell.value != CellValue::Blank {
                positions.insert((pos.row, pos.col), &cell.value);
            }
        }
        let max_col = positions
            .keys()
            .map(|p| p.1)
            .max()
            .context("workbook sheet has no data")?;
        let max_row = positions.keys().map(|p| p.0).max().unwrap_or(1);
        ensure!(
            max_col <= 512
                && max_row <= 50001
                && u64::from(max_col) * u64::from(max_row) <= 1_000_000,
            "import supports up to 512 columns, 50000 rows and 1000000 cells"
        );
        let mut names = Vec::new();
        let mut used = HashSet::new();
        let mut inferred = vec![None::<Type>; max_col as usize];
        for col in 1..=max_col {
            let original = positions
                .get(&(1, col))
                .map(|x| x.text())
                .unwrap_or_default();
            let base: String = original
                .trim()
                .chars()
                .filter(|c| !c.is_control())
                .take(14)
                .collect();
            let base = if base.is_empty() {
                format!("column_{col}")
            } else if original.len() <= 56 && !original.chars().any(char::is_control) {
                original.trim().into()
            } else {
                base
            };
            let mut name = base.clone();
            let mut i = 2;
            while !used.insert(name.to_lowercase()) {
                name = format!("{base}_{i}");
                i += 1;
            }
            names.push(name);
        }
        let mut rows = Vec::new();
        for row in 2..=max_row {
            let values = (1..=max_col)
                .map(|col| match positions.get(&(row, col)) {
                    Some(CellValue::Number(v))
                        if v.fract() == 0.0 && v.abs() < 9_007_199_254_740_992.0 =>
                    {
                        json!(*v as i64)
                    }
                    Some(CellValue::Number(v)) => json!(v),
                    Some(CellValue::Bool(b)) => json!(b),
                    Some(CellValue::Text(text)) => json!(text),
                    _ => Value::Null,
                })
                .collect::<Vec<_>>();
            if values.iter().all(Value::is_null) {
                continue;
            }
            for (i, v) in values.iter().enumerate().filter(|(_, v)| !v.is_null()) {
                let detected = Type::detect(v);
                inferred[i] = Some(inferred[i].map_or(detected, |prior| prior.unify(detected)));
            }
            rows.push(values);
        }
        let types = inferred
            .into_iter()
            .map(|t| t.unwrap_or(Type::Text))
            .collect();
        Ok(Self {
            sheet: sheet.name.clone(),
            names,
            rows,
            types,
        })
    }
    pub fn preview(&self) -> Value {
        json!({"sheet":self.sheet,"columns":self.names.iter().zip(&self.types).map(|(name,t)|json!({"name":name,"type":t.sql()})).collect::<Vec<_>>(),"rows":&self.rows[..self.rows.len().min(20)],"row_count":self.rows.len(),"truncated":self.rows.len()>20})
    }
    pub fn commands(
        &self,
        kind: Kind,
        schema: &str,
        table: &str,
        create: bool,
    ) -> Result<Vec<Command>> {
        let table = drivers::qualified(kind, schema, table)?;
        let names = self
            .names
            .iter()
            .map(|n| drivers::quote(kind, n))
            .collect::<Result<Vec<_>>>()?;
        let mut commands = Vec::new();
        if create {
            let definitions = names
                .iter()
                .zip(&self.types)
                .map(|(n, t)| format!("{n} {}", t.sql()))
                .collect::<Vec<_>>()
                .join(", ");
            commands.push(Command {
                sql: format!("CREATE TABLE {table} ({definitions})"),
                params: vec![],
                expect_one: false,
            });
        }
        for row in &self.rows {
            let mut params = Vec::new();
            let placeholders = row
                .iter()
                .zip(&self.types)
                .map(|(v, t)| {
                    if v.is_null() {
                        return "NULL".to_owned();
                    }
                    params.push(match t {
                        Type::Text if !v.is_string() => json!(v.to_string()),
                        Type::Real => json!(v.as_f64()),
                        _ => v.clone(),
                    });
                    drivers::placeholder(kind, params.len())
                })
                .collect::<Vec<_>>()
                .join(", ");
            commands.push(Command {
                sql: format!(
                    "INSERT INTO {table} ({}) VALUES ({placeholders})",
                    names.join(", ")
                ),
                params,
                expect_one: false,
            });
        }
        Ok(commands)
    }
}
