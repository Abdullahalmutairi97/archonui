//! A ```` ```viz ```` block's JSON as data, the views it can switch between, and the conversions and
//! number formats the charts share. Pure, so the rules are tested without GPUI.

use serde_json::Value;

/// A chart, table or stat row, parsed from its JSON.
#[derive(Debug, Clone, PartialEq)]
pub enum Spec {
    Xy(Xy),
    Pie(Pie),
    Scatter(Scatter),
    Table(Table),
    Stats(Stats),
}

/// Bars, lines or areas over named x positions.
#[derive(Debug, Clone, PartialEq)]
pub struct Xy {
    pub kind: View,
    pub title: String,
    pub unit: String,
    pub x_label: String,
    pub x: Vec<String>,
    pub series: Vec<Series>,
    pub stacked: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    pub name: String,
    pub data: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pie {
    pub donut: bool,
    pub title: String,
    pub unit: String,
    pub data: Vec<(String, f64)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scatter {
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub series: Vec<Points>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Points {
    pub name: String,
    /// x, y, and an optional label.
    pub points: Vec<(f64, f64, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Cell>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    Num(f64),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stats {
    pub title: String,
    pub items: Vec<Stat>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stat {
    pub label: String,
    pub value: String,
    pub delta: String,
    /// Whether a rise is good. `"good": "down"` makes a fall good, as for latency.
    pub up_is_good: bool,
    pub trend: Vec<f64>,
}

impl Stat {
    /// Whether the delta is good news: it goes the way `good` says.
    pub fn good(&self) -> bool {
        let first = self.delta.trim().chars().next();
        let rising = !matches!(first, Some('-' | '−'));
        rising == self.up_is_good
    }
}

/// What a chart is drawn as. The header's switch moves between the views a spec allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum View {
    Bar,
    Line,
    Area,
    Donut,
    Pie,
    Scatter,
    Table,
    Stats,
}

impl View {
    pub fn name(self) -> &'static str {
        match self {
            View::Bar => "bar",
            View::Line => "line",
            View::Area => "area",
            View::Donut => "donut",
            View::Pie => "pie",
            View::Scatter => "scatter",
            View::Table => "table",
            View::Stats => "stats",
        }
    }

    /// The kind label over the card.
    pub fn kind(self) -> &'static str {
        match self {
            View::Bar => "Bar chart",
            View::Line => "Line chart",
            View::Area => "Area chart",
            View::Donut => "Donut chart",
            View::Pie => "Pie chart",
            View::Scatter => "Scatter plot",
            View::Table => "Table",
            View::Stats => "Stats",
        }
    }
}

impl Spec {
    /// Parses a viz block's JSON. The error says what is wrong in a few words.
    pub fn parse(json: &str) -> Result<Spec, String> {
        let v: Value = serde_json::from_str(json.trim()).map_err(|e| format!("Not valid JSON: {e}"))?;
        let ty = v["type"].as_str().ok_or("Missing \"type\"")?;
        let title = text(&v["title"]);
        let unit = text(&v["unit"]);
        let spec = match ty {
            "bar" => parse_xy(&v, View::Bar, title, unit)?,
            "line" => parse_xy(&v, View::Line, title, unit)?,
            "area" => parse_xy(&v, View::Area, title, unit)?,
            "pie" | "donut" => parse_pie(&v, ty == "donut", title, unit)?,
            "scatter" => parse_scatter(&v, title)?,
            "table" => parse_table(&v, title)?,
            "stats" => parse_stats(&v, title)?,
            other => return Err(format!("Unknown chart type \"{other}\"")),
        };
        Ok(spec)
    }

    pub fn title(&self) -> &str {
        match self {
            Spec::Xy(s) => &s.title,
            Spec::Pie(s) => &s.title,
            Spec::Scatter(s) => &s.title,
            Spec::Table(s) => &s.title,
            Spec::Stats(s) => &s.title,
        }
    }

    /// The views the header switches between, the first being the one it opens with. Tables and
    /// stats have only themselves.
    pub fn views(&self) -> Vec<View> {
        match self {
            Spec::Xy(s) => std::iter::once(s.kind).chain([View::Bar, View::Line, View::Area, View::Table].into_iter().filter(|x| *x != s.kind)).collect(),
            Spec::Pie(p) if p.donut => vec![View::Donut, View::Pie, View::Bar, View::Table],
            Spec::Pie(_) => vec![View::Pie, View::Donut, View::Bar, View::Table],
            Spec::Scatter(_) => vec![View::Scatter, View::Table],
            Spec::Table(_) => vec![View::Table],
            Spec::Stats(_) => vec![View::Stats],
        }
    }

    /// The spec as bars, lines or areas: itself, or a pie's slices as one series.
    pub fn as_xy(&self) -> Option<Xy> {
        match self {
            Spec::Xy(s) => Some(s.clone()),
            Spec::Pie(p) => Some(Xy {
                kind: View::Bar,
                title: p.title.clone(),
                unit: p.unit.clone(),
                x_label: String::new(),
                x: p.data.iter().map(|d| d.0.clone()).collect(),
                series: vec![Series { name: if p.title.is_empty() { "Value".into() } else { p.title.clone() }, data: p.data.iter().map(|d| d.1).collect() }],
                stacked: false,
            }),
            _ => None,
        }
    }

    /// The spec as a table, so every chart can be read as numbers.
    pub fn as_table(&self) -> Table {
        match self {
            Spec::Table(t) => t.clone(),
            Spec::Pie(p) => {
                let total: f64 = p.data.iter().map(|d| d.1).sum();
                Table {
                    title: p.title.clone(),
                    columns: vec!["Label".into(), "Value".into(), "Share".into()],
                    rows: p
                        .data
                        .iter()
                        .map(|(l, v)| vec![Cell::Text(l.clone()), Cell::Num(*v), Cell::Text(format!("{}%", fmt((v / total * 1000.).round() / 10.)))])
                        .collect(),
                }
            }
            Spec::Scatter(s) => Table {
                title: s.title.clone(),
                columns: vec![
                    "Series".into(),
                    "Label".into(),
                    if s.x_label.is_empty() { "x".into() } else { s.x_label.clone() },
                    if s.y_label.is_empty() { "y".into() } else { s.y_label.clone() },
                ],
                rows: s
                    .series
                    .iter()
                    .flat_map(|ser| ser.points.iter().map(|p| vec![Cell::Text(ser.name.clone()), Cell::Text(p.2.clone()), Cell::Num(p.0), Cell::Num(p.1)]))
                    .collect(),
            },
            Spec::Xy(s) => {
                let mut columns = vec![s.x_label.clone()];
                columns.extend(s.series.iter().map(|x| x.name.clone()));
                let rows = s
                    .x
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        let mut row = vec![Cell::Text(l.clone())];
                        row.extend(s.series.iter().map(|x| Cell::Num(x.data.get(i).copied().unwrap_or(0.))));
                        row
                    })
                    .collect();
                Table { title: s.title.clone(), columns, rows }
            }
            Spec::Stats(s) => Table {
                title: s.title.clone(),
                columns: vec!["Label".into(), "Value".into(), "Change".into()],
                rows: s.items.iter().map(|i| vec![Cell::Text(i.label.clone()), Cell::Text(i.value.clone()), Cell::Text(i.delta.clone())]).collect(),
            },
        }
    }
}

impl Table {
    /// Whether column `c` holds numbers, so it is right-aligned in the code font.
    pub fn numeric(&self, c: usize) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().all(|r| match r.get(c) {
                Some(Cell::Num(_)) => true,
                Some(Cell::Text(t)) => looks_numeric(t),
                None => false,
            })
    }

