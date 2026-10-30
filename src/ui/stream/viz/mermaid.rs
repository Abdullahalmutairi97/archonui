//! ```` ```mermaid ```` diagrams, drawn by mermaid.js 11.17.2 (bundled, MIT, see
//! `assets/mermaid/LICENSE`) in a headless Chrome nobody sees. Chrome starts on the first diagram
//! and stays for the rest; each diagram comes back as SVG, kept by the hash of its source and theme.

use super::scene::{css, Ink};
use crate::ui::theme::{mix, Rgba};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    net::TcpStream,
    path::PathBuf,
    process::Child,
    sync::{mpsc, Mutex, OnceLock},
    time::{Duration, Instant},
};
use tungstenite::{stream::MaybeTlsStream, Message, WebSocket};

/// mermaid.js, pinned. Updating it means replacing the file and the version in its name.
const MERMAID_JS: &str = include_str!("../../../../assets/mermaid/mermaid-11.17.2.min.js");

/// Longest one diagram may take, Chrome's start included.
const RENDER_TIMEOUT: Duration = Duration::from_secs(30);

/// A diagram's script, the font family it draws in, and where its SVG goes.
type Job = (String, String, mpsc::Sender<Result<String, String>>);

/// Diagrams drawn so far, by the hash of their source and theme.
fn cache() -> &'static Mutex<HashMap<u64, Result<String, String>>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, Result<String, String>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The key a diagram is kept by: its source and every colour and font it is drawn with.
pub fn key(src: &str, ink: &Ink) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    format!("{ink:?}").hash(&mut h);
    h.finish()
}

/// The diagram for `key`, if it has been drawn.
pub fn cached(key: u64) -> Option<Result<String, String>> {
    cache().lock().ok()?.get(&key).cloned()
}

/// Blocking: draws `src` and returns its SVG, or the first line of mermaid's complaint. Call it off
/// the UI thread.
pub fn render(src: &str, ink: &Ink) -> Result<String, String> {
    let k = key(src, ink);
    if let Some(done) = cached(k) {
        return done;
    }
    let script = render_script(src, ink);
    let (tx, rx) = mpsc::channel();
    worker().lock().map_err(|_| "The diagram renderer stopped".to_string())?.send((script, ink.ui_font.clone(), tx)).map_err(|_| "The diagram renderer stopped".to_string())?;
    let result = rx.recv_timeout(RENDER_TIMEOUT).unwrap_or_else(|_| Err("Drawing took too long".into()));
    if let Ok(mut c) = cache().lock() {
        c.insert(k, result.clone());
    }
    result
}

/// The renderer thread's queue. The thread starts Chrome on its first job.
fn worker() -> &'static Mutex<mpsc::Sender<Job>> {
    static WORKER: OnceLock<Mutex<mpsc::Sender<Job>>> = OnceLock::new();
    WORKER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::spawn(move || {
            let mut page: Option<Result<Page, String>> = None;
            for (script, family, reply) in rx {
                let page = page.get_or_insert_with(Page::open);
                let result = match page {
                    Ok(page) => page.load_font(&family).and_then(|()| page.evaluate(&script)),
                    Err(e) => Err(e.clone()),
                };
                let _ = reply.send(result);
            }
        });
        Mutex::new(tx)
    })
}

/// The JavaScript that draws `src` with the theme's colours and returns the SVG.
fn render_script(src: &str, ink: &Ink) -> String {
    let config = json!({
        "startOnLoad": false,
        "theme": "base",
        "securityLevel": "strict",
        "fontFamily": format!("{}, sans-serif", ink.ui_font),
        // Labels as SVG text, not HTML, so the SVG draws the same outside a browser.
        "htmlLabels": false,
        "flowchart": {"curve": "basis", "padding": 14, "htmlLabels": false},
        "sequence": {"mirrorActors": false},
        "gantt": {"barHeight": 22, "fontSize": 12},
        "themeVariables": theme_variables(ink),
    });
    format!(
        "(async () => {{ mermaid.initialize({config}); globalThis.__n = (globalThis.__n || 0) + 1; const r = await mermaid.render('archonui' + globalThis.__n, {src}); return r.svg; }})()",
        src = Value::from(src)
    )
}

