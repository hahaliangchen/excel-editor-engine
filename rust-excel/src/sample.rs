use crate::{
    formula::recalculate,
    model::{Cell, Sheet, Workbook},
};

pub fn workbook() -> Workbook {
    let mut details = Sheet {
        name: "季度营收明细".into(),
        frozen: true,
        ..Sheet::default()
    };
    details.merges.push("A1:H1".into());
    details
        .widths
        .extend((1..=8).zip([16., 16., 20., 18., 18., 18., 15., 19.]));
    let mut summary = Sheet {
        name: "部门汇总".into(),
        frozen: true,
        ..Sheet::default()
    };
    summary.merges.push("A1:E1".into());
    summary
        .widths
        .extend((1..=5).zip([21., 22., 22., 19., 19.]));
    fn cell(s: &mut Sheet, a: &str, raw: &str) {
        s.cells.insert(a.into(), Cell::from_raw(raw.into()));
    }
    fn banner(s: &mut Sheet, end: u32) {
        for c in 1..=end {
            let a = format!("{}1", crate::model::Coord::column(c));
            let style = &mut s.cells.entry(a).or_default().style;
            style.fill = Some("#137351".into());
            style.color = Some("#FFFFFF".into());
            style.bold = true;
        }
    }
    fn headers(s: &mut Sheet, labels: &[&str]) {
        for (i, label) in labels.iter().enumerate() {
            let a = format!("{}3", crate::model::Coord::column(i as u32 + 1));
            cell(s, &a, label);
            let st = &mut s.cells.get_mut(&a).unwrap().style;
            st.fill = Some("#DAEFE5".into());
            st.color = Some("#164536".into());
            st.bold = true;
        }
    }
    fn number(s: &mut Sheet, a: &str, format: &str) {
        s.cells.get_mut(a).unwrap().style.number_format = Some(format.into());
    }
    let money = "¥#,##0.00";
    cell(&mut details, "A1", "2026 年第一季度 · 营收明细");
    banner(&mut details, 8);
    cell(
        &mut details,
        "A2",
        "财务运营部  /  2026 Q1  /  金额单位：CNY",
    );
    headers(
        &mut details,
        &[
            "月份",
            "部门",
            "项目",
            "营收",
            "成本",
            "毛利",
            "毛利率",
            "状态",
        ],
    );
    for (i, (month, dept, project, revenue, cost)) in [
        ("一月", "销售部", "企业订阅", 128000, 72000),
        ("一月", "客户成功", "续约服务", 86000, 41000),
        ("一月", "产品部", "专业版", 64000, 39000),
        ("二月", "销售部", "企业订阅", 154000, 83000),
        ("二月", "客户成功", "增值服务", 92000, 45000),
        ("二月", "产品部", "专业版", 72500, 42000),
        ("三月", "销售部", "渠道合作", 176000, 94000),
        ("三月", "客户成功", "续约服务", 104000, 49000),
        ("三月", "产品部", "团队版", 98500, 51000),
    ]
    .into_iter()
    .enumerate()
    {
        let r = i + 4;
        for (col, value) in [
            ('A', month.to_owned()),
            ('B', dept.to_owned()),
            ('C', project.to_owned()),
            ('D', revenue.to_string()),
            ('E', cost.to_string()),
            ('F', format!("=D{r}-E{r}")),
            ('G', format!("=IFERROR(F{r}/D{r},0)")),
            ('H', format!("=IF(G{r}>=45%,\"达标\",\"关注\")")),
        ] {
            cell(&mut details, &format!("{col}{r}"), &value);
        }
        for col in ['D', 'E', 'F'] {
            number(&mut details, &format!("{col}{r}"), money);
        }
        number(&mut details, &format!("G{r}"), "0.0%");
    }
    cell(&mut details, "A13", "季度合计");
    for col in ['D', 'E', 'F'] {
        let a = format!("{col}13");
        cell(&mut details, &a, &format!("=SUM({col}4:{col}12)"));
        number(&mut details, &a, money);
    }
    cell(&mut details, "G13", "=IFERROR(F13/D13,0)");
    number(&mut details, "G13", "0.0%");
    for col in 'A'..='H' {
        let st = &mut details.cells.entry(format!("{col}13")).or_default().style;
        st.fill = Some("#DAEFE5".into());
        st.color = Some("#164536".into());
        st.bold = true;
    }
    cell(&mut summary, "A1", "2026 年第一季度 · 部门汇总");
    banner(&mut summary, 5);
    cell(&mut summary, "A2", "数据来源：季度营收明细");
    headers(
        &mut summary,
        &["部门", "营收合计", "毛利合计", "毛利率", "状态"],
    );
    for (r, dept) in (4..=6).zip(["销售部", "客户成功", "产品部"]) {
        cell(&mut summary, &format!("A{r}"), dept);
        for (col, source) in [('B', 'D'), ('C', 'F')] {
            let a = format!("{col}{r}");
            cell(
                &mut summary,
                &a,
                &format!("=SUMIF('季度营收明细'!B4:B12,A{r},'季度营收明细'!{source}4:{source}12)"),
            );
            number(&mut summary, &a, money);
        }
        cell(
            &mut summary,
            &format!("D{r}"),
            &format!("=IFERROR(C{r}/B{r},0)"),
        );
        number(&mut summary, &format!("D{r}"), "0.0%");
        cell(
            &mut summary,
            &format!("E{r}"),
            &format!("=IF(D{r}>=45%,\"达标\",\"关注\")"),
        );
    }
    cell(&mut summary, "A8", "季度总营收");
    cell(&mut summary, "B8", "=SUM('季度营收明细'!D4:D12)");
    number(&mut summary, "B8", money);
    cell(&mut summary, "A9", "单项目均值");
    cell(&mut summary, "B9", "=AVERAGE('季度营收明细'!D4:D12)");
    number(&mut summary, "B9", money);
    let mut wb = Workbook {
        sheets: vec![details, summary],
        source: None,
    };
    recalculate(&mut wb);
    wb
}
