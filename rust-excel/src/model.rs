use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const ROWS: u32 = 1_048_576;
pub const COLS: u32 = 16_384;
pub const MAX_SELECTION: u64 = 200_000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sheet {
    pub name: String,
    pub cells: BTreeMap<String, Cell>,
    pub merges: Vec<String>,
    pub widths: BTreeMap<u32, f64>,
    pub frozen: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_index: Option<usize>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Cell {
    pub raw: String,
    pub formula: Option<String>,
    pub value: Value,
    pub display: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub error: bool,
    pub style: Style,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub color: Option<String>,
    pub fill: Option<String>,
    pub align: Option<String>,
    pub number_format: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    #[default]
    Blank,
    Number(f64),
    Bool(bool),
    Text(String),
}

impl Value {
    pub fn text(&self) -> String {
        match self {
            Self::Blank => String::new(),
            Self::Number(n) => n.to_string(),
            Self::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
            Self::Text(s) => s.clone(),
        }
    }
    pub fn display(&self, format: Option<&str>) -> String {
        let Self::Number(n) = self else {
            return self.text();
        };
        let Some(f) = format.filter(|f| *f != "General") else {
            return self.text();
        };
        let section = f.split(';').next().unwrap_or(f);
        let digits = section.split_once('.').map_or(0, |(_, suffix)| {
            suffix
                .chars()
                .take_while(|c| matches!(c, '0' | '#'))
                .count()
                .min(12)
        });
        let percent = section.contains('%');
        let number = format!(
            "{:.*}",
            digits,
            if percent { n.abs() * 100.0 } else { n.abs() }
        );
        let (whole, fraction) = number.split_once('.').unwrap_or((&number, ""));
        let grouped = if section.contains(",0") || section.contains(",#") {
            whole
                .chars()
                .enumerate()
                .fold(String::new(), |mut acc, (i, c)| {
                    if i > 0 && (whole.len() - i).is_multiple_of(3) {
                        acc.push(',');
                    }
                    acc.push(c);
                    acc
                })
        } else {
            whole.into()
        };
        if !section.contains('0') && !section.contains('#') && !percent {
            return self.text();
        }
        let currency = if section.contains('¥') || section.contains('￥') {
            "¥"
        } else if section.contains('€') {
            "€"
        } else if section.contains('£') {
            "£"
        } else if section.contains('$') {
            "$"
        } else {
            ""
        };
        format!(
            "{}{currency}{grouped}{}{}",
            if *n < 0.0 { "-" } else { "" },
            if fraction.is_empty() {
                String::new()
            } else {
                format!(".{fraction}")
            },
            if percent { "%" } else { "" }
        )
    }
}

impl Cell {
    pub fn from_raw(raw: String) -> Self {
        let formula = raw.strip_prefix('=').map(str::to_owned);
        let value = if formula.is_some() || raw.is_empty() {
            Value::Blank
        } else if let Some(text) = raw.strip_prefix('\'') {
            Value::Text(text.into())
        } else if raw.eq_ignore_ascii_case("true") || raw.eq_ignore_ascii_case("false") {
            Value::Bool(raw.eq_ignore_ascii_case("true"))
        } else if let Ok(n) = raw.trim().parse::<f64>()
            && n.is_finite()
        {
            Value::Number(n)
        } else {
            Value::Text(raw.clone())
        };
        Self {
            display: value.text(),
            raw,
            formula,
            value,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Coord {
    pub row: u32,
    pub col: u32,
}
impl Coord {
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.strip_prefix('$').unwrap_or(s);
        let letters = s.bytes().take_while(u8::is_ascii_alphabetic).count();
        ensure!((1..=3).contains(&letters), "invalid cell address: {s}");
        let (a, b) = s.split_at(letters);
        let b = b.strip_prefix('$').unwrap_or(b);
        ensure!(
            !b.is_empty() && b.bytes().all(|x| x.is_ascii_digit()),
            "invalid cell address: {s}"
        );
        let col = a.bytes().fold(0u32, |n, b| {
            n * 26 + u32::from(b.to_ascii_uppercase() - b'A' + 1)
        });
        let row: u32 = b.parse()?;
        ensure!(
            (1..=ROWS).contains(&row) && (1..=COLS).contains(&col),
            "cell outside Excel limits: {s}"
        );
        Ok(Self { row, col })
    }
    pub fn column(mut col: u32) -> String {
        let mut out = Vec::new();
        while col > 0 {
            col -= 1;
            out.push((b'A' + (col % 26) as u8) as char);
            col /= 26;
        }
        out.into_iter().rev().collect()
    }
    pub fn a1(self) -> String {
        format!("{}{}", Self::column(self.col), self.row)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Range {
    pub start: Coord,
    pub end: Coord,
}
impl Range {
    pub fn parse(s: &str) -> Result<Self> {
        let (a, b) = s.split_once(':').unwrap_or((s, s));
        let (a, b) = (Coord::parse(a)?, Coord::parse(b)?);
        Ok(Self {
            start: Coord {
                row: a.row.min(b.row),
                col: a.col.min(b.col),
            },
            end: Coord {
                row: a.row.max(b.row),
                col: a.col.max(b.col),
            },
        })
    }
    pub fn a1(self) -> String {
        format!("{}:{}", self.start.a1(), self.end.a1())
    }
    pub fn size(self) -> u64 {
        u64::from(self.end.row - self.start.row + 1) * u64::from(self.end.col - self.start.col + 1)
    }
    pub fn contains(self, c: Coord) -> bool {
        (self.start.row..=self.end.row).contains(&c.row)
            && (self.start.col..=self.end.col).contains(&c.col)
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.start.row <= other.end.row
            && self.end.row >= other.start.row
            && self.start.col <= other.end.col
            && self.end.col >= other.start.col
    }
    pub fn iter(self) -> impl Iterator<Item = Coord> {
        (self.start.row..=self.end.row)
            .flat_map(move |row| (self.start.col..=self.end.col).map(move |col| Coord { row, col }))
    }
}

impl Workbook {
    pub fn blank() -> Self {
        Self {
            sheets: vec![Sheet {
                name: "Sheet1".into(),
                ..Sheet::default()
            }],
            source: None,
        }
    }
    pub fn sheet_index(&self, name: &str) -> Result<usize> {
        self.sheets
            .iter()
            .position(|s| s.name.to_lowercase() == name.to_lowercase())
            .ok_or_else(|| anyhow::anyhow!("unknown worksheet: {name}"))
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(!self.sheets.is_empty(), "workbook needs a worksheet");
        let mut names = BTreeSet::new();
        for s in &self.sheets {
            validate_name(&s.name)?;
            ensure!(
                names.insert(s.name.to_lowercase()),
                "duplicate worksheet: {}",
                s.name
            );
            for a in s.cells.keys() {
                ensure!(
                    Coord::parse(a)?.a1() == *a,
                    "cell addresses must be canonical"
                );
            }
            let mut merges = Vec::new();
            for a in &s.merges {
                let area = Range::parse(a)?;
                ensure!(
                    area.size() > 1 && !merges.iter().any(|r| area.overlaps(*r)),
                    "invalid or overlapping merge"
                );
                merges.push(area);
            }
            for (&col, &width) in &s.widths {
                ensure!(
                    (1..=COLS).contains(&col)
                        && width.is_finite()
                        && (0.0..=255.0).contains(&width),
                    "invalid column width"
                );
            }
        }
        Ok(())
    }
}

pub fn validate_name(name: &str) -> Result<()> {
    if name.trim().is_empty()
        || name.encode_utf16().count() > 31
        || name.starts_with('\'')
        || name.ends_with('\'')
        || name
            .chars()
            .any(|c| c.is_control() || "[]:*?/\\".contains(c))
    {
        bail!("invalid worksheet name: {name}");
    }
    Ok(())
}