/// mermaid's theme variables from the theme, as the prototype's `mTheme` sets them.
pub fn theme_variables(ink: &Ink) -> Value {
    let (bg, fg) = (ink.bg, ink.fg);
    let (acc, acc2) = (ink.pal[1], ink.pal[0]);
    let m = |a: Rgba, b: Rgba, t: f64| css(mix(a, b, t));
    let red = "#f85149";
    let pairs: Vec<(&str, Value)> = vec![
        ("darkMode", Value::from(true)),
        ("background", Value::from(css(bg))),
        ("fontFamily", Value::from(format!("{}, sans-serif", ink.ui_font))),
        ("fontSize", Value::from("14px")),
        ("primaryColor", Value::from(m(bg, fg, 0.09))),
        ("primaryTextColor", Value::from(css(fg))),
        ("primaryBorderColor", Value::from(m(bg, acc2, 0.6))),
        ("secondaryColor", Value::from(m(bg, acc, 0.3))),
        ("secondaryTextColor", Value::from(css(fg))),
        ("tertiaryColor", Value::from(m(bg, fg, 0.04))),
        ("tertiaryTextColor", Value::from(css(fg))),
        ("lineColor", Value::from(m(bg, fg, 0.45))),
        ("textColor", Value::from(css(fg))),
        ("mainBkg", Value::from(m(bg, fg, 0.09))),
        ("nodeBorder", Value::from(m(bg, acc2, 0.6))),
        ("clusterBkg", Value::from(m(bg, fg, 0.035))),
        ("clusterBorder", Value::from(m(bg, fg, 0.22))),
        ("titleColor", Value::from(css(fg))),
        ("edgeLabelBackground", Value::from(m(bg, fg, 0.06))),
        ("actorBkg", Value::from(m(bg, fg, 0.09))),
        ("actorBorder", Value::from(m(bg, acc2, 0.6))),
        ("actorTextColor", Value::from(css(fg))),
        ("actorLineColor", Value::from(m(bg, fg, 0.3))),
        ("signalColor", Value::from(m(bg, fg, 0.7))),
        ("signalTextColor", Value::from(css(fg))),
        ("labelBoxBkgColor", Value::from(m(bg, fg, 0.09))),
        ("labelBoxBorderColor", Value::from(m(bg, acc2, 0.6))),
        ("labelTextColor", Value::from(css(fg))),
        ("loopTextColor", Value::from(css(fg))),
        ("noteBkgColor", Value::from(m(bg, acc2, 0.18))),
        ("noteTextColor", Value::from(css(fg))),
        ("noteBorderColor", Value::from(m(bg, acc2, 0.5))),
        ("activationBkgColor", Value::from(m(bg, acc, 0.4))),
        ("activationBorderColor", Value::from(css(acc))),
        ("sectionBkgColor", Value::from(m(bg, fg, 0.05))),
        ("altSectionBkgColor", Value::from(m(bg, fg, 0.02))),
        ("sectionBkgColor2", Value::from(m(bg, fg, 0.05))),
        ("taskBkgColor", Value::from(m(bg, acc, 0.75))),
        ("taskBorderColor", Value::from(css(acc))),
        ("taskTextColor", Value::from(css(fg))),
        ("taskTextLightColor", Value::from(css(fg))),
        ("taskTextOutsideColor", Value::from(css(fg))),
        ("taskTextDarkColor", Value::from(css(fg))),
        ("activeTaskBkgColor", Value::from(m(bg, acc2, 0.7))),
        ("activeTaskBorderColor", Value::from(css(acc2))),
        ("doneTaskBkgColor", Value::from(m(bg, fg, 0.25))),
        ("doneTaskBorderColor", Value::from(m(bg, fg, 0.4))),
        ("critBkgColor", Value::from(red)),
        ("critBorderColor", Value::from(red)),
        ("gridColor", Value::from(m(bg, fg, 0.15))),
        ("todayLineColor", Value::from(css(acc2))),
        ("excludeBkgColor", Value::from(m(bg, fg, 0.03))),
        ("commitLabelColor", Value::from(css(fg))),
        ("commitLabelBackground", Value::from(m(bg, fg, 0.1))),
        ("tagLabelColor", Value::from(css(fg))),
        ("tagLabelBackground", Value::from(m(bg, fg, 0.1))),
        ("tagLabelBorder", Value::from(css(ink.line2))),
    ];
    let mut v = Value::Object(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect());
    let map = v.as_object_mut().expect("an object");
    for (i, c) in ink.pal.iter().chain(ink.pal.iter()).enumerate() {
        map.insert(format!("git{i}"), css(*c).into());
        map.insert(format!("gitBranchLabel{i}"), css(bg).into());
        map.insert(format!("cScale{i}"), m(bg, *c, 0.55).into());
        map.insert(format!("cScaleLabel{i}"), css(fg).into());
        map.insert(format!("pie{}", i + 1), css(*c).into());
    }
    v
}

/// The blank page mermaid.js runs in, and the Chrome behind it.
struct Page {
    ws: WebSocket<MaybeTlsStream<TcpStream>>,
    /// Bundled font families already given to the page.
    fonts: HashSet<String>,
    session: String,
    next: u64,
    child: Child,
    profile: PathBuf,
}

impl Page {
    fn open() -> Result<Page, String> {
        let (child, profile, ws) = crate::preview::launch_chrome().map_err(|e| format!("Diagrams need Chrome: {e}"))?;
        let mut page = Page { ws, fonts: Default::default(), session: String::new(), next: 0, child, profile };
        let targets = page.call(None, "Target.getTargets", json!({}))?;
        let target = targets["targetInfos"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|t| t["type"] == "page")
            .and_then(|t| t["targetId"].as_str())
            .ok_or("Chrome opened no page")?
            .to_string();
        let attached = page.call(None, "Target.attachToTarget", json!({"targetId": target, "flatten": true}))?;
        page.session = attached["sessionId"].as_str().ok_or("Chrome would not attach to its page")?.to_string();
        page.evaluate_raw(MERMAID_JS, false)?;
        Ok(page)
    }

