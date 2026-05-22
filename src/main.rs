mod app;
mod assets;
mod components;
mod core;
mod layout;
mod logging;
mod pages;
mod runtime;
mod services;

use gpui::*;
use assets::CombinedAssets;
use layout::AppLayout;

fn main() {
    let _log_guard = logging::init();

    // Capture panics into the log file so we can debug crashes.
    // We write directly (synchronously) to the log file as a fallback,
    // because tracing-appender is non-blocking and may lose buffered
    // messages if the process aborts.
    std::panic::set_hook(Box::new(|info| {
        let location = info.location().map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "<unknown>".to_string());
        let payload = info.payload();
        let msg = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_string()
        };

        let backtrace = std::backtrace::Backtrace::force_capture();
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let line = format!(
            "[PANIC] ts={} at {}: {}\nbacktrace:\n{}\n",
            now_secs,
            location,
            msg,
            backtrace,
        );

        // 1) Best-effort write into tracing (may be lost if we abort).
        tracing::error!(location = %location, message = %msg, "PANIC");

        // 2) Synchronous write directly to today's log file.
        let path = logging::log_dir().join(format!("clashr.panic.log"));
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
            let _ = f.flush();
        }

        // 3) Print to stderr too.
        eprintln!("{}", line);
    }));

    tracing::info!("ClashR starting");

    let app = gpui_platform::application().with_assets(CombinedAssets::new());

    app.run(move |cx| {
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(960.), px(680.)), cx)),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| AppLayout::new(window, cx));
                cx.new(|cx| gpui_component::Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}
