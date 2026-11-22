use crate::{
    formula::{recalculate, refresh_display},
    model::{Cell, Sheet, Style, Value, Workbook},
};
use anyhow::{Context, Result, anyhow, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use quick_xml::{
    Reader, Writer,
    events::{BytesEnd, BytesStart, BytesText, Event},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read, Write},
};
use umya_spreadsheet as x;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const MAX_FILE: usize = 32 * 1024 * 1024;
const MAX_EXPANDED: u64 = 192 * 1024 * 1024;

fn open(bytes: &[u8]) -> Result<x::Spreadsheet> {
    ensure!(bytes.len() <= MAX_FILE, "workbook exceeds 32 MiB");
    let archive = ZipArchive::new(Cursor::new(bytes)).context("not a valid .xlsx ZIP")?;
    let expanded = archive.decompressed_size().context("missing ZIP sizes")?;
    ensure!(
        archive.len() <= 4096 && expanded <= u128::from(MAX_EXPANDED),
        "workbook expands beyond allowed size"
    );
    std::panic::catch_unwind(|| x::reader::xlsx::read_reader(Cursor::new(bytes), true))
        .map_err(|_| anyhow!("invalid OOXML workbook"))?
        .context("read .xlsx workbook")
}

fn color(c: &x::Color, theme: &x::structs::drawing::Theme) -> Option<String> {
    let s = c.get_argb_with_theme(theme);
    let rgb = s.get(s.len().saturating_sub(6)..)?;
    (rgb.len() == 6 && rgb.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("#{}", rgb.to_ascii_uppercase()))
}
fn project(ws: &x::Worksheet, theme: &x::structs::drawing::Theme, index: usize) -> Sheet {
    use x::CellRawValue as R;
    use x::HorizontalAlignmentValues as A;
    let mut sheet = Sheet {
        name: ws.get_name().into(),
        source_index: Some(index),
        ..Sheet::default()
    };
    for c in ws.get_cell_collection() {
        let a = c.get_coordinate().get_coordinate();
        let cv = c.get_cell_value();
        let value = match cv.get_raw_value() {
            R::Empty => Value::Blank,
            R::Numeric(n) if n.is_finite() => Value::Number(*n),
            R::Bool(b) => Value::Bool(*b),
            _ => Value::Text(cv.get_value().into_owned()),
        };
        let formula = cv
            .is_formula()
            .then(|| cv.get_formula().to_owned())
            .filter(|f| !f.is_empty());
        let raw = formula
            .as_ref()
            .map_or_else(|| value.text(), |f| format!("={f}"));
        let s = c.get_style();
        let font = s.get_font();
        let style = Style {
            bold: font.is_some_and(|f| *f.get_bold()),
            italic: font.is_some_and(|f| *f.get_italic()),
            color: font.and_then(|f| color(f.get_color(), theme)),
            fill: s.get_background_color().and_then(|c| color(c, theme)),
            align: s.get_alignment().and_then(|a| match a.get_horizontal() {
                A::Left => Some("left".into()),
                A::Right => Some("right".into()),
                A::Center | A::CenterContinuous => Some("center".into()),
                _ => None,
            }),
            number_format: s
                .get_numbering_format()
                .map(|n| n.get_format_code())
                .filter(|f| !f.is_empty() && *f != "General")
                .map(str::to_owned),
        };
        let mut cell = Cell {
            raw,
            formula,
            value,
            style,
            origin: Some(a.clone()),
            error: cv.is_error(),
            ..Cell::default()
        };
        refresh_display(&mut cell);
        sheet.cells.insert(a, cell);
    }
    sheet.widths = ws
        .get_column_dimensions()
        .iter()
        .map(|c| (*c.get_col_num(), *c.get_width()))
        .collect();
    sheet.merges = ws.get_merge_cells().iter().map(|m| m.get_range()).collect();
    sheet.frozen = ws
        .get_sheets_views()
        .get_sheet_view_list()
        .first()
        .and_then(|v| v.get_pane())
        .is_some_and(|p| {
            *p.get_vertical_split() > 0.
                && matches!(
                    p.get_state(),
                    x::PaneStateValues::Frozen | x::PaneStateValues::FrozenSplit
                )
        });
    sheet
}