    /// Gives the page the app's own copy of `family`, once, so mermaid measures labels with the font
    /// the SVG is drawn in. A family the app doesn't bundle is left to the system.
    fn load_font(&mut self, family: &str) -> Result<(), String> {
        use base64::Engine;
        if !self.fonts.insert(family.to_string()) {
            return Ok(());
        }
        let faces: Vec<String> = crate::ui::fonts::FONTS
            .iter()
            .filter(|(f, _, _, _)| *f == family)
            .map(|(f, weight, italic, bytes)| {
                let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                format!(
                    "new FontFace({}, 'url(data:font/ttf;base64,{data})', {{weight: '{weight}', style: '{}'}})",
                    Value::from(*f),
                    if *italic { "italic" } else { "normal" }
                )
            })
            .collect();
        if faces.is_empty() {
            return Ok(());
        }
        let script = format!("(async () => {{ for (const f of [{}]) {{ document.fonts.add(f); await f.load(); }} return ''; }})()", faces.join(","));
        self.evaluate(&script).map(|_| ())
    }

    /// Runs `script`, which returns a promise of a string, and returns the string.
    fn evaluate(&mut self, script: &str) -> Result<String, String> {
        let result = self.evaluate_raw(script, true)?;
        result["result"]["value"].as_str().map(str::to_string).ok_or_else(|| "mermaid returned no drawing".to_string())
    }

    fn evaluate_raw(&mut self, script: &str, wait: bool) -> Result<Value, String> {
        let session = self.session.clone();
        let result = self.call(Some(&session), "Runtime.evaluate", json!({"expression": script, "awaitPromise": wait, "returnByValue": true}))?;
        if let Some(e) = result.get("exceptionDetails") {
            let text = e["exception"]["description"].as_str().or(e["text"].as_str()).unwrap_or("mermaid failed");
            // The first line says what is wrong; the rest is a stack.
            return Err(text.lines().next().unwrap_or(text).trim_start_matches("Error: ").to_string());
        }
        Ok(result)
    }

    /// Sends one DevTools command and waits for its answer, skipping events.
    fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> Result<Value, String> {
        self.next += 1;
        let id = self.next;
        let mut msg = json!({"id": id, "method": method, "params": params});
        if let Some(s) = session {
            msg["sessionId"] = s.into();
        }
        self.ws.send(Message::text(msg.to_string())).map_err(|e| format!("DevTools: {e}"))?;
        let deadline = Instant::now() + RENDER_TIMEOUT;
        while Instant::now() < deadline {
            match self.ws.read() {
                Ok(Message::Text(text)) => {
                    let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
                    if v["id"] == id {
                        if let Some(e) = v.get("error") {
                            return Err(e["message"].as_str().unwrap_or("DevTools error").to_string());
                        }
                        return Ok(v["result"].clone());
                    }
                }
                Ok(_) => {}
                // The socket reads with a short timeout; that is not an error.
                Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(format!("DevTools: {e}")),
            }
        }
        Err("Chrome did not answer".into())
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        crate::preview::stop_process_group(&mut self.child);
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink() -> Ink {
        let t = crate::ui::theme::resolve(crate::ui::theme::preset("catppuccin"), &Default::default(), Default::default());
        Ink::of(&t)
    }

    #[test]
    fn the_theme_reaches_mermaid() {
        let v = theme_variables(&ink());
        assert_eq!(v["critBkgColor"], "#f85149");
        assert!(v["git0"].is_string() && v["pie12"].is_string(), "every series colour is set twice over");
        assert_ne!(key("graph LR\n a-->b", &ink()), key("graph LR\n a-->c", &ink()));
    }

    /// Needs Chrome or Chromium: `cargo test mermaid -- --ignored`.
    #[test]
    #[ignore]
    fn every_demo_diagram_draws_and_rasterises() {
        let replies: serde_json::Map<String, Value> = serde_json::from_str(include_str!("../../../../assets/demo/viz-replies.json")).unwrap();
        let mut drawn = 0;
        for reply in replies.values() {
            let text = reply.as_str().unwrap();
            for part in text.split("```mermaid ").skip(1) {
                let body = part.split_once('\n').unwrap().1.split("\n```").next().unwrap();
                let svg = render(body, &ink()).unwrap_or_else(|e| panic!("{e}\n{body}"));
                let (fixed, size) = super::super::card::fit_svg(&svg, ink().bg);
                assert!(size.0 > 50. && size.1 > 50., "{size:?}");
                super::super::scene::svg_to_png(&fixed).unwrap();
                drawn += 1;
            }
        }
        assert_eq!(drawn, 6, "mind map, sequence, two flowcharts, gantt and git graph");
        assert!(render("flowchart LR\n a -->", &ink()).is_err(), "a broken diagram says so");
    }
}