    /// The row order for a sort on column `c`, ascending or not. Numbers compare as numbers.
    pub fn order(&self, sort: Option<(usize, bool)>) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.rows.len()).collect();
        if let Some((c, ascending)) = sort {
            order.sort_by(|&a, &b| {
                let (x, y) = (self.rows[a].get(c), self.rows[b].get(c));
                let o = match (x, y) {
                    (Some(Cell::Num(x)), Some(Cell::Num(y))) => x.partial_cmp(y).unwrap_or(std::cmp::Ordering::Equal),
                    _ => cell_text(x).to_lowercase().cmp(&cell_text(y).to_lowercase()),
                };
                if ascending { o } else { o.reverse() }
            });
        }
        order
    }
}

/// A cell as text: numbers in the charts' format.
pub fn cell_text(cell: Option<&Cell>) -> String {
    match cell {
        Some(Cell::Num(n)) => fmt(*n),
        Some(Cell::Text(t)) => t.clone(),
        None => String::new(),
    }
}

/// `12`, `3.25`, `12.4k`, `1.2M`: short numbers for axes and tooltips.
pub fn fmt(n: f64) -> String {
    let a = n.abs();
    let trim = |x: f64, places: usize| {
        let s = format!("{x:.places$}");
        let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_string() } else { s };
        if s == "-0" { "0".to_string() } else { s }
    };
    if a >= 1e6 {
        format!("{}M", trim(n / 1e6, 1))
    } else if a >= 1e4 {
        format!("{}k", trim(n / 1e3, 1))
    } else if n.fract() == 0. {
        trim(n, 0)
    } else {
        trim(n, 2)
    }
}

/// The top of an axis for `v`: 1, 2, 2.5, 5 or 10 times a power of ten.
pub fn nice_max(v: f64) -> f64 {
    if !(v > 0.) || !v.is_finite() {
        return 1.;
    }
    let p = 10f64.powf(v.log10().floor());
    let f = v / p;
    let step = if f <= 1. { 1. } else if f <= 2. { 2. } else if f <= 2.5 { 2.5 } else if f <= 5. { 5. } else { 10. };
    step * p
}

