//! A closed settings window must not keep a WebView2 renderer alive.
//! Call with_window from a worker, never a synchronous Windows event handler.
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime, WebviewWindow, WebviewWindowBuilder};

static CREATION_LOCK: Mutex<()> = Mutex::new(());

pub fn with_window<R: Runtime, T>(
    app: &AppHandle<R>,
    action: impl FnOnce(&WebviewWindow<R>, bool) -> T,
) -> Result<T, String> {
    // Tray clicks, dock clicks and the second-instance signal can arrive together.
    let _guard = CREATION_LOCK.lock().map_err(|e| e.to_string())?;
    if crate::state::SHUTTING_DOWN.load(std::sync::atomic::Ordering::Relaxed) {
        return Err("Bloom is shutting down".into());
    }
    if let Some(window) = app.get_webview_window("settings") {
        return Ok(action(&window, false));
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|window| window.label == "settings")
        .ok_or_else(|| "Missing settings window template".to_string())?;
    let window = WebviewWindowBuilder::from_config(app, config)
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?;
    // Initialize and show while still holding the lock, so close cannot destroy
    // the window between creation and the caller's appearance/focus operations.
    Ok(action(&window, true))
}

pub fn close<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let _guard = CREATION_LOCK.lock().map_err(|e| e.to_string())?;
    if let Some(window) = app.get_webview_window("settings") {
        window.destroy().map_err(|e| e.to_string())?;
        // Destruction is dispatched to the UI thread. Do not let a fast reopen
        // reuse the dying window while its label is still registered.
        let started = std::time::Instant::now();
        while app.get_webview_window("settings").is_some() {
            if started.elapsed() > std::time::Duration::from_secs(5) {
                return Err("Settings window did not finish closing".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn closed_settings_does_not_load_a_webview_at_startup() {
        let config: tauri::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let settings = config
            .app
            .windows
            .iter()
            .find(|w| w.label == "settings")
            .unwrap();
        assert!(!settings.create);
        assert!(!settings.visible);
    }

    fn get_or_create<R: Runtime>(app: &AppHandle<R>) -> (WebviewWindow<R>, bool) {
        with_window(app, |window, created| (window.clone(), created)).unwrap()
    }

    // This uses real WebView2, but only hidden about:blank windows in a separate
    // application profile. It never opens or clicks the user's Bloom windows.
    #[test]
    #[ignore = "real hidden WebView2 lifecycle; run explicitly"]
    fn settings_can_be_created_reused_destroyed_and_reopened() {
        let mut context = tauri::generate_context!();
        context.config_mut().identifier = "com.sehaz.bloom.settings-lifecycle-test".into();
        context
            .config_mut()
            .app
            .windows
            .retain(|w| w.label == "settings");
        let config = &mut context.config_mut().app.windows[0];
        config.url = tauri::WebviewUrl::External("about:blank".parse().unwrap());
        config.visible = false;
        config.focus = false;
        let app = tauri::Builder::default()
            .any_thread()
            .setup(|app| {
                tauri::WebviewWindowBuilder::new(
                    app,
                    "test-keepalive",
                    tauri::WebviewUrl::External("about:blank".parse().unwrap()),
                )
                .visible(false)
                .focused(false)
                .build()?;
                Ok(())
            })
            .build(context)
            .unwrap();
        let handle = app.handle().clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                assert!(handle.get_webview_window("settings").is_none());
                // Closing an already closed settings window is harmless.
                close(&handle).unwrap();
                for _ in 0..3 {
                    let (first, created) = get_or_create(&handle);
                    assert!(created);
                    assert!(!first.is_visible().unwrap());
                    let (same, created_again) = get_or_create(&handle);
                    assert!(!created_again);
                    assert_eq!(first.hwnd().unwrap(), same.hwnd().unwrap());
                    assert_eq!(handle.webview_windows().len(), 2);
                    drop(first);
                    drop(same);
                    close(&handle).unwrap();
                    let started = Instant::now();
                    while handle.get_webview_window("settings").is_some() {
                        assert!(
                            started.elapsed() < Duration::from_secs(10),
                            "Settings renderer did not close"
                        );
                        std::thread::sleep(Duration::from_millis(25));
                    }
                    assert_eq!(handle.webview_windows().len(), 1);
                }

                // Simultaneous dock/tray/second-instance requests create once.
                let barrier = std::sync::Arc::new(std::sync::Barrier::new(4));
                let requests: Vec<_> = (0..4)
                    .map(|_| {
                        let handle = handle.clone();
                        let barrier = barrier.clone();
                        std::thread::spawn(move || {
                            barrier.wait();
                            with_window(&handle, |window, created| {
                                (window.hwnd().unwrap().0 as usize, created)
                            })
                            .unwrap()
                        })
                    })
                    .collect();
                let results: Vec<_> = requests.into_iter().map(|r| r.join().unwrap()).collect();
                assert_eq!(results.iter().filter(|(_, created)| *created).count(), 1);
                assert!(results.iter().all(|(hwnd, _)| *hwnd == results[0].0));

                // Either serialized ordering is valid, but the next open must
                // always yield a live window rather than a dying registered label.
                for _ in 0..3 {
                    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
                    let close_handle = handle.clone();
                    let close_barrier = barrier.clone();
                    let closing = std::thread::spawn(move || {
                        close_barrier.wait();
                        close(&close_handle).unwrap();
                    });
                    barrier.wait();
                    with_window(&handle, |window, _| assert!(window.hwnd().is_ok())).unwrap();
                    closing.join().unwrap();
                    with_window(&handle, |window, _| assert!(window.hwnd().is_ok())).unwrap();
                    assert_eq!(handle.webview_windows().len(), 2);
                }
                close(&handle).unwrap();
            }));
            tx.send(result.is_ok()).unwrap();
            handle.exit(0);
        });
        let exit_code = app.run_return(|_, _| {});
        assert_eq!(exit_code, 0);
        worker.join().unwrap();
        assert!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "Hidden settings lifecycle failed"
        );
    }
}
