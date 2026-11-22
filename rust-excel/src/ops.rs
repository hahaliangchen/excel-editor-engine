use crate::{
    codec::{ensure_text, set_width},
    formula::{RefEdit, recalculate, rewrite},
    model::{COLS, Cell, Coord, MAX_SELECTION, ROWS, Range, Sheet, Style, Workbook, validate_name},
};
use anyhow::{Result, bail, ensure};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum Input {
    Text(String),
    Number(f64),
    Bool(bool),
    Blank,
}
impl Input {
    fn raw(self) -> String {
        match self {
            Self::Text(s) => s,
            Self::Number(n) => n.to_string(),
            Self::Bool(b) => if b { "TRUE" } else { "FALSE" }.into(),
            Self::Blank => String::new(),
        }
    }
}

pub fn write_cells(
    wb: &mut Workbook,
    sheet: &str,
    cells: BTreeMap<String, Input>,
) -> Result<Vec<String>> {
    let i = wb.sheet_index(sheet)?;
    ensure!(
        cells.len() as u64 <= MAX_SELECTION,
        "too many cells in one transaction"
    );
    let values = cells
        .into_iter()
        .map(|(a, v)| {
            let a = Coord::parse(&a)?.a1();
            let raw = v.raw();
            ensure_text(&raw)?;
            Ok((a, raw))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut changed = Vec::new();
    for (a, raw) in values {
        let old = wb.sheets[i].cells.get(&a);
        let mut c = Cell::from_raw(raw);
        if let Some(old) = old {
            c.style = old.style.clone();
            c.origin = old.origin.clone();
        }
        if !c.raw.is_empty() || old.is_some() {
            wb.sheets[i].cells.insert(a.clone(), c);
        }
        changed.push(a);
    }
    changed.sort_unstable();
    changed.dedup();
    recalculate(wb);
    Ok(changed)
}

// Distinguish an omitted property from an explicit reset to null.
fn nullable<'de, D, T>(decoder: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(decoder).map(Some)
}
#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase", deny_unknown_fields)]
pub struct StylePatch {
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    #[serde(deserialize_with = "nullable")]
    pub color: Option<Option<String>>,
    #[serde(deserialize_with = "nullable")]
    pub fill: Option<Option<String>>,
    #[serde(deserialize_with = "nullable")]
    pub align: Option<Option<String>>,
    #[serde(deserialize_with = "nullable")]
    pub number_format: Option<Option<String>>,
}
impl StylePatch {
    fn validate(&self) -> Result<()> {
        for color in [&self.color, &self.fill].into_iter().flatten().flatten() {
            ensure!(
                color.len() == 7
                    && color.starts_with('#')
                    && color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit),
                "expected #RRGGBB color"
            );
        }
        if let Some(Some(a)) = &self.align {
            ensure!(
                matches!(a.as_str(), "left" | "center" | "right"),
                "invalid alignment"
            );
        }
        if let Some(Some(f)) = &self.number_format {
            ensure!(f.len() <= 255, "number format too long");
            ensure_text(f)?;
        }
        Ok(())
    }
    fn apply(&self, s: &mut Style) {
        if let Some(b) = self.bold {
            s.bold = b;
        }
        if let Some(b) = self.italic {
            s.italic = b;
        }
        if let Some(c) = &self.color {
            s.color = c.as_ref().map(|s| s.to_ascii_uppercase());
        }
        if let Some(c) = &self.fill {
            s.fill = c.as_ref().map(|s| s.to_ascii_uppercase());
        }
        if let Some(a) = &self.align {
            s.align = a.clone();
        }
        if let Some(f) = &self.number_format {
            s.number_format = f.clone().filter(|f| f != "General");
        }
    }
}
pub fn format_range(
    wb: &mut Workbook,
    sheet: &str,
    range: &str,
    style: &StylePatch,
    merge: Option<bool>,
) -> Result<()> {
    let i = wb.sheet_index(sheet)?;
    let range = Range::parse(range)?;
    style.validate()?;
    ensure!(
        range.size() <= MAX_SELECTION,
        "selection is too large to format"
    );
    let s = &mut wb.sheets[i];
    if let Some(merge) = merge {
        if merge {
            ensure!(range.size() > 1, "select more than one cell to merge");
            ensure!(
                !s.merges
                    .iter()
                    .filter_map(|m| Range::parse(m).ok())
                    .any(|m| m != range && m.overlaps(range)),
                "merge overlaps another merged area"
            );
            ensure!(
                !range
                    .iter()
                    .filter(|c| *c != range.start)
                    .any(|c| s.cells.get(&c.a1()).is_some_and(|c| !c.raw.is_empty())),
                "merge would hide non-empty cells"
            );
            if !s.merges.contains(&range.a1()) {
                s.merges.push(range.a1());
            }
        } else {
            s.merges
                .retain(|m| !Range::parse(m).is_ok_and(|m| m.overlaps(range)));
        }
    }
    if style.bold.is_some()
        || style.italic.is_some()
        || style.color.is_some()
        || style.fill.is_some()
        || style.align.is_some()
        || style.number_format.is_some()
    {
        for a in range.iter() {
            style.apply(&mut s.cells.entry(a.a1()).or_default().style);
        }
    }
    recalculate(wb);
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DimensionOp {
    InsertRows {
        at: u32,
        #[serde(default = "one")]
        count: u32,
    },
    DeleteRows {
        at: u32,
        #[serde(default = "one")]
        count: u32,
    },
    InsertColumns {
        at: u32,
        #[serde(default = "one")]
        count: u32,
    },
    DeleteColumns {
        at: u32,
        #[serde(default = "one")]
        count: u32,
    },
    SetWidth {
        column: u32,
        width: f64,
    },
    FreezeFirstRow {
        frozen: bool,
    },
}
fn one() -> u32 {
    1
}
fn rewrite_book(wb: &mut Workbook, target: &str, edit: RefEdit<'_>) {
    for s in &mut wb.sheets {
        for c in s.cells.values_mut() {
            if let Some(f) = &mut c.formula {
                let updated = rewrite(f, &s.name, target, edit);
                if updated != *f {
                    c.raw = format!("={updated}");
                    *f = updated;
                }
            }
        }
    }
}
fn moved_axis(p: u32, at: u32, count: u32, insert: bool) -> Option<u32> {
    if p < at {
        Some(p)
    } else if insert {
        Some(p + count)
    } else if p < at + count {
        None
    } else {
        Some(p - count)
    }
}
fn moved_merge(mut r: Range, rows: bool, at: u32, count: u32, insert: bool) -> Option<Range> {
    let (a, b) = if rows {
        (&mut r.start.row, &mut r.end.row)
    } else {
        (&mut r.start.col, &mut r.end.col)
    };
    if insert {
        *a = moved_axis(*a, at, count, true)?;
        *b = moved_axis(*b, at, count, true)?;
    } else {
        let end = at + count - 1;
        if *a >= at && *b <= end {
            return None;
        }
        *a = moved_axis(*a, at, count, false).unwrap_or(at);
        *b = moved_axis(*b, at, count, false).unwrap_or(at - 1);
    }
    (r.size() > 1).then_some(r)
}
pub fn manage_dimensions(wb: &mut Workbook, sheet: &str, op: DimensionOp) -> Result<()> {
    let i = wb.sheet_index(sheet)?;
    let (rows, insert, at, count) = match op {
        DimensionOp::InsertRows { at, count } => (true, true, at, count),
        DimensionOp::DeleteRows { at, count } => (true, false, at, count),
        DimensionOp::InsertColumns { at, count } => (false, true, at, count),
        DimensionOp::DeleteColumns { at, count } => (false, false, at, count),
        DimensionOp::SetWidth { column, width } => {
            return set_width(&mut wb.sheets[i], column, width);
        }
        DimensionOp::FreezeFirstRow { frozen } => {
            wb.sheets[i].frozen = frozen;
            return Ok(());
        }
    };
    let max = if rows { ROWS } else { COLS };
    ensure!(
        (1..=max).contains(&at) && (1..=max - at + 1).contains(&count),
        "invalid insert/delete range"
    );
    let axis = |c: Coord| if rows { c.row } else { c.col };
    let s = &wb.sheets[i];
    if insert {
        ensure!(
            !s.cells
                .keys()
                .filter_map(|a| Coord::parse(a).ok())
                .any(|a| axis(a) >= at && axis(a) > max - count),
            "would move cells beyond Excel limits"
        );
        ensure!(
            !s.merges
                .iter()
                .filter_map(|a| Range::parse(a).ok())
                .any(|r| axis(r.end) >= at && axis(r.end) > max - count),
            "would move merge beyond Excel limits"
        );
        ensure!(
            rows || !s.widths.keys().any(|&c| c >= at && c > max - count),
            "would move column beyond Excel limits"
        );
    }
    let name = s.name.clone();
    rewrite_book(
        wb,
        &name,
        RefEdit::Shift {
            rows,
            at,
            count,
            insert,
        },
    );
    let s = &mut wb.sheets[i];
    s.cells = std::mem::take(&mut s.cells)
        .into_iter()
        .filter_map(|(a, c)| {
            let mut p = Coord::parse(&a).ok()?;
            let n = moved_axis(axis(p), at, count, insert)?;
            if rows {
                p.row = n;
            } else {
                p.col = n;
            }
            Some((p.a1(), c))
        })
        .collect();
    s.merges = std::mem::take(&mut s.merges)
        .into_iter()
        .filter_map(|m| moved_merge(Range::parse(&m).ok()?, rows, at, count, insert).map(Range::a1))
        .collect();
    if !rows {
        s.widths = std::mem::take(&mut s.widths)
            .into_iter()
            .filter_map(|(c, w)| moved_axis(c, at, count, insert).map(|new| (new, w)))
            .collect();
    }
    recalculate(wb);
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum SheetOp {
    New { name: String },
    Rename { sheet: String, name: String },
    Copy { sheet: String, name: String },
    Delete { sheet: String },
}
pub fn manage_sheets(wb: &mut Workbook, op: SheetOp) -> Result<()> {
    let unique = |wb: &Workbook, name: &str, ignore: Option<usize>| {
        validate_name(name)?;
        ensure!(
            wb.sheet_index(name).ok().is_none_or(|i| Some(i) == ignore),
            "worksheet already exists"
        );
        Ok::<_, anyhow::Error>(())
    };
    match op {
        SheetOp::New { name } => {
            unique(wb, &name, None)?;
            wb.sheets.push(Sheet {
                name,
                ..Sheet::default()
            });
        }
        SheetOp::Rename { sheet, name } => {
            let i = wb.sheet_index(&sheet)?;
            unique(wb, &name, Some(i))?;
            let old = wb.sheets[i].name.clone();
            rewrite_book(wb, &old, RefEdit::Rename(&name));
            wb.sheets[i].name = name;
        }
        SheetOp::Copy { sheet, name } => {
            let i = wb.sheet_index(&sheet)?;
            unique(wb, &name, None)?;
            let mut s = wb.sheets[i].clone();
            s.name = name;
            wb.sheets.insert(i + 1, s);
        }
        SheetOp::Delete { sheet } => {
            if wb.sheets.len() <= 1 {
                bail!("cannot delete the last worksheet");
            }
            let i = wb.sheet_index(&sheet)?;
            let name = wb.sheets.remove(i).name;
            rewrite_book(wb, &name, RefEdit::DeleteSheet);
        }
    }
    recalculate(wb);
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    Write {
        sheet: String,
        cells: BTreeMap<String, Input>,
    },
    Format {
        sheet: String,
        range: String,
        #[serde(default)]
        style: StylePatch,
        merge: Option<bool>,
    },
    Dimensions {
        sheet: String,
        change: DimensionOp,
    },
    Sheets {
        change: SheetOp,
    },
}
pub fn apply(wb: &mut Workbook, op: Operation) -> Result<()> {
    match op {
        Operation::Write { sheet, cells } => write_cells(wb, &sheet, cells).map(|_| ()),
        Operation::Format {
            sheet,
            range,
            style,
            merge,
        } => format_range(wb, &sheet, &range, &style, merge),
        Operation::Dimensions { sheet, change } => manage_dimensions(wb, &sheet, change),
        Operation::Sheets { change } => manage_sheets(wb, change),
    }
}
