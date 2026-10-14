mod app;
mod chat;
mod editor;
mod engine;
mod git;
mod ide;
mod preview;
mod store;
mod terminal;
mod thumbs;
mod ui;
mod workspace;

use engine::{Event, Out};
use gpui::{prelude::*, px, size, App, Application, Bounds, WindowBounds, WindowOptions};
use smol::channel::Receiver;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use terminal::{Screen, Span, TermEvent, Terminal};

/// Directory new sessions run in: `ARCHONUI_CWD`, else where the app was started.
fn working_dir() -> PathBuf {
    std::env::var_os("ARCHONUI_CWD")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Headless use: `--detect` lists agents, `--discover` their commands and
/// models; `--ask [--model <id>] [--effort <level>] [--file <path>]... <agent>
/// <prompt>...` sends each prompt in turn within one session and prints the
/// replies (files go with the last prompt).
fn headless(args: &[String]) -> bool {
    match args.first().map(String::as_str) {
        Some("--detect") => {
            for a in engine::detect(&store::load_settings()) {
                println!("{:<10} {:<12} {:<9} {}", a.harness.id(), a.version, a.auth.label(), a.path.display());
            }
        }
        // `--preview-shot <url> <out.png> [<x> <y> <out2.png>]`: load a page in
        // the preview browser, save what it shows, optionally click and save again.
        Some("--preview-shot") => {
            if let Err(e) = preview_shot(&args[1..]) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        // `--approval-mcp <socket> <session>`: the MCP server Claude asks about tool permissions (engine/approve.rs).
        Some("--approval-mcp") => {
            let [_, socket, session] = args else {
                eprintln!("usage: --approval-mcp <socket> <session>");
                std::process::exit(2);
            };
            if let Err(e) = engine::approve::run_server(std::path::Path::new(socket), session) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        // `--test <agent>`: one "Reply with exactly: ok" turn with the agent's defaults.
        Some("--test") => {
            let agents = engine::detect(&store::load_settings());
            let Some(agent) = args.get(1).and_then(|id| agents.iter().find(|a| a.harness.id() == id)) else {
                eprintln!("usage: --test <agent>, an installed agent id");
                std::process::exit(2);
            };
            match agent.harness.test_connection(&agent.path, &working_dir()) {
                Ok(reply) => println!("{}: {reply}", agent.harness.name()),
                Err(e) => {
                    eprintln!("{}: {e}", agent.harness.name());
                    std::process::exit(1);
                }
            }
        }
        // `--term-test`: run a shell in the embedded terminal and check its screen.
        Some("--term-test") => {
            if let Err(e) = term_test() {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        // `--tmux-test`: check that a tmux-backed shell outlives its terminal. Uses `ARCHONUI_TMUX_SOCKET`.
        Some("--tmux-test") => {
            if let Err(e) = tmux_test() {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        Some("--discover") => {
            for a in engine::detect(&store::load_settings()) {
                let caps = engine::discover(&a, &working_dir());
                println!(
                    "{}: {} commands, {} models, default {}",
                    a.harness.name(),
                    caps.commands.len(),
                    caps.models.len(),
                    caps.default_model.as_deref().unwrap_or("unknown"),
                );
                println!(
                    "  default effort {} of [{}]",
                    caps.default_effort.as_deref().unwrap_or("unknown"),
                    caps.default_efforts.join(", "),
                );
                for m in caps.models.iter().take(6) {
                    println!("  {:<40} {:<24} [{}]", m.id, m.label, m.efforts.join(", "));
                }
            }
        }
        Some("--ask") => {
            let mut rest = &args[1..];
            let config = app::persist::Paths::current().config;
            let settings = app::persist::load(&config, &store::settings_path()).settings;
            let tools = engine::ToolSettings {
                max_steps: Some(settings.max_steps),
                sandbox: settings.sandbox,
                ask_network: settings.ask_network,
                ..Default::default()
            };
            let mut opts = engine::TurnOptions { tools, ..Default::default() };
            let mut files = Vec::new();
            while let [flag, value, tail @ ..] = rest {
                match flag.as_str() {
                    "--model" => opts.model = Some(value),
                    "--effort" => opts.effort = Some(value),
                    "--mode" => {
                        opts.mode = engine::Mode::from_name(value).unwrap_or_else(|| {
                            eprintln!("mode must be auto, plan or ask");
                            std::process::exit(2);
                        })
                    }
                    "--file" => match engine::attach::Attachment::inspect(value.as_ref()) {
                        Ok(a) => files.push(a),
                        Err(e) => {
                            eprintln!("{e}");
                            std::process::exit(2);
                        }
                    },
                    _ => break,
                }
                rest = tail;
            }
            let (Some(id), prompts) = (rest.first(), &rest[1.min(rest.len())..]) else {
                eprintln!("usage: --ask [--model <id>] [--effort <level>] [--file <path>]... <agent> <prompt>...");
                std::process::exit(2);
            };
            let agents = engine::detect(&store::load_settings());
            let Some(agent) = agents.iter().find(|a| a.harness.id() == id) else {
                eprintln!("agent '{id}' not found");
                std::process::exit(1);
            };
            let mut session = agent.harness.preassign_session().then(store::new_id);
            for prompt in prompts {
                println!("> {prompt}");
                let last = std::ptr::eq(prompt, prompts.last().unwrap());
                let opts = engine::TurnOptions { files: if last { &files } else { &[] }, ..opts };
                let turn =
                    engine::start_turn(agent, &working_dir(), prompt, session.as_deref(), opts)
                        .expect("start agent");
                let mut streamed = false;
                while let Ok(ev) = smol::block_on(turn.events.recv()) {
                    match ev {
                        Event::Out(Out::Text(t)) => {
                            streamed = true;
                            print!("{t}");
                        }
                        Event::Out(Out::Final(t)) if !streamed => print!("{t}"),
                        Event::Out(Out::Final(_)) => {}
                        Event::Out(Out::Session(s)) => {
                            session.get_or_insert(s);
                        }
                        Event::Out(other) => eprintln!("[{other:?}]"),
                        Event::Finished(r) => println!("\n[finished: {r:?}, session: {session:?}]"),
                    }
                }
            }
        }
        _ => return false,
    }
    true
}

fn preview_shot(args: &[String]) -> Result<(), String> {
    use preview::{Browser, BrowserEvent, Command, MouseKind};
    let [url, out, rest @ ..] = args else { return Err("usage: --preview-shot <url> <out.png> [x y out2.png]".into()) };
    let browser = Browser::launch()?;
    browser.send(Command::Resize { width: 800, height: 600, scale: 1.0 });
    browser.send(Command::Navigate(preview::normalize_address(url, &working_dir())));
    // Wait until loading stops and frames settle, keeping the newest frame.
    let wait = |browser: &Browser, quiet_ms: u64| -> Result<std::sync::Arc<gpui::RenderImage>, String> {
        let mut last = None;
        let start = std::time::Instant::now();
        let mut loading = true;
        let mut quiet_since = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(30) {
            match smol::block_on(async {
                smol::future::or(async { browser.events.recv().await.ok() }, async {
                    smol::Timer::after(std::time::Duration::from_millis(100)).await;
                    None
                })
                .await
            }) {
                Some(BrowserEvent::Frame(f, w, h)) => {
                    eprintln!("frame {w}x{h}");
                    last = Some(f);
                    quiet_since = std::time::Instant::now();
                }
                Some(BrowserEvent::Loading(l)) => loading = l,
                Some(BrowserEvent::Url(u)) => eprintln!("url {u}"),
                Some(BrowserEvent::Title(t)) => eprintln!("title {t}"),
                Some(BrowserEvent::History { .. }) => {}
                Some(BrowserEvent::Error(e)) => return Err(e),
                Some(BrowserEvent::Closed) => return Err("browser closed".into()),
                None => {}
            }
            if !loading && last.is_some() && quiet_since.elapsed().as_millis() as u64 > quiet_ms {
                break;
            }
        }
        last.ok_or_else(|| "no frame received".to_string())
    };
    let save = |frame: &gpui::RenderImage, path: &str| -> Result<(), String> {
        let size = frame.size(0);
        let mut bytes = frame.as_bytes(0).ok_or_else(|| "empty frame".to_string())?.to_vec();
        for px in bytes.chunks_exact_mut(4) {
            px.swap(0, 2);
        }
        image::RgbaImage::from_raw(size.width.0 as u32, size.height.0 as u32, bytes)
            .ok_or_else(|| "bad frame".to_string())?
            .save(path)
            .map_err(|e| e.to_string())
    };
    save(&*wait(&browser, 700)?, out)?;
    if let [x, y, out2, ..] = rest {
        let (x, y): (f32, f32) = (x.parse().map_err(|_| "bad x")?, y.parse().map_err(|_| "bad y")?);
        for kind in [MouseKind::Move, MouseKind::Down, MouseKind::Up] {
            browser.send(Command::Mouse { kind, x, y, right: false, clicks: 1 });
        }
        save(&*wait(&browser, 700)?, out2)?;
    }
    Ok(())
}

/// The next terminal event, or `None` if none arrives within `timeout`.
fn next_event(events: &Receiver<TermEvent>, timeout: Duration) -> Option<TermEvent> {
    smol::block_on(smol::future::or(async { events.recv().await.ok() }, async {
        smol::Timer::after(timeout).await;
        None
    }))
}

/// Waits up to five seconds for the screen to satisfy `done`.
fn wait_for(
    term: &Terminal,
    events: &Receiver<TermEvent>,
    what: &str,
    done: impl Fn(&Screen) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if done(&term.screen()) {
            return Ok(());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        match next_event(events, left) {
            Some(TermEvent::Exited(code)) => {
                return Err(format!("the shell exited ({code:?}) before {what}"));
            }
            Some(TermEvent::Changed) => {}
            None => return Err(format!("timed out waiting for {what}")),
        }
    }
}

/// A screen row's text.
fn row_text(row: &[Span]) -> String {
    row.iter().map(|span| span.text.as_str()).collect()
}

/// Whether tmux has a session `name` on the socket `socket`.
fn tmux_has_session(socket: &str, name: &str) -> Result<bool, String> {
    let status = std::process::Command::new("tmux")
        .args(["-L", socket, "has-session", "-t", &format!("={name}")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| e.to_string())?;
    Ok(status.success())
}

fn term_test() -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let (term, events) = Terminal::spawn(&cwd, 80, 24, None)?;

    term.write(b"echo hello-$((40+2))\r");
    wait_for(&term, &events, "hello-42", |s| s.rows.iter().any(|r| row_text(r).contains("hello-42")))?;

    term.write(b"printf '\\033[31mRED\\033[0m\\n'\r");
    wait_for(&term, &events, "red text", |s| {
        s.rows.iter().flatten().any(|span| span.text.contains("RED") && span.fg == Some(0xcd3131))
    })?;

    term.write(b"printf '\\033[?1049h'\r");
    wait_for(&term, &events, "alternate screen", |s| s.alternate)?;
    term.write(b"printf '\\033[?1049l'\r");
    wait_for(&term, &events, "normal screen", |s| !s.alternate)?;

    term.resize(100, 30);
    if term.screen().rows.len() != 30 {
        return Err("resize did not change the screen height".into());
    }
    term.write(b"seq 1 60\r");
    wait_for(&term, &events, "line 60", |s| s.rows.iter().any(|r| row_text(r).trim_end() == "60"))?;
    term.set_scrollback(5);
    if term.scrollback() != 5 || term.screen().cursor.is_some() {
        return Err("scrolled-back view is not shown".into());
    }
    term.set_scrollback(0);
    if term.screen().cursor.is_none() {
        return Err("live view has no cursor".into());
    }

    // The shell sets the title at every prompt, so it's shown rather than checked.
    let screen = term.screen();
    if !screen.title.is_empty() {
        println!("title: {}", screen.title);
    }
    for row in &screen.rows {
        let text = row_text(row);
        if !text.trim_end().is_empty() {
            println!("{}", text.trim_end());
        }
    }

    term.write(b"exit\r");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match next_event(&events, left) {
            Some(TermEvent::Exited(_)) => break,
            Some(TermEvent::Changed) => {}
            None => return Err("the shell did not exit within 3 seconds".into()),
        }
    }
    println!("OK");
    Ok(())
}

/// Whether tmux's pane of session `name` is in a mode, such as copy mode.
fn tmux_in_mode(socket: &str, name: &str) -> Result<bool, String> {
    let out = std::process::Command::new("tmux")
        .args(["-L", socket, "display", "-p", "-t", &format!("={name}:"), "#{pane_in_mode}"])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).trim() == "1")
}

/// `--tmux-test`: a shell in a tmux session outlives the `Terminal` that started it, and a
/// new `Terminal` attaches to the same shell, with its state. The wheel's copy mode
/// starts and stops too.
fn tmux_test() -> Result<(), String> {
    use terminal::{tmux_cancel, tmux_kill, tmux_scroll, tmux_socket};

    let name = "qa-tmux";
    let socket = tmux_socket();
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;

    let (term, events) = Terminal::spawn(&cwd, 80, 24, Some(name))?;
    if term.tmux() != Some(name) {
        return Err("the terminal did not start in tmux".into());
    }
    term.write(b"export MARK=persist-$((6*7)); echo started-$((1+1))\r");
    wait_for(&term, &events, "started-2", |s| s.rows.iter().any(|r| row_text(r).contains("started-2")))?;
    drop(term);
    if !tmux_has_session(&socket, name)? {
        return Err("dropping the terminal ended the tmux session".into());
    }

    let (term, events) = Terminal::spawn(&cwd, 80, 24, Some(name))?;
    term.write(b"echo $MARK\r");
    wait_for(&term, &events, "persist-42", |s| s.rows.iter().any(|r| row_text(r).contains("persist-42")))?;

    // The scroll runs on a thread, so poll for the copy mode it enters.
    tmux_scroll(name, true, 3);
    let deadline = Instant::now() + Duration::from_secs(3);
    while !tmux_in_mode(&socket, name)? {
        if Instant::now() > deadline {
            return Err("scrolling did not enter tmux's copy mode".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    tmux_cancel(name);
    if tmux_in_mode(&socket, name)? {
        return Err("cancelling did not leave tmux's copy mode".into());
    }

    tmux_kill(name);
    if tmux_has_session(&socket, name)? {
        return Err("tmux_kill left the session running".into());
    }
    println!("OK");
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if headless(&args) {
        return;
    }
    // The previous UI, kept for a while behind `--old-ui`.
    let old_ui = args.first().map(String::as_str) == Some("--old-ui");
    // Development check of the message stream.
    if args.first().map(String::as_str) == Some("--stream-demo") {
        Application::new().run(|cx: &mut App| {
            ui::fonts::register(cx);
            ui::stream::demo::stream_demo(cx);
        });
        return;
    }
    // Development check of the shared widgets.
    if args.first().map(String::as_str) == Some("--widgets-demo") {
        Application::new().run(|cx: &mut App| {
            ui::fonts::register(cx);
            ui::widgets::demo::open_demo(cx);
        });
        return;
    }
    // `archonui <files>...` opens a new chat with those files attached. `--new-ui` is accepted
    // for scripts written while the new UI was opt-in.
    let files: Vec<PathBuf> = args
        .iter()
        .filter(|a| !matches!(a.as_str(), "--old-ui" | "--new-ui"))
        .filter_map(|a| std::path::absolute(a).ok())
        .collect();
    if !old_ui {
        Application::new().run(move |cx: &mut App| {
            ui::shell::open_window(cx, files);
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
        });
        return;
    }
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(780.)), cx);
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| chat::ChatView::new(working_dir(), files, cx)),
            )
            .expect("open window");
        window
            .update(cx, |view, window, cx| {
                window.focus(&view.focus);
                cx.activate(true);
            })
            .ok();
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