/// A diagram's kind label, from its first word.
pub fn mermaid_kind(src: &str) -> &'static str {
    match src.split_whitespace().next().unwrap_or_default().to_lowercase().as_str() {
        "flowchart" => "Flowchart",
        "graph" => "Graph",
        "sequencediagram" => "Sequence",
        "gantt" => "Timeline",
        "mindmap" => "Mind map",
        "gitgraph" => "Git graph",
        "architecture-beta" => "Architecture",
        _ => "Diagram",
    }
}

fn parse_xy(v: &Value, kind: View, title: String, unit: String) -> Result<Spec, String> {
    let x: Vec<String> = v["x"].as_array().ok_or("A chart needs an \"x\" list")?.iter().map(text).collect();
    let series: Vec<Series> = v["series"]
        .as_array()
        .ok_or("A chart needs a \"series\" list")?
        .iter()
        .map(|s| Series { name: text(&s["name"]), data: s["data"].as_array().map_or_else(Vec::new, |d| d.iter().map(num).collect()) })
        .collect();
    if x.is_empty() || series.is_empty() {
        return Err("A chart needs at least one x and one series".into());
    }
    Ok(Spec::Xy(Xy { kind, title, unit, x_label: text(&v["xLabel"]), x, series, stacked: v["stacked"].as_bool().unwrap_or(false) }))
}

fn parse_pie(v: &Value, donut: bool, title: String, unit: String) -> Result<Spec, String> {
    let data: Vec<(String, f64)> = v["data"]
        .as_array()
        .ok_or("A pie needs a \"data\" list")?
        .iter()
        .map(|d| (text(&d["label"]), num(&d["value"]).max(0.)))
        .collect();
    if data.iter().map(|d| d.1).sum::<f64>() <= 0. {
        return Err("A pie needs values above zero".into());
    }
    Ok(Spec::Pie(Pie { donut, title, unit, data }))
}

fn parse_scatter(v: &Value, title: String) -> Result<Spec, String> {
    let series: Vec<Points> = v["series"]
        .as_array()
        .ok_or("A scatter plot needs a \"series\" list")?
        .iter()
        .map(|s| Points {
            name: text(&s["name"]),
            points: s["points"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| Some((num(p.get(0)?), num(p.get(1)?), p.get(2).map(text).unwrap_or_default())))
                .collect(),
        })
        .collect();
    if series.iter().all(|s| s.points.is_empty()) {
        return Err("A scatter plot needs points".into());
    }
    Ok(Spec::Scatter(Scatter { title, x_label: text(&v["xLabel"]), y_label: text(&v["yLabel"]), series }))
}

fn parse_table(v: &Value, title: String) -> Result<Spec, String> {
    let columns: Vec<String> = v["columns"].as_array().ok_or("A table needs \"columns\"")?.iter().map(text).collect();
    let rows = v["rows"]
        .as_array()
        .ok_or("A table needs \"rows\"")?
        .iter()
        .map(|r| r.as_array().into_iter().flatten().map(cell).collect())
        .collect();
    Ok(Spec::Table(Table { title, columns, rows }))
}

fn parse_stats(v: &Value, title: String) -> Result<Spec, String> {
    let items = v["items"]
        .as_array()
        .ok_or("Stats need an \"items\" list")?
        .iter()
        .map(|i| Stat {
            label: text(&i["label"]),
            value: text(&i["value"]),
            delta: text(&i["delta"]),
            up_is_good: i["good"].as_str() != Some("down"),
            trend: i["trend"].as_array().map_or_else(Vec::new, |t| t.iter().map(num).collect()),
        })
        .collect();
    Ok(Spec::Stats(Stats { title, items }))
}

fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), fmt),
        other => other.to_string(),
    }
}

fn num(v: &Value) -> f64 {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()).unwrap_or(0.),
        Value::String(s) => s.trim().parse().ok().filter(|f: &f64| f.is_finite()).unwrap_or(0.),
        _ => 0.,
    }
}

fn cell(v: &Value) -> Cell {
    match v {
        Value::Number(n) => Cell::Num(n.as_f64().unwrap_or(0.)),
        other => Cell::Text(text(other)),
    }
}

