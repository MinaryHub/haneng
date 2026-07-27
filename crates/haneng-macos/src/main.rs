//! hanengd — 한/영 상태 표시기 (macOS 상주 데몬).
//!
//! 포커스된 입력 카렛 옆에 현재 상태 배지를 표시한다(마우스와 무관):
//! 파랑 "한"(한글) / 회색 "a"(영문 소문자) / 주황 "A"(영문 + Caps Lock).
//!
//! - 카렛 위치: Accessibility API (포커스 요소의 AXBoundsForRange). **손쉬운
//!   사용 권한** 필요 — 없거나 카렛을 못 읽으면 배지를 숨긴다.
//! - 한/영 판별: 현재 키보드 입력 소스(TIS) 조회.
//! - **키 입력을 관찰하거나 텍스트를 조작하지 않는다** — 카렛 위치·모드만 읽는다.
//! - 메뉴바 아이콘: 배지 토글 · 설정 · 종료.

#[cfg(target_os = "macos")]
mod ax;
#[cfg(target_os = "macos")]
mod badge;
#[cfg(target_os = "macos")]
mod mac_input;
#[cfg(target_os = "macos")]
mod tis;
#[cfg(target_os = "macos")]
mod tray;

/// 배지에 표시할 입력 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    EnglishLower,
    EnglishUpper,
    Korean,
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("hanengd는 macOS 전용입니다.");
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn main() {
    macos::run();
}

