//! 진단 로그 — 기본 꺼짐. 배지가 뜨거나 숨는 **이유**를 파일에 남긴다.
//!
//! 데몬은 콘솔 없는 GUI 앱(`windows_subsystem = "windows"`)이고 로그인 시
//! 자동 실행되므로, stderr 대신 파일에 쓴다. 켜는 방법 두 가지:
//! - `config.txt`에 `debug_log = on` (자동 실행되는 데몬용 — 재시작 필요)
//! - 환경변수 `HANENG_DEBUG=1` (직접 실행할 때)
//!
//! **입력 내용은 절대 기록하지 않는다** — 컨트롤 종류·좌표·판정 결과만 남긴다.
//! (요소 이름은 사용자가 입력한 글자일 수 있어 기록하지 않는다.)

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

/// 로그 파일 상한 — 넘으면 지우고 새로 쓴다(상주 데몬이라 무한정 커지면 안 된다).
const MAX_BYTES: u64 = 1_000_000;

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var_os("HANENG_DEBUG").is_some()
            || haneng_core::config::load_config().extra("debug_log") == Some("on")
    })
}

fn log_path() -> Option<PathBuf> {
    haneng_core::config::config_dir().map(|dir| dir.join("debug.log"))
}

/// 진단 한 줄. 직전과 **같은 내용이면 건너뛴다** — 폴링이 초당 6~7회라
/// 상태가 그대로일 때 같은 줄이 파일을 채우는 것을 막는다.
pub fn log(msg: &str) {
    if !enabled() {
        return;
    }
    static LAST: Mutex<String> = Mutex::new(String::new());
    let Ok(mut last) = LAST.lock() else { return };
    if *last == msg {
        return;
    }
    last.clear();
    last.push_str(msg);
    drop(last);

    let Some(path) = log_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        let _ = std::fs::remove_file(&path);
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let t: SYSTEMTIME = unsafe {
        let mut t = std::mem::zeroed();
        GetLocalTime(&mut t);
        t
    };
    let _ = writeln!(
        file,
        "{:02}:{:02}:{:02}.{:03} {msg}",
        t.wHour, t.wMinute, t.wSecond, t.wMilliseconds
    );
}
