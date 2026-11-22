use excel_editor_engine::{
    codec,
    formula::recalculate,
    model::{Cell, Sheet, Value, Workbook},
    ops, sample,
    server::bind_address,
};
use serde_json::{Value as Json, json};
use std::{
    io::{Cursor, Read, Write},
    process::{Command, Stdio},
};

fn set(w: &mut Workbook, s: usize, a: &str, r: &str) {
    w.sheets[s].cells.insert(a.into(), Cell::from_raw(r.into()));
}
fn text(w: &Workbook, s: usize, a: &str) -> String {
    w.sheets[s].cells[a].value.text()
}
fn xml(data: &[u8], p: &str) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(data)).unwrap();
    let mut s = String::new();
    archive.by_name(p).unwrap().read_to_string(&mut s).unwrap();
    s
}

#[test]
fn formula_functions_and_precedence() {
    let mut w = Workbook::blank();
    for (a, v) in [
        ("A1", "Alpha"),
        ("A2", "beta"),
        ("A3", "alpha"),
        ("B1", "10"),
        ("B2", "30"),
        ("B3", "20"),
        ("B4", "0"),
    ] {
        set(&mut w, 0, a, v);
    }
    let checks = [
        ("2+3*4^2", "50"),
        ("(2+3)*4", "20"),
        ("-2^2+50%", "4.5"),
        ("SuM(B1:B3)", "60"),
        ("AVERAGE(B1:B3)", "20"),
        ("COUNT(B1:B4,A1:A3)", "4"),
        ("COUNTA(A1:A9)", "3"),
        ("MIN(B1:B3)+MAX(B1:B3)+MEDIAN(B1:B3)", "60"),
        ("COUNTIF(A1:A4,\"al*\")", "2"),
        ("SUMIF(A1:A3,\"ALPHA\",B1:B3)", "30"),
        ("SUMIF(A1:A3,\"ALPHA\",B1)", "30"),
        ("IF(B1<=20,42,1/0)", "42"),
        ("IFS(B1>20,1,B1<>0,2)", "2"),
        ("IFERROR(1/0,99)", "99"),
        ("AND(1,NOT(FALSE),OR(0,TRUE))", "TRUE"),
        ("ROUND(12.456,2)+ROUNDUP(1.21,1)+ROUNDDOWN(2.59,1)", "16.26"),
        ("ABS(-2)+INT(-2.1)+MOD(-3,2)+POWER(2,3)+SQRT(9)", "11"),
        (
            "CONCAT(UPPER(LEFT(A1,2)),LOWER(RIGHT(A2,2)),MID(\"中文词\",2,1))",
            "ALta文",
        ),
        (
            "CONCATENATE(TRIM(\"  a  b  \"),LEN(\"猫咪\"))&\"!\"",
            "a b2!",
        ),
        ("VLOOKUP(\"BETA\",A1:B3,2,FALSE)", "30"),
        ("INDEX(B1:B3,MATCH(\"beta\",A1:A3,0))", "30"),
        ("NO_SUCH_FUNCTION(A1)", "#NAME?"),
        ("1/B4", "#DIV/0!"),
        ("\"#DIV/0!\"", "#DIV/0!"),
    ];
    for (i, (formula, _)) in checks.iter().enumerate() {
        set(&mut w, 0, &format!("C{}", i + 1), &format!("={formula}"));
    }
    recalculate(&mut w);
    for (i, (formula, expected)) in checks.iter().enumerate() {
        let got = text(&w, 0, &format!("C{}", i + 1));
        if formula.starts_with("ROUND(") {
            assert!((got.parse::<f64>().unwrap() - 16.26).abs() < 1e-10);
        } else {
            assert_eq!(&got, expected, "{formula}");
        }
    }
    assert!(w.sheets[0].cells["C23"].error);
    assert!(!w.sheets[0].cells["C24"].error);
}
#[test]
fn dag_handles_cross_sheet_cycles_downstream_and_long_chains() {
    let mut w = Workbook::blank();
    w.sheets.push(Sheet {
        name: "Q1 营收".into(),
        ..Sheet::default()
    });
    set(&mut w, 0, "A1", "=SUM('Q1 营收'!A1:A2)+1");
    set(&mut w, 1, "A1", "=Sheet1!A1");
    set(&mut w, 1, "A2", "3");
    set(&mut w, 0, "A2", "=IFERROR(A1,17)");
    set(&mut w, 0, "A3", "=A1");
    set(&mut w, 0, "A4", "=SUM('Q1 营收'!A2:A1048576)");
    set(&mut w, 0, "A5", "=Missing!A1");
    set(&mut w, 0, "A6", "=A6");
    set(&mut w, 0, "B1", "1");
    for i in 2..=1800 {
        set(&mut w, 0, &format!("B{i}"), &format!("=B{}+1", i - 1));
    }
    recalculate(&mut w);
    for (s, a, v) in [
        (0, "A1", "#CIRC!"),
        (1, "A1", "#CIRC!"),
        (0, "A2", "17"),
        (0, "A3", "#CIRC!"),
        (0, "A4", "3"),
        (0, "A5", "#REF!"),
        (0, "A6", "#CIRC!"),
        (0, "B1800", "1800"),
    ] {
        assert_eq!(text(&w, s, a), v);
    }
}
#[test]
fn xlsx_round_trips_styles_merges_types_and_real_caches() {
    let mut w = sample::workbook();
    set(&mut w, 1, "F1", "=\"0042\"");
    set(&mut w, 1, "F2", "=TRUE");
    set(&mut w, 1, "F3", "=F3");
    set(&mut w, 1, "F4", "=1/0");
    set(&mut w, 1, "F5", "'#CIRC!");
    let data = codec::encode(&w).unwrap();
    let result = codec::decode(&data).unwrap();
    assert_eq!(result.sheets.len(), 2);
    for (i, (a, b)) in w.sheets.iter().zip(&result.sheets).enumerate() {
        assert_eq!(
            (&a.name, &a.merges, &a.widths, a.frozen),
            (&b.name, &b.merges, &b.widths, b.frozen)
        );
        for (coord, c) in &a.cells {
            let d = &b.cells[coord];
            assert_eq!(c.formula, d.formula, "{i} {coord}");
            assert_eq!(c.style.fill, d.style.fill, "{i} {coord}");
            assert_eq!(c.style.number_format, d.style.number_format, "{i} {coord}");
            if i == 0 {
                assert_eq!(c.value, d.value, "{i} {coord}");
            }
        }
    }
    let sheet = xml(&data, "xl/worksheets/sheet2.xml");
    assert!(sheet.contains("<v>458000</v>"));
    assert!(sheet.contains("<v>975000</v>"));
    assert!(sheet.contains("<v>0042</v>"));
    assert!(sheet.contains("<v>#CIRC!</v>"));
    assert_eq!(
        result.sheets[1].cells["F1"].value,
        Value::Text("0042".into())
    );
    assert_eq!(result.sheets[1].cells["F2"].value, Value::Bool(true));
    assert!(result.sheets[1].cells["F3"].error);
    assert!(result.sheets[1].cells["F4"].error);
    assert!(!result.sheets[1].cells["F5"].error);
}
#[test]
fn edited_source_updates_caches_preserving_other_parts() {
    let data = codec::encode(&sample::workbook()).unwrap();
    let mut w = codec::decode(&data).unwrap();
    let details = w.sheets[0].name.clone();
    ops::write_cells(
        &mut w,
        &details,
        serde_json::from_value(json!({"D4":148000})).unwrap(),
    )
    .unwrap();
    assert_eq!(text(&w, 0, "F4"), "76000");
    assert_eq!(text(&w, 1, "B4"), "478000");
    let out = codec::encode(&w).unwrap();
    let res = codec::decode(&out).unwrap();
    assert_eq!(text(&res, 1, "B4"), "478000");
    assert_eq!(text(&res, 1, "B8"), "995000");
    assert_eq!(xml(&data, "xl/styles.xml"), xml(&out, "xl/styles.xml"));
    assert_eq!(xml(&data, "xl/workbook.xml"), xml(&out, "xl/workbook.xml"));
    assert!(xml(&out, "xl/worksheets/sheet2.xml").contains("<v>478000</v>"));
}
#[test]
fn structural_edits_rewrite_refs_and_keep_workbook_valid() {
    let mut w = sample::workbook();
    let details = w.sheets[0].name.clone();
    ops::manage_dimensions(
        &mut w,
        &details,
        ops::DimensionOp::InsertRows { at: 5, count: 2 },
    )
    .unwrap();
    assert!(w.sheets[1].cells["B8"].raw.contains("D4:D14"));
    assert_eq!(text(&w, 1, "B8"), "975000");
    ops::manage_sheets(
        &mut w,
        ops::SheetOp::Rename {
            sheet: details,
            name: "Q1's Data".into(),
        },
    )
    .unwrap();
    assert!(w.sheets[1].cells["B8"].raw.contains("'Q1''s Data'!"));
    let other = w.sheets[1].name.clone();
    ops::manage_sheets(
        &mut w,
        ops::SheetOp::Copy {
            sheet: other,
            name: "Copy".into(),
        },
    )
    .unwrap();
    let round = codec::decode(&codec::encode(&w).unwrap()).unwrap();
    assert_eq!(round.sheets[2].name, "Copy");
    assert_eq!(text(&round, 2, "B8"), "975000");
    ops::manage_sheets(
        &mut w,
        ops::SheetOp::Delete {
            sheet: "Q1's Data".into(),
        },
    )
    .unwrap();
    assert_eq!(text(&w, 1, "B8"), "#REF!");
}
fn rpc(root: &std::path::Path, name: &str, args: Json) -> Json {
    let mut child = Command::new(env!("CARGO_BIN_EXE_excel-server"))
        .arg("--tool")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    writeln!(child.stdin.take().unwrap(),"{}",json!({"jsonrpc":"2.0","method":"tools/call","id":"test","params":{"workspace_root":root,"name":name,"arguments":args}})).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.len() <= 200 * 1024);
    let data: Json = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(data["jsonrpc"], "2.0");
    assert_eq!(data["id"], "test");
    data
}
fn ok(r: Json) -> Json {
    assert!(r.get("error").is_none(), "{r}");
    r["result"].clone()
}
#[test]
fn all_eight_stdio_tools_and_workspace_isolation() {
    let ws = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let name = "data.xlsx";
    let workbook = json!({"sheets":[{"name":"Data","cells":{"A1":{"raw":"14"},"A2":{"raw":"6"}}},{"name":"Summary","cells":{"B1":{"raw":"=SUM(Data!A1:A2)"}}}]});
    let cases = [
        (
            "excel__create_xlsx",
            json!({"path":name,"workbook":workbook}),
        ),
        ("excel__inspect", json!({"path":name})),
        (
            "excel__read_range",
            json!({"path":name,"sheet":"Data","range":"A1:XFD1048576","limit":5}),
        ),
        (
            "excel__write_cells",
            json!({"path":name,"sheet":"Data","cells":{"A1":28}}),
        ),
        (
            "excel__manage_rows_cols",
            json!({"path":name,"sheet":"Data","change":{"action":"insert_rows","at":2}}),
        ),
        (
            "excel__manage_sheets",
            json!({"path":name,"change":{"action":"rename","sheet":"Data","name":"Revenue"}}),
        ),
        (
            "excel__format_range",
            json!({"path":name,"sheet":"Revenue","range":"A1:B1","style":{"bold":true,"fill":"#ABCDEF"},"merge":true}),
        ),
        ("excel__search", json!({"path":name,"query":"SUM"})),
    ];
    std::fs::write(
        external.path().join(name),
        codec::encode(&sample::workbook()).unwrap(),
    )
    .unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(external.path(), ws.path().join("linked")).unwrap();
        std::os::unix::fs::symlink(external.path().join(name), ws.path().join("escape.xlsx"))
            .unwrap();
    }
    for (name, args) in &cases {
        let result = ok(rpc(ws.path(), name, args.clone()));
        match *name {
            "excel__inspect" => assert_eq!(result["total"], 2),
            "excel__read_range" => {
                assert_eq!(result["next_index"], 5);
                assert_eq!(result["items"][0]["raw"], "14");
            }
            "excel__write_cells" => assert!(
                result["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["sheet"] == "Summary" && c["value"].as_f64() == Some(34.0))
            ),
            "excel__search" => assert_eq!(result["items"][0]["formula"], "SUM('Revenue'!A1:A3)"),
            _ => {}
        }
        let mut bad = vec![
            "../data.xlsx".into(),
            external
                .path()
                .join("data.xlsx")
                .to_string_lossy()
                .into_owned(),
        ];
        #[cfg(unix)]
        bad.extend(["linked/data.xlsx".into(), "escape.xlsx".into()]);
        for path in bad {
            let mut bad = args.clone();
            bad["path"] = json!(path);
            bad["overwrite"] = json!(true);
            assert!(rpc(ws.path(), name, bad).get("error").is_some(), "{name}");
        }
    }
    assert!(
        rpc(ws.path(), "excel__create_xlsx", cases[0].1.clone())
            .get("error")
            .is_some()
    );
    let w = codec::decode(&std::fs::read(ws.path().join(name)).unwrap()).unwrap();
    assert_eq!(w.sheets[0].merges, ["A1:B1"]);
    assert!(w.sheets[0].cells["A1"].style.bold);
    assert_eq!(text(&w, 1, "B1"), "34");
    assert_eq!(
        codec::decode(&std::fs::read(external.path().join(name)).unwrap())
            .unwrap()
            .sheets[0]
            .name,
        "季度营收明细"
    );
}
#[test]
fn pagination_uses_wire_bytes_and_resumes_utf8_fragments() {
    let ws = tempfile::tempdir().unwrap();
    let long = "界".repeat(31000);
    let workbook =
        json!({"sheets":[{"name":"Data","cells":{"A1":{"raw":long},"A2":{"raw":"next"}}}]});
    ok(rpc(
        ws.path(),
        "excel__create_xlsx",
        json!({"path":"data.xlsx","workbook":workbook}),
    ));
    let mut args = json!({"path":"data.xlsx","sheet":"Data","range":"A1:A2","limit":1000});
    let mut assembled = String::new();
    loop {
        let r = ok(rpc(ws.path(), "excel__read_range", args.clone()));
        assembled.push_str(r["fragment"]["text"].as_str().unwrap());
        if let Some(n) = r["next_fragment_offset"].as_u64() {
            args["fragment_offset"] = json!(n);
        } else {
            assert_eq!(r["next_index"], 1);
            break;
        }
    }
    let cell: Json = serde_json::from_str(&assembled).unwrap();
    assert_eq!(cell["raw"], long);
    assert_eq!(cell["display"], long);
    args["fragment_offset"] = json!(0);
    args["start_index"] = json!(1);
    assert_eq!(
        ok(rpc(ws.path(), "excel__read_range", args))["items"][0]["value"],
        "next"
    );
}
#[test]
fn quickagent_and_standalone_binding() {
    assert_eq!(
        bind_address(Some("127.0.0.1:49001"), Some("9999"))
            .unwrap()
            .to_string(),
        "127.0.0.1:49001"
    );
    assert_eq!(
        bind_address(None, None).unwrap().to_string(),
        "127.0.0.1:8083"
    );
    assert_eq!(
        bind_address(Some("::1"), Some("9050")).unwrap().to_string(),
        "[::1]:9050"
    );
    assert!(bind_address(Some("0.0.0.0:8083"), None).is_err());
    assert!(bind_address(Some("127.0.0.1"), Some("not-a-port")).is_err());
}

