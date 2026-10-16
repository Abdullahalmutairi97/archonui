//! The stream demo window: a rich seeded transcript in Blood Moon, for comparing the stream with
//! `.qa/design/chat-*.png`. Meant to be opened from a debug entry point (`--stream-demo`).

use super::{StreamEnv, StreamHost, StreamUi};
use crate::{
    engine::attach::{Attachment, Kind},
    store::{Message, Role, Session, ToolCall, ToolKind},
    ui::{
        fonts::FontChoice,
        theme::{resolve, Theme},
        widgets::Toasts,
    },
};
use gpui::{div, prelude::*, App, Context, Entity, IntoElement, Render, Window, WindowOptions};
use std::{collections::HashMap, path::{Path, PathBuf}};

/// Opens the stream demo in a new window, in the Blood Moon theme.
pub fn stream_demo(cx: &mut App) {
    let theme = resolve(crate::ui::theme::preset(crate::ui::theme::DEFAULT_THEME), &HashMap::new(), FontChoice::default());
    cx.open_window(WindowOptions::default(), move |_window, cx| cx.new(|cx| DemoView::new(theme, cx)))
        .expect("open stream demo window");
}

struct DemoView {
    theme: Theme,
    stream: StreamUi,
    session: Session,
    toasts: Entity<Toasts>,
}

impl DemoView {
    fn new(theme: Theme, cx: &mut Context<Self>) -> Self {
        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let mut session = Session::new("demo", cwd.clone());
        session.title = "Cache hit rate".into();
        session.messages = seed(&cwd);
        let toasts = cx.new(|_| Toasts::new(&theme));
        Self { theme, stream: StreamUi::default(), session, toasts }
    }
}

impl Render for DemoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let env = StreamEnv {
            theme: &self.theme,
            app_name: "ArchonUI",
            text_size: 15.,
            session: &self.session,
            busy: false,
            show_tools: true,
            place: "ArchonUI · Sonnet".into(),
        };
        let stream = self.stream.render(cx.entity(), env, cx);
        let lightbox = self.stream.lightbox_overlay(cx.entity(), &self.theme);
        div()
            .size_full()
            .relative()
            .bg(self.theme.bg.to_hsla())
            .flex()
            .flex_col()
            .child(stream)
            .children(lightbox)
            .child(self.toasts.clone())
    }
}

impl StreamHost for DemoView {
    fn stream_ui(&mut self) -> &mut StreamUi {
        &mut self.stream
    }

    fn stream_toast(&self, message: String, cx: &mut Context<Self>) {
        self.toasts.update(cx, |toasts, cx| toasts.show(message, cx));
    }

    /// The demo has no composer to attach to; the stream's own "Attached …" toast still shows.
    fn stream_attach(&mut self, _session: &str, _files: Vec<Attachment>, _cx: &mut Context<Self>) {}

    fn stream_open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        self.stream_toast(format!("Opening {url} isn't wired yet"), cx);
    }
}

/// The seeded transcript: a user question with an attachment, a reply with every block kind, a short follow-up,
/// and an error.
fn seed(cwd: &Path) -> Vec<Message> {
    let tool = |kind, target: &str, detail: Option<&str>, ok| ToolCall {
        kind,
        target: target.into(),
        detail: detail.map(str::to_string),
        ok,
    };
    vec![
        Message {
            role: Role::User,
            text: "Why did the cache hit rate drop after the deploy? Show me the hot path.".into(),
            attachments: vec![Attachment {
                path: cwd.join("src/server/session.ts"),
                name: "session.ts".into(),
                kind: Kind::Text,
                size: 486,
            }],
            tools: Vec::new(),
            thought_secs: None, took_secs: None, agent: None, final_at: None, usage: None, todos: Vec::new(),
        },
        Message {
            role: Role::Agent,
            text: REPLY.into(),
            attachments: Vec::new(),
            tools: vec![
                tool(ToolKind::Read, "src/server/session.ts", None, Some(true)),
                tool(ToolKind::Grep, "\"verify(\" in src/", Some("7 matches"), Some(true)),
                tool(ToolKind::Edit, "src/server/session.ts", Some("+24 −8"), Some(true)),
                tool(ToolKind::Edit, "src/server/cache.ts", Some("+41"), Some(true)),
                tool(ToolKind::Run, "pnpm test", Some("128 passed"), Some(true)),
                tool(ToolKind::Run, "pnpm e2e upload.spec.ts --repeat 20", Some("3 failed"), Some(false)),
                tool(ToolKind::Run, "git log v0.8..HEAD --oneline", Some("23 commits"), Some(true)),
            ],
            thought_secs: Some(14), took_secs: None, agent: None, final_at: None, usage: None, todos: Vec::new(),
        },
        Message {
            role: Role::User,
            text: "Revert the key change and rerun the e2e suite.".into(),
            attachments: Vec::new(),
            tools: Vec::new(),
            thought_secs: None, took_secs: None, agent: None, final_at: None, usage: None, todos: Vec::new(),
        },
        Message {
            role: Role::Error,
            text: "pnpm e2e failed: 3 specs timed out after 30s.".into(),
            attachments: Vec::new(),
            tools: Vec::new(),
            thought_secs: None, took_secs: None, agent: None, final_at: None, usage: None, todos: Vec::new(),
        },
    ]
}

const REPLY: &str = r##"The **TokenCache** key now includes `session.id`, so every reconnect misses. Revert that part in `src/server/cache.ts`:

```ts src/server/cache.ts
import { TokenCache } from "./cache";

export function get(key: string) {
  // Keys are per session again
  const hit = TokenCache.get(key) ?? 300;
  return hit > 0 ? "cache/" + key : null;
}
```

Hit rate per hour on staging since the deploy:

```chart Cache hit rate (%) · staging
09:00 38
10:00 61
11:00 74
12:00 82
13:00 87
14:00 91
```

## Next steps

1. Revert the key change in `cache.ts`
2. Re-run `pnpm e2e upload.spec.ts --repeat 20`
3. Compare with the baseline in assets/themes/wallpapers/bonfire.jpg

```image assets/themes/wallpapers/bonfire.jpg bonfire-frame-240.png
bonfire.wgsl · frame 240 · 1920×1080
```

| Run | Hits | Misses |
|---|---|---|
| before | 38 | 62 |
| after | 91 | 9 |

> Staging only; production is still at 52%.

- Keys are scoped per session
- Hits are measured per hour
  - Misses come from reconnects only

---

Docs: [session cache](https://example.com/docs/session-cache). Preview: ![preview](https://example.com/preview.png)

Missing shot: ![gone](assets/missing/shot.png)
"##;
