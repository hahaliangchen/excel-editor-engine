use crate::{
    codec,
    formula::recalculate,
    model::{Cell, Coord, Range, Workbook},
    ops::{self, DimensionOp, Input, SheetOp, StylePatch},
};
use anyhow::{Context, Result, anyhow, bail, ensure};
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Read, Write},
    path::{Component, Path},
    sync::atomic::{AtomicU64, Ordering},
};

const MAX_IN: u64 = 1024 * 1024 + 1;
const MAX_OUT: usize = 200 * 1024;
const PAGE_BUDGET: usize = MAX_OUT - 2048;
const NAMES: [&str; 9] = [
    "excel__inspect",
    "excel__read_range",
    "excel__write_cells",
    "excel__manage_rows_cols",
    "excel__manage_sheets",
    "excel__format_range",
    "excel__search",
    "excel__create_xlsx",
    "excel__focus_view",
];

struct Workspace {
    root: std::path::PathBuf,
    dir: Dir,
}
impl Workspace {
    fn open(root: &str) -> Result<Self> {
        ensure!(
            Path::new(root).is_absolute(),
            "workspace_root must be absolute"
        );
        let canonical = std::fs::canonicalize(root).context("open workspace")?;
        Ok(Self {
            dir: Dir::open_ambient_dir(&canonical, ambient_authority()).context("open workspace")?,
            root: canonical,
        })
    }
    fn path(&self, path: &str) -> Result<(Dir, String)> {
        ensure!(
            !path.is_empty() && !path.contains('\0'),
            "expected a workspace-relative .xlsx path"
        );
        #[cfg(not(windows))]
        ensure!(
            !path.contains(['\\', ':']),
            "expected a workspace-relative .xlsx path"
        );
        #[cfg(windows)]
        let cleaned = path
            .strip_prefix(r"\\?\")
            .or_else(|| path.strip_prefix("//?/"))
            .unwrap_or(path)
            .replace('\\', "/");
        #[cfg(not(windows))]
        let cleaned = path.to_owned();
        let candidate = Path::new(&cleaned);
        let rel_owned: std::path::PathBuf;
        let rel = if candidate.is_absolute() {
            let root_str = self.root.to_string_lossy();
            let clean_root = root_str
                .strip_prefix(r"\\?\")
                .or_else(|| root_str.strip_prefix("//?/"))
                .unwrap_or(&root_str)
                .replace('\\', "/");
            #[cfg(windows)]
            let stripped = cleaned
                .to_ascii_lowercase()
                .strip_prefix(&clean_root.to_ascii_lowercase())
                .map(|_| cleaned[clean_root.len()..].trim_start_matches('/'));
            #[cfg(not(windows))]
            let stripped = cleaned
                .strip_prefix(&clean_root)
                .map(|rest| rest.trim_start_matches('/'));
            let rest = stripped.context("path must be relative and stay within workspace")?;
            rel_owned = std::path::PathBuf::from(rest);
            rel_owned.as_path()
        } else {
            #[cfg(windows)]
            ensure!(
                !cleaned.contains(':'),
                "expected a workspace-relative .xlsx path"
            );
            candidate
        };
        ensure!(
            rel.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("xlsx")),
            "expected a workspace-relative .xlsx path"
        );
        let mut dir = self.dir.try_clone()?;
        let mut parts = rel.components().peekable();
        while let Some(part) = parts.next() {
            match part {
                Component::Normal(name) if parts.peek().is_none() => {
                    return Ok((dir, name.to_string_lossy().into_owned()));
                }
                Component::Normal(name) => {
                    dir = dir
                        .open_dir_nofollow(name)
                        .context("workspace directory missing or symlink")?
                }
                _ => bail!("path must be relative and stay within workspace"),
            }
        }
        bail!("missing workbook path")
    }
    fn read(dir: &Dir, name: &str) -> Result<Workbook> {
        let mut opts = OpenOptions::new();
        opts.read(true).follow(FollowSymlinks::No);
        ensure!(
            dir.symlink_metadata(name)?.is_file(),
            "workbook is not a regular file"
        );
        let f = dir
            .open_with(name, &opts)
            .context("open workbook (symlinks are not supported)")?;
        ensure!(f.metadata()?.is_file(), "workbook is not a regular file");
        let mut bytes = Vec::new();
        f.take(codec::MAX_FILE as u64 + 1).read_to_end(&mut bytes)?;
        codec::decode(&bytes)
    }
    fn save(dir: &Dir, name: &str, wb: &Workbook, overwrite: bool) -> Result<()> {
        if let Ok(meta) = dir.symlink_metadata(name) {
            ensure!(
                overwrite,
                "workbook exists; pass overwrite: true to replace it"
            );
            ensure!(meta.is_file(), "target is not a regular file");
        }
        let bytes = codec::encode(wb)?;
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true).follow(FollowSymlinks::No);
        let (tmp, mut f) = loop {
            let tmp = format!(
                ".excel-{}-{}.tmp",
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            );
            match dir.open_with(&tmp, &opts) {
                Ok(f) => break (tmp, f),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        };
        let result = (|| {
            f.write_all(&bytes)?;
            f.sync_all()?;
            drop(f);
            if overwrite {
                dir.rename(&tmp, dir, name)?;
            } else {
                dir.hard_link(&tmp, dir, name)?;
            }
            Ok(())
        })();
        let _ = dir.remove_file(&tmp);
        result
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Page {
    start_index: u64,
    limit: Option<usize>,
    fragment_offset: usize,
}
#[derive(Deserialize)]
struct Inspect {
    #[serde(flatten)]
    page: Page,
}
#[derive(Deserialize)]
struct ReadRange {
    sheet: String,
    range: String,
    #[serde(flatten)]
    page: Page,
}
#[derive(Deserialize)]
struct WriteCells {
    sheet: String,
    cells: BTreeMap<String, Input>,
}
#[derive(Deserialize)]
struct FormatRange {
    sheet: String,
    range: String,
    #[serde(default)]
    style: StylePatch,
    merge: Option<bool>,
}
#[derive(Deserialize)]
struct Dimensions {
    sheet: String,
    change: DimensionOp,
}
#[derive(Deserialize)]
struct Sheets {
    change: SheetOp,
}
#[derive(Deserialize)]
struct Search {
    sheet: Option<String>,
    query: String,
    #[serde(default)]
    case_sensitive: bool,
    #[serde(flatten)]
    page: Page,
}
#[derive(Deserialize)]
struct Create {
    #[serde(default)]
    overwrite: bool,
    workbook: Workbook,
}
fn parse<T: DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).context("invalid tool arguments")
}
fn page(page: Page, total: u64, mut item: impl FnMut(u64) -> Value) -> Result<Value> {
    ensure!(
        page.start_index <= total,
        "start_index exceeds result count"
    );
    ensure!(
        page.fragment_offset == 0 || page.start_index < total,
        "fragment_offset has no item"
    );
    let limit = page.limit.unwrap_or(100).clamp(1, 1000) as u64;
    let mut items = Vec::new();
    let mut size = 0;
    let mut index = page.start_index;
    let mut fragment = None;
    let mut next_offset = None;
    while index < total && index - page.start_index < limit {
        let v = item(index);
        let raw = serde_json::to_string(&v)?;
        if page.fragment_offset == 0 && size + raw.len() + 1 <= PAGE_BUDGET {
            size += raw.len() + 1;
            items.push(v);
            index += 1;
            continue;
        }
        if !items.is_empty() {
            break;
        }
        let start = page.fragment_offset;
        ensure!(
            start < raw.len() && raw.is_char_boundary(start),
            "invalid fragment_offset"
        );
        let mut end = start;
        let mut cost = 0;
        for c in raw[start..].chars() {
            let n = match c {
                '"' | '\\' | '\n' | '\r' | '\t' => 2,
                c if c < ' ' => 6,
                _ => c.len_utf8(),
            };
            if cost + n > PAGE_BUDGET {
                break;
            }
            cost += n;
            end += c.len_utf8();
        }
        fragment = Some(json!({"index":index,"offset":start,"text":&raw[start..end]}));
        if end < raw.len() {
            next_offset = Some(end);
        } else {
            index += 1;
        }
        break;
    }
    Ok(
        json!({"items":items,"total":total,"start_index":page.start_index,"next_index":(index<total).then_some(index),"fragment":fragment,"next_fragment_offset":next_offset}),
    )
}
fn cell_json(a: &str, c: Option<&Cell>) -> Value {
    match c {
        Some(c) => {
            json!({"address":a,"raw":c.raw,"formula":c.formula,"value":c.value,"display":c.display,"style":c.style,"error":c.error})
        }
        None => json!({"address":a,"raw":"","formula":null,"value":null,"display":""}),
    }
}
fn prefix(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
fn call(name: &str, root: &str, mut args: Value) -> Result<Value> {
    ensure!(args.is_object(), "arguments must be an object");
    if name == "excel__focus_view" {
        let obj = args.as_object().unwrap();
        let has_selector = ["sheet", "range", "column", "row", "find_value"]
            .iter()
            .any(|k| obj.get(*k).is_some_and(|v| !v.is_null()));
        ensure!(
            has_selector,
            "excel__focus_view requires at least one of sheet, range, column, row, or find_value"
        );
        return Ok(json!({
            "focused": true,
            "path": obj.get("path").cloned().unwrap_or(Value::Null),
            "sheet": obj.get("sheet").cloned().unwrap_or(Value::Null),
            "range": obj.get("range").cloned().unwrap_or(Value::Null),
            "column": obj.get("column").cloned().unwrap_or(Value::Null),
            "row": obj.get("row").cloned().unwrap_or(Value::Null),
            "find_column": obj.get("find_column").cloned().unwrap_or(Value::Null),
            "find_value": obj.get("find_value").cloned().unwrap_or(Value::Null),
        }));
    }
    let path = args
        .as_object_mut()
        .and_then(|args| args.remove("path"))
        .and_then(|p| p.as_str().map(str::to_owned))
        .context("path is required")?;
    let (dir, file) = Workspace::open(root)?.path(&path)?;
    if name == "excel__create_xlsx" {
        let mut a: Create = parse(args)?;
        a.workbook.source = None;
        for s in &mut a.workbook.sheets {
            s.source_index = None;
            for c in s.cells.values_mut() {
                codec::ensure_text(&c.raw)?;
                let mut parsed = Cell::from_raw(c.raw.clone());
                parsed.style = c.style.clone();
                *c = parsed;
            }
        }
        a.workbook.validate()?;
        Workspace::save(&dir, &file, &a.workbook, a.overwrite)?;
        return Ok(json!({"path":path,"worksheets":a.workbook.sheets.len()}));
    }
    let mut wb = Workspace::read(&dir, &file)?;
    match name {
        "excel__inspect" => {
            let a: Inspect = parse(args)?;
            page(a.page, wb.sheets.len() as u64, |i| {
                let s = &wb.sheets[i as usize];
                let coords = s.cells.keys().filter_map(|a| Coord::parse(a).ok());
                let (rows, cols) = coords.fold((0, 0), |(r, c), a| (r.max(a.row), c.max(a.col)));
                let preview = (1..=rows.min(3))
                    .map(|row| {
                        (1..=cols.min(24))
                            .map(|col| {
                                s.cells
                                    .get(&Coord { row, col }.a1())
                                    .map_or("", |c| prefix(&c.display, 160))
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                json!({"sheet":s.name,"rows":rows,"columns":cols,"merges":s.merges.len(),"formulas":s.cells.values().filter(|c|c.formula.is_some()).count(),"preview":preview})
            })
        }
        "excel__read_range" => {
            let a: ReadRange = parse(args)?;
            let r = Range::parse(&a.range)?;
            let s = &wb.sheets[wb.sheet_index(&a.sheet)?];
            let width = u64::from(r.end.col - r.start.col + 1);
            page(a.page, r.size(), |i| {
                let pos = Coord {
                    row: r.start.row + (i / width) as u32,
                    col: r.start.col + (i % width) as u32,
                }
                .a1();
                cell_json(&pos, s.cells.get(&pos))
            })
        }
        "excel__search" => {
            let a: Search = parse(args)?;
            ensure!(!a.query.is_empty(), "query is empty");
            let filter = a.sheet.as_ref().map(|s| wb.sheet_index(s)).transpose()?;
            let needle = if a.case_sensitive {
                a.query
            } else {
                a.query.to_lowercase()
            };
            let matches = |s: &str| {
                if a.case_sensitive {
                    s.contains(&needle)
                } else {
                    s.to_lowercase().contains(&needle)
                }
            };
            let mut hits = Vec::new();
            for (i, s) in wb.sheets.iter().enumerate() {
                if filter.is_some_and(|v| v != i) {
                    continue;
                }
                let mut entries = s
                    .cells
                    .iter()
                    .filter(|(_, c)| {
                        matches(&c.display) || c.formula.as_deref().is_some_and(matches)
                    })
                    .collect::<Vec<_>>();
                entries.sort_by_key(|(a, _)| Coord::parse(a).ok());
                hits.extend(entries.into_iter().map(|(a, c)| (&s.name, a, c)));
            }
            page(a.page, hits.len() as u64, |i| {
                let (sheet, a, c) = hits[i as usize];
                json!({"sheet":sheet,"address":a,"display":prefix(&c.display,240),"formula":c.formula.as_deref().map(|f|prefix(f,240))})
            })
        }
        "excel__write_cells" => {
            let a: WriteCells = parse(args)?;
            let changed = ops::write_cells(&mut wb, &a.sheet, a.cells)?;
            Workspace::save(&dir, &file, &wb, true)?;
            let i = wb.sheet_index(&a.sheet)?;
            let mut entries: Vec<_> = changed.iter().map(|a| (i, a)).collect();
            entries.extend(
                wb.sheets
                    .iter()
                    .enumerate()
                    .flat_map(|(j, s)| {
                        s.cells
                            .iter()
                            .filter_map(move |(a, c)| (c.formula.is_some()).then_some((j, a)))
                    })
                    .filter(|(j, a)| *j != i || changed.binary_search(a).is_err()),
            );
            let mut result = page(Page::default(), entries.len() as u64, |n| {
                let (j, a) = entries[n as usize];
                let s = &wb.sheets[j];
                let mut cell = cell_json(a, s.cells.get(a));
                cell["sheet"] = json!(s.name);
                cell
            })?;
            result["written"] = json!(changed.len());
            Ok(result)
        }
        "excel__format_range" => {
            let a: FormatRange = parse(args)?;
            ops::format_range(&mut wb, &a.sheet, &a.range, &a.style, a.merge)?;
            Workspace::save(&dir, &file, &wb, true)?;
            Ok(json!({"path":path,"sheet":a.sheet,"range":a.range}))
        }
        "excel__manage_rows_cols" => {
            let a: Dimensions = parse(args)?;
            ops::manage_dimensions(&mut wb, &a.sheet, a.change)?;
            Workspace::save(&dir, &file, &wb, true)?;
            Ok(json!({"path":path,"sheet":a.sheet}))
        }
        "excel__manage_sheets" => {
            let a: Sheets = parse(args)?;
            ops::manage_sheets(&mut wb, a.change)?;
            recalculate(&mut wb);
            Workspace::save(&dir, &file, &wb, true)?;
            Ok(json!({"path":path,"worksheets":wb.sheets.len()}))
        }
        _ => Err(anyhow!("unknown tool")),
    }
}
fn error(id: Value, code: i64, message: impl AsRef<str>) -> Value {
    json!({"jsonrpc":"2.0","id":id,"error":{"code":code,"message":prefix(message.as_ref(),1024)}})
}
fn request(v: Value, expected: Option<&str>) -> Option<Value> {
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    if !v.is_object()
        || v.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || v.get("method").and_then(Value::as_str).is_none()
        || !matches!(id, Value::Number(_) | Value::String(_) | Value::Null)
    {
        return Some(error(Value::Null, -32600, "invalid JSON-RPC request"));
    }
    let params = &v["params"];
    let result = match v["method"].as_str().unwrap_or_default() {
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default();
            if !NAMES.contains(&name) || expected.is_some_and(|e| e != name) {
                error(id, -32602, "unknown or mismatched tool name")
            } else if let Some(root) = params["workspace_root"].as_str() {
                match call(name, root, params["arguments"].clone()) {
                    Ok(result) => json!({"jsonrpc":"2.0","id":id,"result":result}),
                    Err(e) => error(id, -32602, format!("{e:#}")),
                }
            } else {
                error(id, -32602, "workspace_root is required")
            }
        }
        _ => error(id, -32601, "method not found"),
    };
    v.get("id").map(|_| result)
}
pub fn serve(expected: Option<&str>) -> Result<()> {
    let mut input = io::stdin().lock();
    let mut out = io::stdout().lock();
    loop {
        let mut line = Vec::new();
        let read = (&mut input).take(MAX_IN + 1).read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        let too_long = read as u64 > MAX_IN;
        let response = if too_long {
            Some(error(Value::Null, -32600, "request exceeds 1 MiB"))
        } else {
            match serde_json::from_slice(&line) {
                Ok(v) => request(v, expected),
                Err(_) => Some(error(Value::Null, -32700, "parse error")),
            }
        };
        if let Some(response) = response {
            let mut bytes = serde_json::to_vec(&response)?;
            if bytes.len() + 1 > MAX_OUT {
                bytes =
                    serde_json::to_vec(&error(Value::Null, -32600, "response exceeds 200 KiB"))?;
            }
            out.write_all(&bytes)?;
            out.write_all(b"\n")?;
            out.flush()?;
        }
        if too_long {
            break;
        }
    }
    Ok(())
}