#[test]
fn sumif_depends_on_implicitly_extended_formula_cells() {
    let mut w = Workbook::blank();
    for (a, raw) in [
        ("A1", "=SUMIF(B1:B2,\"x\",C1)"),
        ("B1", "x"),
        ("B2", "x"),
        ("C1", "3"),
        ("C2", "=D1*4"),
        ("D1", "=2+1"),
    ] {
        set(&mut w, 0, a, raw);
    }
    recalculate(&mut w);
    assert_eq!(text(&w, 0, "A1"), "15");
    set(&mut w, 0, "C2", "=A1");
    recalculate(&mut w);
    assert_eq!(text(&w, 0, "A1"), "#CIRC!");
}

#[test]
fn numeric_shaped_inline_text_survives_value_and_style_exports() {
    let mut w = Workbook::blank();
    set(&mut w, 0, "A1", "1");
    set(&mut w, 0, "A2", "2");
    let baseline = codec::encode(&w).unwrap();
    let mut w = codec::decode(&baseline).unwrap();
    ops::write_cells(
        &mut w,
        "Sheet1",
        serde_json::from_value(json!({"A1":"'0042","A2":"'a & <b>"})).unwrap(),
    )
    .unwrap();
    let updated = codec::encode(&w).unwrap();
    assert!(xml(&updated, "xl/worksheets/sheet1.xml").contains("inlineStr"));
    let mut w = codec::decode(&updated).unwrap();
    assert_eq!(w.sheets[0].cells["A1"].value, Value::Text("0042".into()));
    assert_eq!(w.sheets[0].cells["A2"].value, Value::Text("a & <b>".into()));
    assert_eq!(codec::encode(&w).unwrap(), updated);
    ops::format_range(
        &mut w,
        "Sheet1",
        "A1",
        &ops::StylePatch {
            bold: Some(true),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    let formatted = codec::decode(&codec::encode(&w).unwrap()).unwrap();
    assert_eq!(
        formatted.sheets[0].cells["A1"].value,
        Value::Text("0042".into())
    );
    assert!(formatted.sheets[0].cells["A1"].style.bold);
}