pub fn decode(bytes: &[u8]) -> Result<Workbook> {
    let book = open(bytes)?;
    let mut wb = Workbook {
        sheets: book
            .get_sheet_collection()
            .iter()
            .enumerate()
            .map(|(i, s)| project(s, book.get_theme(), i))
            .collect(),
        source: Some(STANDARD.encode(bytes)),
    };
    // Read inline strings and error caches without umya's automatic type inference.
    repair_ooxml_values(bytes, &mut wb.sheets)?;
    wb.validate()?;
    Ok(wb)
}

fn argb(s: &str) -> Option<String> {
    let s = s.trim_start_matches('#');
    (s.len() == 6 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("FF{}", s.to_ascii_uppercase()))
}
fn edit_style(s: &mut x::Style, old: &Style, new: &Style) {
    use x::HorizontalAlignmentValues as A;
    if old.bold != new.bold {
        s.get_font_mut().set_bold(new.bold);
    }
    if old.italic != new.italic {
        s.get_font_mut().set_italic(new.italic);
    }
    if old.color != new.color {
        if let Some(c) = new.color.as_deref().and_then(argb) {
            *s.get_font_mut().get_color_mut() = x::Color::default();
            s.get_font_mut().get_color_mut().set_argb(c);
        } else {
            *s.get_font_mut().get_color_mut() = x::Color::default();
        }
    }
    if old.fill != new.fill {
        if let Some(c) = new.fill.as_deref().and_then(argb) {
            s.set_background_color_solid(c);
        } else {
            s.set_fill(x::Fill::default());
        }
    }
    if old.align != new.align {
        s.get_alignment_mut()
            .set_horizontal(match new.align.as_deref() {
                Some("left") => A::Left,
                Some("center") => A::Center,
                Some("right") => A::Right,
                _ => A::General,
            });
    }
    if old.number_format != new.number_format {
        s.get_numbering_format_mut()
            .set_format_code(new.number_format.as_deref().unwrap_or("General"));
    }
}
fn edit_cell(c: &mut x::Cell, old: Option<&Cell>, new: &Cell) {
    let style = Style::default();
    edit_style(
        c.get_style_mut(),
        old.map_or(&style, |c| &c.style),
        &new.style,
    );
    let v = c.get_cell_value_mut();
    if let Some(f) = &new.formula {
        if !old.is_some_and(|o| o.formula == new.formula) {
            v.set_formula(f.trim_start_matches('='));
        }
        // The final XML pass writes explicit OOXML cache types (including string and error).
        v.set_formula_result_default("0");
    } else if !old.is_some_and(|o| {
        o.formula.is_none() && o.raw == new.raw && o.value == new.value && o.error == new.error
    }) || matches!(
        (&new.value, v.get_raw_value()),
        (
            Value::Text(_),
            x::CellRawValue::Empty | x::CellRawValue::Numeric(_) | x::CellRawValue::Bool(_)
        )
    ) {
        match &new.value {
            Value::Blank => {
                v.set_blank();
            }
            Value::Number(n) => {
                v.set_value_number(*n);
            }
            Value::Bool(b) => {
                v.set_value_bool(*b);
            }
            Value::Text(t) if new.error => {
                v.set_error(t);
                v.remove_formula();
            }
            Value::Text(t) => {
                v.set_value_string(t);
            }
        }
    }
}