#[cfg(target_os = "macos")]
mod macos {
    use crate::badge::Badge;
    use crate::{ax, mac_input, tis, Mode};
    use haneng_core::config;
    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    use tao::event::{Event, StartCause};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};

    pub static ENABLED: AtomicBool = AtomicBool::new(true);

    /// 상태가 바뀌는 동안(입력·포커스·모드 변화)의 빠른 폴링 주기.
    const POLL_ACTIVE: Duration = Duration::from_millis(120);
    /// 상태가 한동안 그대로일 때의 유휴 폴링 주기 — 배지 위치는 카렛을 읽는
    /// Accessibility 호출이 비싸므로(포커스 앱·WindowServer까지 CPU 소모),
    /// 아무것도 안 바뀌면 폴링을 늦춰 상주 시 CPU 점유를 줄인다.
    const POLL_IDLE: Duration = Duration::from_millis(450);
    /// 이만큼의 틱 동안 상태(카렛 위치+모드)가 그대로면 유휴로 본다.
    const IDLE_AFTER_TICKS: u32 = 6;

    /// 한 틱의 표시 상태 요약 — 이전 틱과 같으면 화면이 안 바뀐 것.
    /// `None` = 배지 숨김(포커스된 텍스트 카렛 없음). 값은 카렛 위치(비트)와
    /// 모드로 이뤄진다. 카렛이 정지해 있으면 AX가 매번 같은 사각형을
    /// 돌려주므로 비트 비교로 "변화 없음"을 정확히 판정할 수 있다.
    type TickState = Option<(u64, u64, u64, u8)>;

    /// 마지막으로 알아낸 한/영 값 (입력 소스 조회 실패 시 폴백).
    static LAST_KOREAN: AtomicBool = AtomicBool::new(false);

    fn current_mode() -> Mode {
        let korean =
            tis::current_source_is_korean().unwrap_or_else(|| LAST_KOREAN.load(Ordering::Relaxed));
        LAST_KOREAN.store(korean, Ordering::Relaxed);
        if korean {
            Mode::Korean
        } else if mac_input::caps_lock_on() {
            Mode::EnglishUpper
        } else {
            Mode::EnglishLower
        }
    }

    pub fn run() {
        // 데몬은 하나만 — 이미 실행 중이면 종료.
        if !haneng_core::single_instance::acquire("hanengd") {
            eprintln!("hanengd가 이미 실행 중입니다.");
            return;
        }
        let cfg = config::load_config();
        ENABLED.store(
            cfg.extra("hover_indicator") != Some("off"),
            Ordering::Relaxed,
        );
        LAST_KOREAN.store(
            cfg.extra("initial_mode") == Some("korean"),
            Ordering::Relaxed,
        );
        // 시야를 가리지 않도록 배지를 반투명하게 (기본 80%).
        let opacity = cfg.badge_opacity_percent() as f64 / 100.0;

        // 손쉬운 사용 권한 요청 (없으면 텍스트 감지 불가 → 배지가 안 뜬다).
        if !ax::accessibility_trusted(true) {
            eprintln!(
                "손쉬운 사용 권한이 필요합니다 — 시스템 설정 → 개인정보 보호 및 보안 →\n\
                 손쉬운 사용에 hanengd(또는 실행 중인 터미널)를 추가하세요."
            );
        }

        let mut event_loop = EventLoopBuilder::<()>::with_user_event().build();
        // 메뉴바 전용 앱 — Dock 아이콘 없음.
        event_loop.set_activation_policy(ActivationPolicy::Accessory);

        // 배지·트레이 모두 루프가 끝날 때까지 살아 있어야 한다.
        let badge: RefCell<Option<Badge>> = RefCell::new(None);
        let tray: RefCell<Option<tray_icon::TrayIcon>> = RefCell::new(None);

        // 유휴 back-off 상태: 직전 틱의 표시 상태와 변화 없이 지난 틱 수.
        let last_state: Cell<TickState> = Cell::new(None);
        let idle_ticks: Cell<u32> = Cell::new(0);

        eprintln!("hanengd 실행 중 — 포커스된 입력창의 카렛 옆에 한/영 상태를 표시합니다.");

        event_loop.run(move |event, _target, control_flow| match event {
            Event::NewEvents(StartCause::Init) => {
                let mtm =
                    objc2::MainThreadMarker::new().expect("event loop runs on the main thread");
                *badge.borrow_mut() = Some(Badge::new(mtm, opacity));
                *tray.borrow_mut() = Some(crate::tray::install(&ENABLED));
                *control_flow = ControlFlow::WaitUntil(Instant::now() + POLL_ACTIVE);
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                let state = tick(&badge);
                // 상태가 바뀌면 빠르게, 한동안 그대로면 유휴 주기로 늦춘다.
                let next = if state != last_state.replace(state) {
                    idle_ticks.set(0);
                    POLL_ACTIVE
                } else {
                    let n = idle_ticks.get().saturating_add(1);
                    idle_ticks.set(n);
                    if n >= IDLE_AFTER_TICKS {
                        POLL_IDLE
                    } else {
                        POLL_ACTIVE
                    }
                };
                *control_flow = ControlFlow::WaitUntil(Instant::now() + next);
            }
            _ => {}
        });
    }

    /// 한 틱: 포커스된 카렛에 배지를 갱신하고, 그 표시 상태 요약을 돌려준다
    /// (back-off 판정용). 카렛이 없으면 숨기고 `None`.
    fn tick(badge: &RefCell<Option<Badge>>) -> TickState {
        let mut guard = badge.borrow_mut();
        let badge = guard.as_mut()?;

        if !ENABLED.load(Ordering::Relaxed) {
            badge.hide();
            return None;
        }
        // 마우스와 무관하게 **포커스된 입력 요소의 카렛**이 있으면 그 카렛에
        // 표시한다. 카렛이 없으면(텍스트 입력 포커스 아님) 숨긴다.
        if let Some(rect) = ax::focused_caret_bounds() {
            let mode = current_mode();
            badge.show_at_caret(rect.origin.x, rect.origin.y, rect.size.height, mode);
            Some((
                rect.origin.x.to_bits(),
                rect.origin.y.to_bits(),
                rect.size.height.to_bits(),
                mode as u8,
            ))
        } else {
            badge.hide();
            None
        }
    }
}
