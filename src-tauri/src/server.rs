//! Жизненный цикл llama-server.exe: запуск, остановка, статус, стриминг лога.
//! Флаги — маппинг из llama.bat + дефолты из config::LaunchDefaults.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

/// Сколько ждать готовности сервера, прежде чем сообщить UI об ошибке.
const READY_TIMEOUT: Duration = Duration::from_secs(180);

/// Контекст: разумные границы для llama-server (не HTML-валидация).
const CTX_MIN: u32 = 256;
const CTX_MAX: u32 = 131_072;
const THREADS_MIN: u32 = 1;
const THREADS_MAX: u32 = 512;
const NGL_MAX: u32 = 10_000;

/// Конфиг запуска, приходит из UI.
#[derive(Debug, Clone, Deserialize)]
pub struct LaunchConfig {
    /// Папка с llama-server.exe.
    pub llama_dir: String,
    /// Полный путь к .gguf.
    pub model_path: String,
    pub ctx: u32,
    /// "f16" | "q8_0" | "q4_0".
    pub kv_quant: String,
    pub threads: u32,
    pub ngl: u32,
    pub port: u16,
    pub tools: bool,
}

/// Статус для UI.
#[derive(Debug, Clone, Serialize)]
pub struct ServerStatus {
    pub running: bool,
    /// Модель загружена и сервер слушает (не только процесс жив).
    pub ready: bool,
    pub port: Option<u16>,
    pub model_name: Option<String>,
}

/// Состояние запущенного сервера (живёт в Tauri-managed state).
/// Child уходит в поток-наблюдатель; здесь храним только pid + метаданные + id поколения.
#[derive(Default)]
pub struct ServerState {
    inner: Mutex<Option<RunningServer>>,
    next_id: AtomicU64,
    /// Код выхода последнего завершившегося процесса (диагностика).
    last_exit: Mutex<Option<i32>>,
}

impl ServerState {
    /// Взять lock, устойчиво к «отравлению» (паника другого потока не должна
    /// класть всё приложение — восстанавливаем внутреннее значение).
    fn lock(&self) -> MutexGuard<'_, Option<RunningServer>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn lock_last_exit(&self) -> MutexGuard<'_, Option<i32>> {
        self.last_exit.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Код выхода последнего завершившегося llama-server (для диагностики).
    pub fn last_exit_code(&self) -> Option<i32> {
        *self.lock_last_exit()
    }
}

struct RunningServer {
    pid: u32,
    port: u16,
    model_name: String,
    /// Поколение — чтобы наблюдатель не затирал состояние более нового запуска.
    id: u64,
    /// Готовность (log «listening» или /health). Делится с watchdog/reader.
    ready: Arc<AtomicBool>,
}

// Windows: не открывать консольное окно у дочернего процесса.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Валидация параметров запуска на backend (HTML/UI не считается защитой).
fn validate_launch_config(cfg: &LaunchConfig) -> Result<(), String> {
    if cfg.llama_dir.trim().is_empty() {
        return Err("Путь к llama-server не задан.".into());
    }
    if cfg.model_path.trim().is_empty() {
        return Err("Путь к модели не задан.".into());
    }
    if !(CTX_MIN..=CTX_MAX).contains(&cfg.ctx) {
        return Err(format!(
            "Контекст (ctx) должен быть в диапазоне {CTX_MIN}…{CTX_MAX}, получено {}.",
            cfg.ctx
        ));
    }
    if !(THREADS_MIN..=THREADS_MAX).contains(&cfg.threads) {
        return Err(format!(
            "Потоки (threads) должны быть в диапазоне {THREADS_MIN}…{THREADS_MAX}, получено {}.",
            cfg.threads
        ));
    }
    if cfg.ngl > NGL_MAX {
        return Err(format!(
            "Число слоёв GPU (ngl) слишком велико (>{NGL_MAX}): {}.",
            cfg.ngl
        ));
    }
    match cfg.kv_quant.as_str() {
        "f16" | "q8_0" | "q4_0" => {}
        other => {
            return Err(format!(
                "Недопустимый KV-квант «{other}». Допустимо: f16, q8_0, q4_0."
            ));
        }
    }
    if cfg.port == 0 {
        return Err("Порт не может быть 0.".into());
    }
    // Привилегированные порты на Windows обычно доступны, но <1024 часто заняты —
    // не запрещаем, только 0 уже отсечён (u16 max ок).
    Ok(())
}

/// Собрать аргументы llama-server из конфига (портирует build_command + llama.bat).
fn build_args(cfg: &LaunchConfig) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-m".into(),
        cfg.model_path.clone(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        cfg.port.to_string(),
        "-c".into(),
        cfg.ctx.to_string(),
        "--flash-attn".into(),
        "on".into(),
        "-ngl".into(),
        cfg.ngl.to_string(),
        "-t".into(),
        cfg.threads.to_string(),
        "-ub".into(),
        "1024".into(),
        "-b".into(),
        "1024".into(),
        // Чистый лог для парсинга в UI.
        "--log-colors".into(),
        "off".into(),
        "--no-log-prefix".into(),
    ];

