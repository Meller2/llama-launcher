//! Локальный чат: прокси к OpenAI-совместимому API llama-server.
//! Стрим токенов → событие `chat-delta`, завершение → `chat-done` / `chat-error`.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChatDelta {
    /// Накопительный фрагмент (дельта content).
    delta: String,
    done: bool,
    error: Option<String>,
}

#[derive(Default)]
pub struct ChatState {
    /// Отмена текущего стрима.
    cancel: AtomicBool,
    /// Уже идёт генерация.
    active: Mutex<bool>,
}

impl ChatState {
    fn lock_active(&self) -> std::sync::MutexGuard<'_, bool> {
        self.active.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        // Генерация может идти долго — без общего timeout на body.
        .build()
        .map_err(|e| format!("HTTP-клиент: {e}"))
}

fn emit_delta(app: &AppHandle, delta: &str, done: bool, error: Option<String>) {
    let _ = app.emit(
        "chat-delta",
        ChatDelta {
            delta: delta.to_string(),
            done,
            error,
        },
    );
}

/// Стрим chat completions с llama-server (SSE OpenAI-формата).
#[tauri::command]
pub async fn chat_stream(
    app: AppHandle,
    state: State<'_, ChatState>,
    port: u16,
    messages: Vec<ChatMessage>,
) -> Result<(), String> {
    if port == 0 {
        return Err("Сервер не запущен (порт 0).".into());
    }
    if messages.is_empty() {
        return Err("Нет сообщений.".into());
    }
    {
        let mut active = state.lock_active();
        if *active {
            return Err("Уже идёт генерация ответа.".into());
        }
        *active = true;
    }
    state.cancel.store(false, Ordering::SeqCst);

    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    let body = serde_json::json!({
        "model": "local",
        "messages": messages,
        "stream": true,
        "temperature": 0.7,
    });

    let result = do_stream(&app, &state, &url, &body).await;

    *state.lock_active() = false;

    match result {
        Ok(()) => {
            emit_delta(&app, "", true, None);
            Ok(())
        }
        Err(e) if e == "canceled" => {
            emit_delta(&app, "", true, None);
            Err("Генерация отменена.".into())
        }
        Err(e) => {
            emit_delta(&app, "", true, Some(e.clone()));
            Err(e)
        }
    }
}

async fn do_stream(
    app: &AppHandle,
    state: &ChatState,
    url: &str,
    body: &serde_json::Value,
) -> Result<(), String> {
    let resp = client()?
        .post(url)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("Не удалось связаться с llama-server: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!(
            "llama-server ответил {status}: {}",
            text.chars().take(200).collect::<String>()
        ));
    }

    use futures_util::StreamExt;
    let mut stream = resp.bytes_stream();
    let mut buf = String::new();

    while let Some(chunk) = stream.next().await {
        if state.cancel.load(Ordering::SeqCst) {
            return Err("canceled".into());
        }
        let chunk = chunk.map_err(|e| format!("Ошибка чтения потока: {e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        // SSE: блоки, разделённые \n\n
        while let Some(idx) = buf.find("\n\n") {
            let block = buf[..idx].to_string();
            buf = buf[idx + 2..].to_string();
            for line in block.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with(':') {
                    continue;
                }
                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data == "[DONE]" {
                    return Ok(());
                }
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
                    // OpenAI: choices[0].delta.content
                    if let Some(content) = v
                        .pointer("/choices/0/delta/content")
                        .and_then(|c| c.as_str())
                    {
                        if !content.is_empty() {
                            emit_delta(app, content, false, None);
                        }
                    }
                    // error object
                    if let Some(err) = v.get("error") {
                        let msg = err
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("ошибка модели");
                        return Err(msg.to_string());
                    }
                }
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn chat_cancel(state: State<ChatState>) {
    state.cancel.store(true, Ordering::SeqCst);
}