pub fn encode(input: &Workbook) -> Result<Vec<u8>> {
    input.validate()?;
    let mut wb = input.clone();
    recalculate(&mut wb);
    let original = wb.source.as_ref().map(|s| STANDARD.decode(s)).transpose()?;
    let mut book = match &original {
        Some(bytes) => open(bytes)?,
        None => x::new_file(),
    };
    let mut old: Vec<_> = if original.is_some() {
        book.get_sheet_collection()
            .iter()
            .enumerate()
            .map(|(i, s)| project(s, book.get_theme(), i))
            .collect()
    } else {
        Vec::new()
    };
    if let Some(bytes) = &original {
        repair_ooxml_values(bytes, &mut old)?;
        if let Some(out) = overlay_values(bytes, &old, &wb)? {
            return Ok(out);
        }
    }
    let original_sheets = book.get_sheet_collection().to_vec();
    book.get_sheet_collection_mut().clear();
    let mut occupied = BTreeSet::new();
    for (i, s) in wb.sheets.iter().enumerate() {
        let index = s
            .source_index
            .filter(|idx| original.is_some() && *idx < old.len());
        let previous = index.and_then(|idx| old.get(idx));
        let template = index.and_then(|idx| original_sheets.get(idx));
        let ws = if let Some(template) =
            template.filter(|_| index.is_some_and(|idx| occupied.insert(idx)))
        {
            let mut ws = template.clone();
            ws.set_name(&s.name);
            book.add_sheet(ws).map_err(|e| anyhow!(e))?
        } else {
            let ws = book.new_sheet(&s.name).map_err(|e| anyhow!(e))?;
            if let Some(template) = template {
                // Duplicate sheets use a fresh sheet identity; copy supported cell data and styles.
                for c in template.get_cell_collection() {
                    *ws.get_cell_mut(c.get_coordinate().get_coordinate()) = c.clone();
                }
            }
            ws
        };
        let relocated: BTreeMap<_, _> = s
            .cells
            .iter()
            .filter_map(|(a, c)| {
                c.origin
                    .as_ref()
                    .filter(|o| *o != a)
                    .and_then(|o| template?.get_cell(o.as_str()))
                    .map(|c| (a, c.clone()))
            })
            .collect();
        if let Some(old) = previous {
            for a in old.cells.keys() {
                if !s.cells.contains_key(a) {
                    ws.remove_cell(a.as_str());
                }
            }
        }
        for (a, new) in &s.cells {
            if let Some(original) = relocated.get(a) {
                let mut c = original.clone();
                c.get_coordinate_mut().set_coordinate(a);
                *ws.get_cell_mut(a.as_str()) = c;
            }
            let old_cell = previous.and_then(|sheet| {
                new.origin
                    .as_ref()
                    .and_then(|o| sheet.cells.get(o))
                    .or_else(|| sheet.cells.get(a))
            });
            edit_cell(ws.get_cell_mut(a.as_str()), old_cell, new);
        }
        let is_copy =
            index.is_some_and(|idx| wb.sheets[..i].iter().any(|s| s.source_index == Some(idx)));
        if is_copy || previous.is_none_or(|p| p.merges != s.merges) {
            ws.get_merge_cells_mut().clear();
            for m in &s.merges {
                ws.add_merge_cells(m);
            }
        }
        // get_cell_mut materializes default column definitions, even with no width edit.
        ws.get_column_dimensions_mut()
            .retain(|c| s.widths.contains_key(c.get_col_num()));
        for (c, w) in &s.widths {
            if is_copy || previous.and_then(|p| p.widths.get(c)) != Some(w) {
                ws.get_column_dimension_by_number_mut(c).set_width(*w);
            }
        }
        if s.frozen != previous.is_some_and(|p| p.frozen) || (is_copy && s.frozen) {
            let views = ws.get_sheet_views_mut().get_sheet_view_list_mut();
            if s.frozen {
                if views.is_empty() {
                    views.push(x::SheetView::default());
                }
                let mut pane = x::Pane::default();
                pane.set_state(x::PaneStateValues::Frozen)
                    .set_active_pane(x::PaneValues::BottomLeft)
                    .set_vertical_split(1.);
                pane.get_top_left_cell_mut().set_coordinate("A2");
                views[0].set_pane(pane);
            } else if let Some(view) = views.first_mut() {
                *view = x::SheetView::default();
            }
        }
    }
    book.get_workbook_view_mut().set_active_tab(0);
    let mut bytes = Vec::new();
    x::writer::xlsx::write_writer(&book, &mut bytes)?;
    let paths = sheet_paths(&bytes)?;
    let mut updates = BTreeMap::new();
    for (path, sheet) in paths.into_iter().zip(&wb.sheets) {
        updates.insert(
            path,
            sheet
                .cells
                .iter()
                .filter(|(_, c)| c.formula.is_some() || c.error)
                .map(|(a, c)| (a.clone(), (c, Patch::Cache)))
                .collect(),
        );
    }
    patch_zip(&bytes, &updates)
}

