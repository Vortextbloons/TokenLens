//! In-app Cursor login via a dedicated webview window.

use crate::collectors::cursor::api::CursorClient;
use crate::collectors::cursor::{auth, sync_inner};
use crate::errors::{AppError, AppResult};
use crate::scan;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tracing::{info, warn};

const LOGIN_LABEL: &str = "cursor-login";
const LOGIN_URL: &str = "https://cursor.com/dashboard?tab=usage";
const COOKIE_NAME: &str = "WorkosCursorSessionToken";

static LOGIN_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Open the Cursor login webview. Returns once the window is shown; cookie polling
/// continues in the background until sign-in completes or the window closes.
pub async fn start_login(app: AppHandle) -> AppResult<()> {
    if LOGIN_IN_PROGRESS
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err(AppError::Invalid("Cursor login already in progress".into()));
    }

    if let Some(w) = app.get_webview_window(LOGIN_LABEL) {
        let _ = w.set_focus();
        let _ = w.show();
        LOGIN_IN_PROGRESS.store(false, Ordering::SeqCst);
        return Ok(());
    }

    let app_bg = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = open_login_window(app_bg.clone()).await {
            warn!("Cursor login window failed: {e}");
            let _ = app_bg.emit("cursor-login-error", e.to_string());
        }
        LOGIN_IN_PROGRESS.store(false, Ordering::SeqCst);
    });

    Ok(())
}

async fn open_login_window(app: AppHandle) -> AppResult<()> {
    let login_url = LOGIN_URL
        .parse()
        .map_err(|e| AppError::Internal(format!("login url: {e}")))?;

    let window = WebviewWindowBuilder::new(
        &app,
        LOGIN_LABEL,
        WebviewUrl::External(login_url),
    )
    .title("Sign in to Cursor")
    .inner_size(520.0, 780.0)
    .min_inner_size(400.0, 600.0)
    .center()
    .visible(true)
    .focused(true)
    .resizable(true)
    .background_color(tauri::webview::Color(255, 255, 255, 255))
    .build()?;

    let done = Arc::new(AtomicBool::new(false));
    let done_poll = done.clone();
    let app_poll = app.clone();

    tauri::async_runtime::spawn(async move {
        for _ in 0..600 {
            if done_poll.load(Ordering::SeqCst) {
                return;
            }
            if let Some(w) = app_poll.get_webview_window(LOGIN_LABEL) {
                match try_extract_token(&w) {
                    Ok(Some(token)) => {
                        if connect_with_token(&app_poll, &token).await.is_ok() {
                            done_poll.store(true, Ordering::SeqCst);
                            let _ = w.close();
                            let _ = app_poll.emit("cursor-login-success", ());
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!("Cursor cookie poll error: {e}");
                    }
                }
            } else {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        warn!("Cursor login timed out waiting for session cookie");
        let _ = app_poll.emit(
            "cursor-login-error",
            "Sign-in timed out. Try Advanced → paste session token.".to_string(),
        );
    });

    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            done.store(true, Ordering::SeqCst);
        }
    });

    let _ = window.set_focus();
    Ok(())
}

fn try_extract_token(window: &tauri::WebviewWindow) -> AppResult<Option<String>> {
    let cookies = window
        .cookies()
        .map_err(|e| AppError::Internal(e.to_string()))?;
    for c in cookies {
        if c.name() == COOKIE_NAME {
            let value = c.value().to_string();
            if !value.is_empty() {
                return Ok(Some(value));
            }
        }
    }
    Ok(None)
}

/// Connect using a session token (webview capture or manual paste).
pub async fn connect_with_token(app: &AppHandle, token: &str) -> AppResult<()> {
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::Invalid("Empty Cursor session token".into()));
    }

    // Validate against the API before persisting credentials.
    let provisional = auth::CursorCredentials::provisional(token);
    let client = CursorClient::new(&provisional)?;
    let summary = client.validate().await?;
    let label = summary
        .membership_type
        .as_deref()
        .map(|m| format!("Cursor ({m})"))
        .or_else(|| Some("Cursor account".to_string()));

    auth::save(token, None, None, label.as_deref())?;

    info!("Cursor account connected");
    let _ = app.emit("cursor-connected", ());

    let app_bg = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = scan::run_exclusive_blocking_async(|| sync_inner(true)).await {
            warn!("Initial Cursor sync failed: {e}");
            let _ = app_bg.emit("cursor-sync-error", e.to_string());
        } else {
            let _ = app_bg.emit("cursor-sync-complete", ());
        }
    });

    Ok(())
}

/// Manual token connect (Advanced fallback).
pub async fn connect_manual(app: AppHandle, token: String) -> AppResult<()> {
    connect_with_token(&app, &token).await
}
