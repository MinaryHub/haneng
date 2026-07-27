//! 한/영 상태 배지 오버레이 (AppKit NSWindow via objc2).
//!
//! 테두리 없는·비활성·클릭 통과·최상위 창에 색상 + 라벨을 그린다.
//! Windows의 layered 배지 창에 대응한다. AppKit이 한글을 직접 렌더링하므로
//! 별도 폰트 래스터라이저가 필요 없다.

use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSFont, NSTextAlignment, NSTextField, NSWindow,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};

use crate::Mode;

const SIZE: f64 = 24.0;
/// 카렛과 배지 사이 여백 — 배지는 카렛 위쪽에 놓아 입력 글자를 가리지 않는다.
const GAP: f64 = 2.0;

#[repr(C)]
// FFI 레이아웃 유지용 — 주 화면 높이(h)만 읽는다.
#[allow(dead_code)]
struct CgRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CgRect;
}

/// 주 디스플레이(메뉴 막대, 전역 원점)의 높이 — AX(top-left)↔Cocoa
/// (bottom-left) 전역 좌표 변환의 기준이다. CoreGraphics로 직접 얻어
/// (포커스에 따라 달라지는 NSScreen::mainScreen 대신) 항상 주 디스플레이를
/// 가리키게 한다.
fn primary_screen_height() -> f64 {
    unsafe { CGDisplayBounds(CGMainDisplayID()).h }
}

pub struct Badge {
    window: Retained<NSWindow>,
    label: Retained<NSTextField>,
    visible: bool,
    mode: Option<Mode>,
}

impl Badge {
    /// `opacity`는 0.0(투명)~1.0(불투명) — 시야를 가리지 않도록 반투명.
    pub fn new(mtm: MainThreadMarker, opacity: f64) -> Self {
        unsafe {
            let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(SIZE, SIZE));
            let window = NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            );
            window.setOpaque(false);
            window.setAlphaValue(opacity);
            window.setHasShadow(false);
            window.setIgnoresMouseEvents(true);
            // 상태 항목보다 위, 모든 스페이스·전체화면 위에 뜨되 활성화하지 않음.
            window.setLevel(objc2_app_kit::NSStatusWindowLevel);
            window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::Stationary
                    | NSWindowCollectionBehavior::IgnoresCycle,
            );
            window.setBackgroundColor(Some(&NSColor::clearColor()));

            let label = NSTextField::initWithFrame(NSTextField::alloc(mtm), rect);
            label.setBezeled(false);
            label.setEditable(false);
            label.setSelectable(false);
            label.setDrawsBackground(true);
            label.setAlignment(NSTextAlignment::Center);
            label.setFont(Some(&NSFont::boldSystemFontOfSize(15.0)));
            label.setTextColor(Some(&NSColor::whiteColor()));
            // 세로 중앙 정렬을 위해 살짝 내려 그린다 (라벨은 상단 기준).
            label.setFrame(NSRect::new(
                NSPoint::new(0.0, -3.0),
                NSSize::new(SIZE, SIZE),
            ));
            if let Some(view) = window.contentView() {
                view.setWantsLayer(true);
                view.addSubview(&label);
            }

            Self {
                window,
                label,
                visible: false,
                mode: None,
            }
        }
    }

    fn apply_mode(&mut self, mode: Mode) {
        if self.mode == Some(mode) {
            return;
        }
        self.mode = Some(mode);
        let (r, g, b, text) = match mode {
            Mode::Korean => (0.169, 0.424, 0.690, "한"),
            Mode::EnglishUpper => (0.867, 0.420, 0.125, "A"), // 주황 — Caps Lock 경고
            Mode::EnglishLower => (0.290, 0.333, 0.408, "a"),
        };
        let color = NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0);
        self.label.setBackgroundColor(Some(&color));
        self.label.setStringValue(&NSString::from_str(text));
    }

    /// 카렛 사각형(top-left 원점 화면 좌표) 바로 위에 모드를 갱신해 표시한다.
    /// 화면 위로 넘치면 카렛 아래쪽으로 뒤집는다.
    pub fn show_at_caret(&mut self, caret_x: f64, caret_top: f64, caret_height: f64, mode: Mode) {
        self.apply_mode(mode);
        // top-left 기준: 카렛 위쪽에 두되 화면 밖으로 나가면 아래로.
        let y_top_above = caret_top - SIZE - GAP;
        let y_top = if y_top_above >= 0.0 {
            y_top_above
        } else {
            caret_top + caret_height + GAP
        };
        // AX는 top-left 원점(주 화면 기준), AppKit은 bottom-left 원점의 전역
        // 좌표계다. 변환 기준은 **주 화면**(원점 0,0, 메뉴 막대) 높이여야
        // 한다 — 배지가 놓인 화면 높이를 쓰면 멀티모니터에서 어긋난다.
        // caret_x는 이미 전역 x라 그대로 두면 올바른 모니터에 놓인다.
        let screen_h = primary_screen_height();
        let origin = NSPoint::new(caret_x, screen_h - y_top - SIZE);
        // HANENG_DEBUG=1로 실행하면 좌표 진단을 stderr로 출력한다 — 멀티모니터
        // 위치 문제를 정확히 잡기 위한 것. (터미널에서 실행 시에만 보인다.)
        if std::env::var_os("HANENG_DEBUG").is_some() {
            eprintln!(
                "[haneng] caret(ax top-left)=({caret_x:.0},{caret_top:.0}) h={caret_height:.0} \
                 primaryH={screen_h:.0} -> window origin(cocoa)=({:.0},{:.0})",
                origin.x, origin.y
            );
        }
        self.window.setFrameOrigin(origin);
        if !self.visible {
            self.window.orderFront(None);
            self.visible = true;
        }
    }

    pub fn hide(&mut self) {
        if self.visible {
            self.window.orderOut(None);
            self.visible = false;
        }
    }
}