    // KV-квант: f16 = без флагов (дефолт llama.cpp). q8_0/q4_0 требуют flash-attn (включён).
    if cfg.kv_quant != "f16" {
        args.push("--cache-type-k".into());
        args.push(cfg.kv_quant.clone());
        args.push("--cache-type-v".into());
        args.push(cfg.kv_quant.clone());
    }

    args.push("--jinja".into());

    if cfg.tools {
        args.push("--tools".into());
        args.push("all".into());
        args.push("--ui-mcp-proxy".into());
    }

    args
}

/// Проверить, свободен ли TCP-порт на 127.0.0.1.
fn port_available(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Убить процесс (и всё его дерево на Windows). Используется и при стопе, и при
/// закрытии приложения, поэтому вынесено отдельно.
fn kill_pid(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = Command::new("kill").arg(pid.to_string()).output();
    }
}

/// Убить сервер, если он ещё запущен. Вызывается при закрытии окна приложения,
/// чтобы llama-server.exe не остался осиротевшим и не держал порт/VRAM.
pub fn shutdown(state: &ServerState) {
    if let Some(s) = state.lock().take() {
        kill_pid(s.pid);
    }
}

/// Помечает готовность: взводит флаг и шлёт `server-ready` ровно один раз.
fn mark_ready(app: &AppHandle, ready: &AtomicBool, port: u16) {
    // swap → true возвращает прежнее значение; шлём событие только на первом переходе.
    if !ready.swap(true, Ordering::SeqCst) {
        let _ = app.emit("server-ready", port);
    }
}

/// HTTP GET /health — надёжнее голого TCP (чужой процесс на порту ≠ llama-server).
/// llama-server отдаёт 200, когда модель загружена; иначе 503/ошибка.
fn http_health_ok(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_millis(400)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_millis(600)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(400)));
    let req =
        format!("GET /health HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 256];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return false,
    };
    let head = String::from_utf8_lossy(&buf[..n]);
    // "HTTP/1.x 200"
    head.lines()
        .next()
        .map(|l| l.contains(" 200"))
        .unwrap_or(false)
}

/// Читать поток построчно и слать события `server-log`. Детектить готовность.
fn stream_reader<R: std::io::Read + Send + 'static>(
    app: AppHandle,
    reader: R,
    port: u16,
    ready: Arc<AtomicBool>,
) {
    std::thread::spawn(move || {
        let buf = BufReader::new(reader);
        for line in buf.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            // Готовность: llama-server печатает "... listening on http://127.0.0.1:PORT".
            let lower = line.to_lowercase();
            if lower.contains("listening") && lower.contains(&port.to_string()) {
                mark_ready(&app, &ready, port);
            }
            let _ = app.emit("server-log", line);
        }
    });
}

/// Страховочный сторож: если лог не поймал «listening», пробуем HTTP /health
/// (не голый TCP — иначе любой процесс на порту даст ложный ready).
fn spawn_ready_watchdog(app: AppHandle, port: u16, id: u64, ready: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if ready.load(Ordering::SeqCst) {
                return; // уже готов (лог поймал раньше)
            }
            if http_health_ok(port) {
                // Убедимся, что это всё ещё наш запуск.
                let st = app.state::<ServerState>();
                let is_current = st.lock().as_ref().map(|s| s.id == id).unwrap_or(false);
                if is_current {
                    mark_ready(&app, &ready, port);
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        // Дедлайн вышел, готовности нет. Сообщаем только если это всё ещё наше поколение.
        if ready.load(Ordering::SeqCst) {
            return;
        }
        let st = app.state::<ServerState>();
        let is_current = st.lock().as_ref().map(|s| s.id == id).unwrap_or(false);
        if is_current {
            let _ = app.emit(
                "server-timeout",
                format!(
                    "Сервер не запустился за {} с. Возможно, не хватает памяти или модель не загрузилась — смотрите лог.",
                    READY_TIMEOUT.as_secs()
                ),
            );
        }
    });
}

fn status_from(opt: Option<&RunningServer>) -> ServerStatus {
    match opt {
        Some(s) => ServerStatus {
            running: true,
            ready: s.ready.load(Ordering::SeqCst),
            port: Some(s.port),
            model_name: Some(s.model_name.clone()),
        },
        None => ServerStatus {
            running: false,
            ready: false,
            port: None,
            model_name: None,
        },
    }
}