/// `412`, `1,200`, `12.5%`, `+3 pts`, `38 ms`: a number with at most a short unit after it.
fn looks_numeric(t: &str) -> bool {
    let t = t.trim();
    let digits_end = t.find(|c: char| !(c.is_ascii_digit() || ".,%+−-".contains(c))).unwrap_or(t.len());
    let (num, rest) = t.split_at(digits_end);
    let rest = rest.trim_start();
    num.chars().any(|c| c.is_ascii_digit()) && rest.chars().count() <= 3 && rest.chars().all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_read_short() {
        assert_eq!(fmt(12.), "12");
        assert_eq!(fmt(3.256), "3.26");
        assert_eq!(fmt(12_400.), "12.4k");
        assert_eq!(fmt(1_200_000.), "1.2M");
        assert_eq!(fmt(10_000.), "10k");
        assert_eq!(fmt(0.5), "0.5");
        assert_eq!(fmt(-0.), "0");
    }

    #[test]
    fn axis_tops_are_round() {
        assert_eq!(nice_max(98.), 100.);
        assert_eq!(nice_max(4.1), 5.);
        assert_eq!(nice_max(31.), 50.);
        assert_eq!(nice_max(21.), 25.);
        assert_eq!(nice_max(0.), 1.);
        assert_eq!(nice_max(f64::NAN), 1.);
    }

    #[test]
    fn the_demo_specs_parse() {
        let bar = Spec::parse(r#"{"type":"bar","stacked":true,"title":"Revenue by plan","unit":"k","x":["Jul","Aug","Sep"],"series":[{"name":"Pro","data":[14,16,18]},{"name":"Team","data":[8,9,11]}]}"#).unwrap();
        assert_eq!(bar.views(), [View::Bar, View::Line, View::Area, View::Table]);
        let Spec::Xy(xy) = &bar else { panic!() };
        assert!(xy.stacked && xy.series[1].data == [8., 9., 11.]);
        let donut = Spec::parse(r#"{"type":"donut","title":"Signups","data":[{"label":"Organic","value":4200},{"label":"Paid","value":700}]}"#).unwrap();
        assert_eq!(donut.views(), [View::Donut, View::Pie, View::Bar, View::Table]);
        assert_eq!(donut.as_xy().unwrap().series[0].data, [4200., 700.]);
        let t = donut.as_table();
        assert_eq!(cell_text(t.rows[0].get(2)), "85.7%");
        let line = Spec::parse(r#"{"type":"line","x":["a"],"series":[{"name":"p50","data":[1]}]}"#).unwrap();
        assert_eq!(line.views()[0], View::Line);
        let sc = Spec::parse(r#"{"type":"scatter","xLabel":"Tickets","series":[{"name":"W","points":[[42,18,"Mon"],[38,15]]}]}"#).unwrap();
        assert_eq!(sc.as_table().columns, ["Series", "Label", "Tickets", "y"]);
        let stats = Spec::parse(r#"{"type":"stats","items":[{"label":"p95","value":"38 ms","delta":"−61%","good":"down","trend":[98,38]},{"label":"Users","value":"12k","delta":"+18%"}]}"#).unwrap();
        let Spec::Stats(s) = stats else { panic!() };
        assert!(s.items[0].good() && s.items[1].good());
    }

    #[test]
    fn bad_specs_say_why() {
        assert!(Spec::parse("{").unwrap_err().starts_with("Not valid JSON"));
        assert_eq!(Spec::parse(r#"{"type":"radar"}"#).unwrap_err(), "Unknown chart type \"radar\"");
        assert_eq!(Spec::parse(r#"{"title":"x"}"#).unwrap_err(), "Missing \"type\"");
        assert!(Spec::parse(r#"{"type":"bar","x":[],"series":[]}"#).is_err());
        assert!(Spec::parse(r#"{"type":"pie","data":[{"label":"a","value":0}]}"#).is_err());
    }

    #[test]
    fn tables_sort_numbers_as_numbers_and_spot_number_columns() {
        let t = Table {
            title: String::new(),
            columns: vec!["Request".into(), "Votes".into(), "Note".into()],
            rows: vec![
                vec![Cell::Text("b".into()), Cell::Num(9.), Cell::Text("12 ms".into())],
                vec![Cell::Text("a".into()), Cell::Num(100.), Cell::Text("Planned".into())],
            ],
        };
        assert_eq!(t.order(Some((1, true))), [0, 1]);
        assert_eq!(t.order(Some((1, false))), [1, 0]);
        assert_eq!(t.order(Some((0, true))), [1, 0]);
        assert!(!t.numeric(0) && t.numeric(1) && !t.numeric(2));
        assert!(looks_numeric("+3 pts") && looks_numeric("1,200") && looks_numeric("12.5%") && !looks_numeric("Done"));
    }

    #[test]
    fn diagram_kinds_come_from_the_first_word() {
        assert_eq!(mermaid_kind("sequenceDiagram\n A->>B: x"), "Sequence");
        assert_eq!(mermaid_kind("gitGraph\n commit"), "Git graph");
        assert_eq!(mermaid_kind("flowchart LR"), "Flowchart");
        assert_eq!(mermaid_kind("pie"), "Diagram");
    }
}
