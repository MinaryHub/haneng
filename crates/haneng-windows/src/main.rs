//! hanengw — 한/영 상태 표시기 (Windows 상주 데몬).
//!
//! 마우스가 텍스트 입력(I-빔 커서) 위에 있으면 커서 옆에 현재 IME
//! 모드("한"/"A") 배지를 표시한다. 그게 전부다:
//!
//! - **키보드 후킹 없음** — 모드는 포커스 창 IME에 실시간 질의
//!   (IMC_GETOPENSTATUS)로 읽고, 마우스 이동만 관찰한다.
//! - **텍스트 조작 없음** — 아무것도 지우거나 입력하지 않는다.
//!   (변환 기능은 v0.4.0에서 제거 — 이력은 git 태그 v0.3.2 참고)
//! - 설정: `%APPDATA%\haneng\config.txt` — `hover_indicator = off`,
//!   `initial_mode = korean|english`(질의 무응답 환경의 기본값),
//!   `ime_query = off`.

// 릴리스 빌드는 콘솔 없이 트레이 상주만 한다.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod debug;
#[cfg(windows)]
mod ime;
#[cfg(windows)]
mod indicator;
#[cfg(windows)]
mod tray;

#[cfg(not(windows))]
fn main() {
    eprintln!("hanengw는 Windows 전용입니다.");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    win::run();
}

#[cfg(windows)]
mod win {
    use crate::indicator::Mode;
    use crate::{ime, indicator};
    use haneng_core::config;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::LazyLock;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CAPITAL};
    use windows_sys::Win32::UI::WindowsAndMessaging::{DispatchMessageW, GetMessageW, MSG};

    /// 트레이 토글: 배지 표시 켜기/끄기.
    pub static ENABLED: AtomicBool = AtomicBool::new(true);

    /// 마지막으로 알아낸 한/영 모드 (질의 무응답일 때의 폴백 표시값).
    static LAST_KNOWN_MODE: AtomicBool = AtomicBool::new(false);

    static CONFIG: LazyLock<config::Config> = LazyLock::new(config::load_config);

    /// 현재 한/영 모드: 포커스 창 IME에 실시간 질의, 무응답이면 마지막 값.
    fn current_korean_mode() -> bool {
        if CONFIG.extra("ime_query") != Some("off") {
            if let Some(korean) = ime::query_korean_mode() {
                LAST_KNOWN_MODE.store(korean, Ordering::Relaxed);
                return korean;
            }
        }
        LAST_KNOWN_MODE.load(Ordering::Relaxed)
    }

    /// 배지에 표시할 상태. 영문이면 Caps Lock 토글 상태로 대/소문자를
    /// 구분한다 (GetKeyState의 토글 비트 — 키 입력 관찰이 아니라 상태 조회).
    fn current_mode() -> Mode {
        if current_korean_mode() {
            Mode::Korean
        } else if unsafe { GetKeyState(VK_CAPITAL as i32) } & 1 != 0 {
            Mode::EnglishUpper
        } else {
            Mode::EnglishLower
        }
    }

    /// 배지 불투명도(layered 알파, 0~255). 설정 파일을 매번 다시 읽어
    /// 설정 창에서 조절한 값이 재시작 없이 반영되게 한다 (타이머가 주기적으로
    /// 호출). 시작 시 캐시한 CONFIG를 쓰지 않는 이유 — 그건 변경을 못 본다.
    fn current_alpha() -> u8 {
        let pct = config::load_config().badge_opacity_percent();
        (pct as u16 * 255 / 100) as u8
    }

    /// 등장 애니메이션 사용 여부. 불투명도와 같은 이유로 매번 다시 읽는다
    /// (배지가 뜰 때만 호출되므로 파일 읽기 비용이 문제되지 않는다).
    fn animation_enabled() -> bool {
        config::load_config().badge_animation()
    }

    /// 데몬은 하나만 — 네임드 뮤텍스로 중복 실행을 막는다.
    /// 이미 실행 중이면 false (호출자 즉시 종료). 뮤텍스 핸들은 프로세스가
    /// 끝날 때까지 유지되도록 닫지 않는다.
    fn single_instance() -> bool {
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        let name: Vec<u16> = "haneng-indicator-singleton\0".encode_utf16().collect();
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
            if handle.is_null() {
                return true; // 뮤텍스 생성 실패 시 과잉 차단 방지.
            }
            GetLastError() != ERROR_ALREADY_EXISTS
        }
    }

    pub fn run() {
        if !single_instance() {
            eprintln!("hanengw가 이미 실행 중입니다.");
            return;
        }
        LazyLock::force(&CONFIG);
        let initial =
            ime::query_korean_mode().unwrap_or(CONFIG.extra("initial_mode") == Some("korean"));
        LAST_KNOWN_MODE.store(initial, Ordering::Relaxed);
        ENABLED.store(
            CONFIG.extra("hover_indicator") != Some("off"),
            Ordering::Relaxed,
        );

        // 마우스와 무관하게 포커스 카렛을 따라가도록 타이머로 구동한다.
        // init이 내부 타이머를 걸고, ENABLED로 표시 여부를 제어한다.
        // 불투명도는 콜백으로 넘겨 타이머가 주기적으로 다시 읽는다.
        indicator::init(current_mode, &ENABLED, current_alpha, animation_enabled);
        // 트레이 아이콘은 메시지 루프가 도는 이 스레드에서 만들어야 한다.
        let _tray = crate::tray::install(&ENABLED);

        unsafe {
            eprintln!("hanengw 실행 중 — 입력 카렛 옆에 한/영 상태를 표시합니다.");
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                DispatchMessageW(&msg);
            }
        }
    }
}