// ── Tauri-команды ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn start_server(
    app: AppHandle,
    state: State<ServerState>,
    config: LaunchConfig,
) -> Result<ServerStatus, String> {
    let mut guard = state.lock();
    if guard.is_some() {
        return Err("Сервер уже запущен. Сначала остановите его.".into());
    }

    validate_launch_config(&config)?;

    // Валидация путей и порта (раннее обнаружение проблем).
    // Примечание: проверка порта → spawn = TOCTOU; если между проверкой и bind
    // кто-то займёт порт, llama-server упадёт сам с понятной ошибкой в логе.
    let exe = Path::new(&config.llama_dir).join("llama-server.exe");
    if !exe.is_file() {
        return Err(format!("llama-server.exe не найден в {}", config.llama_dir));
    }
    if !Path::new(&config.model_path).is_file() {
        return Err(format!("Файл модели не найден: {}", config.model_path));
    }
    if !port_available(config.port) {
        return Err(format!(
            "Порт {} уже занят. Закройте другой сервер или смените порт в настройках.",
            config.port
        ));
    }

    let model_name = Path::new(&config.model_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("model")
        .to_string();

    let args = build_args(&config);
    let _ = app.emit("server-log", format!("$ llama-server {}", args.join(" ")));

    let mut cmd = Command::new(&exe);
    cmd.args(&args)
        .current_dir(&config.llama_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child: Child = cmd
        .spawn()
        .map_err(|e| format!("Не удалось запустить llama-server: {e}"))?;

    let pid = child.id();
    let id = state.next_id.fetch_add(1, Ordering::SeqCst);
    let ready = Arc::new(AtomicBool::new(false));

    // Стримим оба потока (забираем пайпы у child).
    if let Some(out) = child.stdout.take() {
        stream_reader(app.clone(), out, config.port, ready.clone());
    }
    if let Some(err) = child.stderr.take() {
        stream_reader(app.clone(), err, config.port, ready.clone());
    }

    // Сторож готовности: таймаут + HTTP /health, если лог не поймал «listening».
    spawn_ready_watchdog(app.clone(), config.port, id, ready.clone());

    // Поток-наблюдатель владеет Child и ждёт его завершения (в т.ч. самопадение/краш).
    {
        let app_mon = app.clone();
        std::thread::spawn(move || {
            let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
            // Снять состояние, только если это всё ещё наше поколение.
            let st = app_mon.state::<ServerState>();
            let mut g = st.lock();
            let is_current = g.as_ref().map(|s| s.id == id).unwrap_or(false);
            if is_current {
                *g = None;
                *st.lock_last_exit() = Some(code);
            }
            drop(g);
            let _ = app_mon.emit("server-exit", code);
        });
    }

    *guard = Some(RunningServer {
        pid,
        port: config.port,
        model_name: model_name.clone(),
        id,
        ready: ready.clone(),
    });

    Ok(ServerStatus {
        running: true,
        ready: false,
        port: Some(config.port),
        model_name: Some(model_name),
    })
}

#[tauri::command]
pub fn stop_server(app: AppHandle, state: State<ServerState>) -> Result<(), String> {
    let server = {
        let mut guard = state.lock();
        match guard.take() {
            Some(s) => s,
            None => return Ok(()), // уже остановлен
        }
    };

    // Убиваем всё дерево процессов. Наблюдатель, увидев смерть процесса, эмитит server-exit.
    kill_pid(server.pid);

    let _ = app.emit("server-log", "— Остановка сервера —".to_string());
    Ok(())
}

#[tauri::command]
pub fn server_status(state: State<ServerState>) -> ServerStatus {
    status(&state)
}

/// Текущий статус сервера (без Tauri State-обёртки — для переиспользования из diagnostics).
pub fn status(state: &ServerState) -> ServerStatus {
    let guard = state.lock();
    status_from(guard.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_cfg() -> LaunchConfig {
        LaunchConfig {
            llama_dir: "C:\\llama".into(),
            model_path: "C:\\m.gguf".into(),
            ctx: 4096,
            kv_quant: "q4_0".into(),
            threads: 4,
            ngl: 32,
            port: 8080,
            tools: false,
        }
    }

    #[test]
    fn validate_accepts_sane_config() {
        assert!(validate_launch_config(&base_cfg()).is_ok());
    }

    #[test]
    fn validate_rejects_bad_kv() {
        let mut c = base_cfg();
        c.kv_quant = "q3_K".into();
        assert!(validate_launch_config(&c).is_err());
    }

    #[test]
    fn validate_rejects_ctx_out_of_range() {
        let mut c = base_cfg();
        c.ctx = 10;
        assert!(validate_launch_config(&c).is_err());
        c.ctx = 200_000;
        assert!(validate_launch_config(&c).is_err());
    }

    #[test]
    fn validate_rejects_zero_port() {
        let mut c = base_cfg();
        c.port = 0;
        assert!(validate_launch_config(&c).is_err());
    }

    #[test]
    fn validate_rejects_huge_ngl() {
        let mut c = base_cfg();
        c.ngl = NGL_MAX + 1;
        assert!(validate_launch_config(&c).is_err());
    }
}