// Resolve sheet parts via OOXML relationships, not via order or a guessed sheet filename.
fn attr(e: &BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes()
        .with_checks(false)
        .flatten()
        .find(|a| a.key.as_ref() == key)
        .and_then(|a| a.unescape_value().ok())
        .map(|s| s.into_owned())
}
fn xml_part(archive: &mut ZipArchive<Cursor<&[u8]>>, path: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    archive.by_name(path)?.read_to_end(&mut out)?;
    Ok(out)
}
fn sheet_paths(bytes: &[u8]) -> Result<Vec<String>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let rel = xml_part(&mut archive, "xl/_rels/workbook.xml.rels")?;
    let xml = xml_part(&mut archive, "xl/workbook.xml")?;
    let mut rels = BTreeMap::new();
    let mut reader = Reader::from_reader(rel.as_slice());
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"Relationship" => {
                if let (Some(id), Some(target), Some(kind)) =
                    (attr(&e, b"Id"), attr(&e, b"Target"), attr(&e, b"Type"))
                    && kind.ends_with("/worksheet")
                {
                    let path = if target.starts_with('/') {
                        target.trim_start_matches('/').into()
                    } else {
                        format!("xl/{target}")
                    };
                    rels.insert(id, path);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut paths = Vec::new();
    loop {
        match reader.read_event()? {
            Event::Start(e) | Event::Empty(e) if e.local_name().as_ref() == b"sheet" => {
                if let Some(path) = attr(&e, b"r:id").and_then(|id| rels.remove(&id)) {
                    paths.push(path);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(paths)
}
fn repair_ooxml_values(bytes: &[u8], sheets: &mut [Sheet]) -> Result<()> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    for (path, sheet) in sheet_paths(bytes)?.into_iter().zip(sheets) {
        let xml = xml_part(&mut archive, &path)?;
        let mut r = Reader::from_reader(xml.as_slice());
        let mut pending = None;
        loop {
            match r.read_event()? {
                Event::Start(e) if e.local_name().as_ref() == b"c" => {
                    pending = attr(&e, b"r")
                        .zip(attr(&e, b"t"))
                        .filter(|(_, t)| matches!(t.as_str(), "e" | "inlineStr"));
                }
                Event::Start(e)
                    if e.local_name().as_ref() == b"v"
                        && pending.as_ref().is_some_and(|(_, t)| t == "e") =>
                {
                    let raw = r.read_text(e.name())?;
                    let value = quick_xml::escape::unescape(&raw)?.into_owned();
                    if let Some((a, _)) = pending.take()
                        && let Some(c) = sheet.cells.get_mut(&a)
                    {
                        c.error = true;
                        if c.formula.is_none() {
                            c.raw = value.clone();
                        }
                        c.value = Value::Text(value);
                        refresh_display(c);
                    }
                }
                Event::Start(e)
                    if e.local_name().as_ref() == b"is"
                        && pending.as_ref().is_some_and(|(_, t)| t == "inlineStr") =>
                {
                    let end = e.name();
                    let mut text = String::new();
                    loop {
                        match r.read_event()? {
                            Event::Start(e) if e.local_name().as_ref() == b"rPh" => {
                                r.read_to_end(e.name())?;
                            }
                            Event::Start(e) if e.local_name().as_ref() == b"t" => {
                                let raw = r.read_text(e.name())?;
                                text.push_str(&quick_xml::escape::unescape(&raw)?);
                            }
                            Event::End(e) if e.name() == end => break,
                            Event::Eof => return Err(anyhow!("unterminated inline string")),
                            _ => {}
                        }
                    }
                    if let Some((a, _)) = pending.take()
                        && let Some(c) = sheet.cells.get_mut(&a)
                    {
                        c.error = false;
                        c.raw = text.clone();
                        c.value = Value::Text(text);
                        refresh_display(c);
                    }
                }
                Event::End(e) if e.local_name().as_ref() == b"c" => pending = None,
                Event::Eof => break,
                _ => {}
            }
        }
    }
    Ok(())
}
#[derive(Clone, Copy)]
enum Patch {
    Cache,
    Replace,
}
type Updates<'a> = BTreeMap<String, BTreeMap<String, (&'a Cell, Patch)>>;

// For value-only edits at existing addresses, retain every original package part and worksheet
// element except the edited cell payloads and caches. This also retains unsupported extensions.
fn overlay_values(bytes: &[u8], original: &[Sheet], wb: &Workbook) -> Result<Option<Vec<u8>>> {
    if original.len() != wb.sheets.len() {
        return Ok(None);
    }
    let paths = sheet_paths(bytes)?;
    if paths.len() != original.len() {
        return Ok(None);
    }
    let mut updates = Updates::new();
    for (i, ((old, new), p)) in original.iter().zip(&wb.sheets).zip(paths).enumerate() {
        if new.source_index != Some(i)
            || old.name != new.name
            || old.merges != new.merges
            || old.widths != new.widths
            || old.frozen != new.frozen
            || !old.cells.keys().eq(new.cells.keys())
        {
            return Ok(None);
        }
        let mut patch = BTreeMap::new();
        for (a, c) in &new.cells {
            let before = &old.cells[a];
            if c.style != before.style || c.origin.as_ref() != Some(a) {
                return Ok(None);
            }
            if c.raw != before.raw
                || c.formula != before.formula
                || (c.formula.is_none() && (c.value != before.value || c.error != before.error))
            {
                patch.insert(a.clone(), (c, Patch::Replace));
            } else if c.formula.is_some() && (c.value != before.value || c.error != before.error) {
                patch.insert(a.clone(), (c, Patch::Cache));
            }
        }
        if !patch.is_empty() {
            updates.insert(p, patch);
        }
    }
    Ok(Some(patch_zip(bytes, &updates)?))
}
fn patch_zip(bytes: &[u8], updates: &Updates<'_>) -> Result<Vec<u8>> {
    if updates.values().all(BTreeMap::is_empty) {
        return Ok(bytes.to_vec());
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut f = archive.by_index(i)?;
        let path = f.name().to_owned();
        if let Some(patch) = updates.get(&path).filter(|p| !p.is_empty()) {
            let mut xml = Vec::new();
            f.read_to_end(&mut xml)?;
            out.start_file(
                path,
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )?;
            out.write_all(&patch_xml(&xml, patch)?)?;
        } else {
            out.raw_copy_file(f)?;
        }
    }
    Ok(out.finish()?.into_inner())
}
fn write_tag(w: &mut Writer<Vec<u8>>, q: &str, text: &str) -> Result<()> {
    w.write_event(Event::Start(BytesStart::new(q)))?;
    w.write_event(Event::Text(BytesText::new(text)))?;
    w.write_event(Event::End(BytesEnd::new(q)))?;
    Ok(())
}
fn patch_xml(xml: &[u8], updates: &BTreeMap<String, (&Cell, Patch)>) -> Result<Vec<u8>> {
    let mut r = Reader::from_reader(xml);
    let mut w = Writer::new(Vec::with_capacity(xml.len()));
    let mut remaining = updates.len();
    loop {
        let event = r.read_event()?;
        match event {
            Event::Start(e) | Event::Empty(e)
                if e.local_name().as_ref() == b"c"
                    && attr(&e, b"r").is_some_and(|a| updates.contains_key(&a)) =>
            {
                let empty = xml.get(r.buffer_position() as usize - 2) == Some(&b'/');
                let (cell, mode) = updates[&attr(&e, b"r").context("cell address missing")?];
                remaining -= 1;
                let q = e.name();
                let q = std::str::from_utf8(q.as_ref())?;
                let prefix = q.strip_suffix('c').unwrap_or("");
                let vq = format!("{prefix}v");
                let fq = format!("{prefix}f");
                let mut start = BytesStart::new(q);
                for a in e.attributes().flatten().filter(|a| a.key.as_ref() != b"t") {
                    start.push_attribute(a);
                }
                let ty = match &cell.value {
                    Value::Number(_) | Value::Blank => None,
                    Value::Bool(_) => Some("b"),
                    Value::Text(_) if cell.error => Some("e"),
                    Value::Text(_) if cell.formula.is_some() => Some("str"),
                    Value::Text(_) => Some("inlineStr"),
                };
                if let Some(ty) = ty {
                    start.push_attribute(("t", ty));
                }
                w.write_event(Event::Start(start))?;
                let mut wrote_cache = false;
                if matches!(mode, Patch::Replace)
                    && let Some(f) = &cell.formula
                {
                    write_tag(&mut w, &fq, f.trim_start_matches('='))?;
                }
                if !empty {
                    loop {
                        match r.read_event()? {
                            Event::Start(inner)
                                if (matches!(inner.local_name().as_ref(), b"v" | b"is")
                                    || matches!(mode, Patch::Replace)
                                        && inner.local_name().as_ref() == b"f") =>
                            {
                                r.read_to_end(inner.name())?;
                            }
                            Event::Empty(inner)
                                if (matches!(inner.local_name().as_ref(), b"v" | b"is")
                                    || matches!(mode, Patch::Replace)
                                        && inner.local_name().as_ref() == b"f") => {}
                            Event::Start(inner) if inner.local_name().as_ref() == b"extLst" => {
                                write_cache(&mut w, cell, ty, prefix, &vq)?;
                                wrote_cache = true;
                                w.write_event(Event::Start(inner))?;
                            }
                            Event::End(end) if end.name().as_ref() == q.as_bytes() => break,
                            Event::Eof => return Err(anyhow!("unterminated cell")),
                            other => w.write_event(other)?,
                        }
                    }
                }
                if !wrote_cache {
                    write_cache(&mut w, cell, ty, prefix, &vq)?;
                }
                w.write_event(Event::End(BytesEnd::new(q)))?;
            }
            Event::Eof => break,
            other => w.write_event(other)?,
        }
    }
    ensure!(remaining == 0, "worksheet cell missing from export");
    Ok(w.into_inner())
}
fn write_cache(
    w: &mut Writer<Vec<u8>>,
    cell: &Cell,
    ty: Option<&str>,
    prefix: &str,
    vq: &str,
) -> Result<()> {
    match &cell.value {
        Value::Blank if cell.formula.is_some() => write_tag(w, vq, ""),
        Value::Blank => Ok(()),
        Value::Bool(v) => write_tag(w, vq, if *v { "1" } else { "0" }),
        Value::Text(s) if ty == Some("inlineStr") => {
            let iq = format!("{prefix}is");
            let tq = format!("{prefix}t");
            w.write_event(Event::Start(BytesStart::new(iq.as_str())))?;
            let mut tag = BytesStart::new(tq.as_str());
            tag.push_attribute(("xml:space", "preserve"));
            w.write_event(Event::Start(tag))?;
            w.write_event(Event::Text(BytesText::new(s)))?;
            w.write_event(Event::End(BytesEnd::new(tq)))?;
            w.write_event(Event::End(BytesEnd::new(iq)))?;
            Ok(())
        }
        v => write_tag(w, vq, &v.text()),
    }
}

pub fn set_width(sheet: &mut Sheet, column: u32, width: f64) -> Result<()> {
    ensure!(
        (1..=crate::model::COLS).contains(&column)
            && width.is_finite()
            && (0.0..=255.).contains(&width),
        "invalid width"
    );
    sheet.widths.insert(column, width);
    Ok(())
}
pub fn ensure_text(raw: &str) -> Result<()> {
    ensure!(
        raw.encode_utf16().count() <= 32767
            && !raw
                .chars()
                .any(|c| c < ' ' && !matches!(c, '\n' | '\r' | '\t')),
        "cell text exceeds Excel limits or contains illegal characters"
    );
    Ok(())
}
