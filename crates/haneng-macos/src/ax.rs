//! 포커스된 **입력 가능한** 요소의 카렛 위치를 Accessibility API로 읽는다.
//!
//! **손쉬운 사용(Accessibility) 권한**이 필요하다 — 없으면 항상 None.
//! 마우스와 무관하게, 포커스된 텍스트 입력 요소가 있으면 그 카렛에 배지를 띄운다.
//!
//! 세 가지를 모두 만족해야 배지가 뜬다:
//! 1. 포커스 요소가 **글자를 입력할 수 있는** 요소일 것 ([`is_text_input`]) —
//!    웹 페이지 본문·목록·정적 텍스트처럼 못 치는 곳에서는 뜨지 않는다.
//! 2. 지금 **실제로 그 요소로 입력이 갈 것** ([`has_keyboard_focus`]) —
//!    앱이나 창이 포커스를 잃으면 숨긴다.
//! 3. 위치를 알아낼 수 있을 것 — 카렛 사각형(AXBoundsForRange)이 우선이고,
//!    그걸 구현하지 않은 앱(암호 필드·Electron·Java 등)은 요소 사각형의
//!    좌측 상단으로 근사한다(그래야 "입력창인데 배지가 없다"가 안 생긴다).
//!
//! `HANENG_DEBUG=1`로 실행하면 판정 결과를 stderr에 찍는다(같은 메시지는 1회).

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use core_foundation_sys::base::CFGetTypeID;
use core_foundation_sys::number::{CFBooleanGetValue, CFBooleanRef};
use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::OnceLock;

type AXUIElementRef = *mut c_void;
type AXValueRef = CFTypeRef;
type AXError = i32;
const AX_SUCCESS: AXError = 0;

/// AXValueType 상수 (값 불변 — 헤더의 kAXValueCGPoint/CGSize/CGRect/CFRangeType).
const AXVALUE_CGPOINT: u32 = 1;
const AXVALUE_CGSIZE: u32 = 2;
const AXVALUE_CGRECT: u32 = 3;
const AXVALUE_CFRANGE: u32 = 4;

/// AX 질의가 응답 없는 앱에서 이벤트 루프를 붙잡지 않도록 하는 상한(초).
/// 기본값(6초)이면 앱 하나가 멎을 때 배지 전체가 멎는다.
const MESSAGING_TIMEOUT: f32 = 0.25;

/// 카렛/글자 사각형으로 인정할 높이 상한 — 이보다 크면 줄이 아니라 영역
/// 전체를 돌려준 것으로 본다.
const MAX_CARET_HEIGHT: f64 = 300.0;
/// 글자 한 개 범위의 너비 상한 — 터미널처럼 범위 밖 요청에 줄 전체 사각형을
/// 돌려주는 앱을 걸러낸다.
const MAX_GLYPH_WIDTH: f64 = 120.0;
/// 요소 사각형으로 근사할 때 배지 세로 기준으로 쓸 최대 높이(한 줄 정도).
const APPROX_LINE_HEIGHT: f64 = 24.0;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGSize {
    pub width: f64,
    pub height: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGRect {
    pub origin: CGPoint,
    pub size: CGSize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CFRange {
    location: isize,
    // length는 FFI로 채워지지만 위치 계산엔 location만 쓴다.
    #[allow(dead_code)]
    length: isize,
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementCopyParameterizedAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        parameter: CFTypeRef,
        value: *mut CFTypeRef,
    ) -> AXError;
    fn AXUIElementIsAttributeSettable(
        element: AXUIElementRef,
        attribute: CFStringRef,
        settable: *mut u8,
    ) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> AXError;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> AXError;
    fn AXValueGetValue(value: AXValueRef, the_type: u32, value_ptr: *mut c_void) -> bool;
    fn AXValueCreate(the_type: u32, value_ptr: *const c_void) -> AXValueRef;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
}

/// 손쉬운 사용 권한 여부. `prompt=true`면 없을 때 시스템 설정 안내를 띄운다.
pub fn accessibility_trusted(prompt: bool) -> bool {
    unsafe {
        let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
        let value = CFBoolean::from(prompt);
        let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const c_void)
    }
}

/// `HANENG_DEBUG=1`일 때만, 직전과 다른 메시지일 때만 출력한다
/// (틱마다 같은 줄이 쏟아지지 않게).
fn debug_log(msg: impl FnOnce() -> String) {
    static ON: OnceLock<bool> = OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("HANENG_DEBUG").is_some()) {
        return;
    }
    thread_local! {
        static LAST: RefCell<String> = const { RefCell::new(String::new()) };
    }
    let msg = msg();
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        if *last != msg {
            eprintln!("[haneng/ax] {msg}");
            *last = msg;
        }
    });
}

// ---- AX 속성 조회 헬퍼 -------------------------------------------------

/// 속성 값(+1 소유권). 호출자가 `CFRelease` 한다.
unsafe fn copy_attr(element: AXUIElementRef, name: &str) -> Option<CFTypeRef> {
    let key = CFString::new(name);
    let mut value: CFTypeRef = std::ptr::null_mut();
    let err = AXUIElementCopyAttributeValue(element, key.as_concrete_TypeRef(), &mut value);
    if err != AX_SUCCESS || value.is_null() {
        None
    } else {
        Some(value)
    }
}

unsafe fn attr_string(element: AXUIElementRef, name: &str) -> Option<String> {
    let value = copy_attr(element, name)?;
    let out = if CFGetTypeID(value) == CFString::type_id() {
        Some(CFString::wrap_under_get_rule(value as CFStringRef).to_string())
    } else {
        None
    };
    CFRelease(value);
    out
}

unsafe fn attr_bool(element: AXUIElementRef, name: &str) -> Option<bool> {
    let value = copy_attr(element, name)?;
    let out = if CFGetTypeID(value) == CFBoolean::type_id() {
        Some(CFBooleanGetValue(value as CFBooleanRef))
    } else {
        None
    };
    CFRelease(value);
    out
}

/// 속성을 쓸 수 있는가. 질의 자체가 실패하면 None(= 모름).
unsafe fn attr_settable(element: AXUIElementRef, name: &str) -> Option<bool> {
    let key = CFString::new(name);
    let mut settable: u8 = 0;
    let err = AXUIElementIsAttributeSettable(element, key.as_concrete_TypeRef(), &mut settable);
    if err != AX_SUCCESS {
        None
    } else {
        Some(settable != 0)
    }
}

/// AXValue로 감싸인 구조체 속성을 읽는다.
unsafe fn attr_axvalue<T: Copy>(
    element: AXUIElementRef,
    name: &str,
    ax_type: u32,
    mut out: T,
) -> Option<T> {
    let value = copy_attr(element, name)?;
    let ok = AXValueGetValue(value, ax_type, &mut out as *mut _ as *mut c_void);
    CFRelease(value);
    if ok {
        Some(out)
    } else {
        None
    }
}

const ZERO_RECT: CGRect = CGRect {
    origin: CGPoint { x: 0.0, y: 0.0 },
    size: CGSize {
        width: 0.0,
        height: 0.0,
    },
};

/// 요소 자체의 화면 사각형 (AXFrame, 없으면 AXPosition+AXSize).
unsafe fn element_frame(element: AXUIElementRef) -> Option<CGRect> {
    if let Some(rect) = attr_axvalue(element, "AXFrame", AXVALUE_CGRECT, ZERO_RECT) {
        return Some(rect);
    }
    let origin = attr_axvalue(
        element,
        "AXPosition",
        AXVALUE_CGPOINT,
        CGPoint { x: 0.0, y: 0.0 },
    )?;
    let size = attr_axvalue(
        element,
        "AXSize",
        AXVALUE_CGSIZE,
        CGSize {
            width: 0.0,
            height: 0.0,
        },
    )?;
    Some(CGRect { origin, size })
}

/// 지정 범위의 화면상 사각형(AXBoundsForRange). AX는 top-left 원점 화면
/// 좌표를 돌려준다 (마우스 좌표와 같은 기준).
unsafe fn bounds_for_range(
    element: AXUIElementRef,
    location: isize,
    length: isize,
) -> Option<CGRect> {
    let range = CFRange { location, length };
    let range_val = AXValueCreate(AXVALUE_CFRANGE, &range as *const _ as *const c_void);
    if range_val.is_null() {
        return None;
    }
    let key = CFString::new("AXBoundsForRange");
    let mut value: CFTypeRef = std::ptr::null_mut();
    let err = AXUIElementCopyParameterizedAttributeValue(
        element,
        key.as_concrete_TypeRef(),
        range_val,
        &mut value,
    );
    CFRelease(range_val);
    if err != AX_SUCCESS || value.is_null() {
        return None;
    }
    let mut rect = ZERO_RECT;
    let ok = AXValueGetValue(value, AXVALUE_CGRECT, &mut rect as *mut _ as *mut c_void);
    CFRelease(value);
    if ok {
        Some(rect)
    } else {
        None
    }
}

// ---- 판정 --------------------------------------------------------------

/// 카렛 사각형으로 쓸 만한 값인가. 앱이 0×0이나 화면 전체 크기 같은
/// 쓰레기값을 돌려주는 경우가 흔해서(터미널 등) 걸러낸다.
fn plausible_caret(rect: &CGRect) -> bool {
    rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && rect.size.height.is_finite()
        && rect.size.height > 0.5
        && rect.size.height <= MAX_CARET_HEIGHT
}

/// 글자 한 개 범위의 사각형인가 (줄 전체를 돌려준 응답을 거른다).
fn plausible_glyph(rect: &CGRect) -> bool {
    plausible_caret(rect) && rect.size.width.is_finite() && rect.size.width <= MAX_GLYPH_WIDTH
}

/// 요소 사각형으로 근사해도 될 만한 값인가.
fn plausible_frame(rect: &CGRect) -> bool {
    rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && rect.size.width > 1.0
        && rect.size.height > 1.0
}

/// **글자를 칠 수 있는 요소인가.** 읽기 전용 웹 페이지·목록·정적 텍스트에
/// 배지가 뜨지 않게 하는 관문이다.
///
/// - 역할이 입력 컨트롤이면 통과: `AXTextField`(검색 필드 포함)·
///   `AXSecureTextField`(암호)·`AXTextArea`·`AXComboBox`.
/// - 그 밖의 역할(`AXWebArea`·`AXGroup`·`AXStaticText`·`AXScrollArea`…)은
///   `AXValue`를 **쓸 수 있을 때만** 통과 — contenteditable 같은 진짜 편집
///   영역만 남는다. 웹 페이지 본문은 선택 범위(AXSelectedTextRange)를
///   지원해서 예전 코드가 배지를 띄웠는데, 여기서 걸러진다.
/// - 비활성(`AXEnabled=false`) 요소는 제외.
///
/// `AXTextArea`를 무조건 통과시키는 이유: 터미널·일부 편집기는 입력이
/// 가능한데도 `AXValue` 쓰기를 허용하지 않는다(실측 확인). 대신 읽기 전용
/// 텍스트 뷰(콘솔 로그 등)가 드물게 통과할 수 있는데, 터미널에서 배지가
/// 사라지는 쪽이 훨씬 나쁘다고 보고 이 절충을 택했다.
unsafe fn is_text_input(element: AXUIElementRef, role: &str) -> bool {
    if attr_bool(element, "AXEnabled") == Some(false) {
        return false;
    }
    match role {
        "AXTextField" | "AXSecureTextField" | "AXTextArea" | "AXComboBox" => true,
        _ => attr_settable(element, "AXValue") == Some(true),
    }
}

/// **지금 실제로 키보드 입력이 이 요소로 가는가.**
///
/// 요소 자신의 `AXFocused`는 쓸 수 없다 — 배경 앱의 요소도 계속 `true`로
/// 답한다(실측: 비활성 터미널의 텍스트 영역 `AXFocused=true`). 포커스 상실은
/// 한 단계 위에서만 보인다:
/// - 앱이 최전면이 아니면(`AXFrontmost=false`) 입력은 그 요소로 가지 않는다.
///   앱 전환·Spotlight·메뉴 열림·화면 잠금이 모두 여기 걸린다.
/// - 같은 앱 안에서도 요소가 속한 창이 **키 창이 아니면** 마찬가지다.
///   창이 "포커스도 아니고 메인도 아니다"라고 분명히 답할 때만 거른다
///   (시트·패널에서 `AXWindow`가 부모 창을 가리키는 구현을 오판하지 않도록).
///
/// 판단이 안 서면(속성 미지원·질의 실패) 표시하는 쪽을 택한다.
unsafe fn has_keyboard_focus(element: AXUIElementRef) -> bool {
    let mut pid: i32 = 0;
    if AXUIElementGetPid(element, &mut pid) == AX_SUCCESS && pid > 0 {
        let app = AXUIElementCreateApplication(pid);
        if !app.is_null() {
            AXUIElementSetMessagingTimeout(app, MESSAGING_TIMEOUT);
            let frontmost = attr_bool(app, "AXFrontmost");
            CFRelease(app as CFTypeRef);
            if frontmost == Some(false) {
                debug_log(|| "숨김: 포커스된 앱이 최전면이 아님".to_string());
                return false;
            }
        }
    }
    let window =
        copy_attr(element, "AXWindow").or_else(|| copy_attr(element, "AXTopLevelUIElement"));
    if let Some(window) = window {
        let focused = attr_bool(window as AXUIElementRef, "AXFocused");
        let main = attr_bool(window as AXUIElementRef, "AXMain");
        CFRelease(window);
        if focused == Some(false) && main != Some(true) {
            debug_log(|| "숨김: 요소가 속한 창이 키 창이 아님".to_string());
            return false;
        }
    }
    true
}

/// 캐럿(선택 범위)의 시작 위치.
unsafe fn caret_location(element: AXUIElementRef) -> Option<isize> {
    let range = attr_axvalue(
        element,
        "AXSelectedTextRange",
        AXVALUE_CFRANGE,
        CFRange {
            location: 0,
            length: 0,
        },
    )?;
    (range.location >= 0).then_some(range.location)
}

/// 포커스된 요소에서 배지 위치(top-left 원점 화면 좌표)를 정한다.
/// 입력 불가 요소면 None.
unsafe fn caret_of(element: AXUIElementRef, role: &str) -> Option<CGRect> {
    if !is_text_input(element, role) {
        debug_log(|| format!("숨김: role={role} — 입력 가능한 요소가 아님"));
        return None;
    }
    // 입력 가능한 요소라도, 지금 그리로 입력이 가지 않으면(앱·창이 포커스를
    // 잃음) 배지를 숨긴다.
    if !has_keyboard_focus(element) {
        return None;
    }

    // 1) 카렛 사각형. 길이 0(캐럿) → 캐럿 위치 글자 → 직전 글자 순으로,
    //    각 단계마다 값이 그럴듯한지 검사한다.
    if let Some(loc) = caret_location(element) {
        let rect = bounds_for_range(element, loc, 0)
            .filter(plausible_caret)
            .or_else(|| bounds_for_range(element, loc, 1).filter(plausible_glyph))
            .or_else(|| {
                (loc > 0)
                    .then(|| bounds_for_range(element, loc - 1, 1))
                    .flatten()
                    .filter(plausible_glyph)
            });
        if let Some(rect) = rect {
            debug_log(|| {
                format!(
                    "표시: role={role} 카렛=({:.0},{:.0}) h={:.0}",
                    rect.origin.x, rect.origin.y, rect.size.height
                )
            });
            return Some(rect);
        }
    }

    // 2) AXBoundsForRange를 구현하지 않는 앱(암호 필드·Electron·Java 등):
    //    요소 사각형 좌측 상단으로 근사한다. 정확하진 않아도 "입력창인데
    //    배지가 아예 없다"보다는 낫다.
    if let Some(frame) = element_frame(element).filter(plausible_frame) {
        debug_log(|| {
            format!(
                "표시(근사): role={role} 요소=({:.0},{:.0}) {:.0}x{:.0}",
                frame.origin.x, frame.origin.y, frame.size.width, frame.size.height
            )
        });
        return Some(CGRect {
            origin: frame.origin,
            size: CGSize {
                width: 0.0,
                height: frame.size.height.min(APPROX_LINE_HEIGHT),
            },
        });
    }

    debug_log(|| format!("숨김: role={role} — 위치를 알 수 없음"));
    None
}

/// **포커스된 입력 요소**의 배지 위치(top-left 원점 화면 좌표).
/// 입력 불가이거나 위치를 못 구하면 `None`.
///
/// 카렛은 마우스 아래 요소가 아니라 **포커스된 요소**에 있다. 시스템 전역
/// `AXFocusedUIElement`를 먼저 쓰고, 그게 실패하는 앱을 위해
/// `AXFocusedApplication`을 거쳐 한 번 더 시도한다.
pub fn focused_caret() -> Option<CGRect> {
    unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }
        AXUIElementSetMessagingTimeout(system, MESSAGING_TIMEOUT);
        let focused = copy_attr(system, "AXFocusedUIElement").or_else(|| {
            let app = copy_attr(system, "AXFocusedApplication")?;
            AXUIElementSetMessagingTimeout(app as AXUIElementRef, MESSAGING_TIMEOUT);
            let focused = copy_attr(app as AXUIElementRef, "AXFocusedUIElement");
            CFRelease(app);
            focused
        });
        CFRelease(system as CFTypeRef);

        let Some(element) = focused else {
            debug_log(|| "숨김: 포커스된 요소 없음".to_string());
            return None;
        };
        let element = element as AXUIElementRef;
        AXUIElementSetMessagingTimeout(element, MESSAGING_TIMEOUT);
        let role = attr_string(element, "AXRole").unwrap_or_else(|| "?".to_string());
        let caret = caret_of(element, &role);
        CFRelease(element as CFTypeRef);
        caret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_sanity_checks() {
        let caret = CGRect {
            origin: CGPoint { x: 10.0, y: 20.0 },
            size: CGSize {
                width: 1.0,
                height: 16.0,
            },
        };
        assert!(plausible_caret(&caret));
        assert!(plausible_glyph(&caret));

        // 터미널이 길이 0 범위에 돌려주는 0×0 쓰레기값.
        let empty = CGRect {
            size: CGSize {
                width: 0.0,
                height: 0.0,
            },
            ..caret
        };
        assert!(!plausible_caret(&empty));

        // 글자 하나인데 줄 전체 너비 — 범위 밖 요청에 대한 엉뚱한 응답.
        let line = CGRect {
            size: CGSize {
                width: 322.0,
                height: 14.0,
            },
            ..caret
        };
        assert!(plausible_caret(&line));
        assert!(!plausible_glyph(&line));

        // 영역 전체 높이는 카렛이 아니다.
        let whole = CGRect {
            size: CGSize {
                width: 4.0,
                height: 900.0,
            },
            ..caret
        };
        assert!(!plausible_caret(&whole));
    }

    #[test]
    fn frame_sanity_checks() {
        assert!(plausible_frame(&CGRect {
            origin: CGPoint {
                x: -1440.0,
                y: 30.0
            },
            size: CGSize {
                width: 200.0,
                height: 22.0
            },
        }));
        assert!(!plausible_frame(&ZERO_RECT));
        assert!(!plausible_frame(&CGRect {
            origin: CGPoint {
                x: f64::NAN,
                y: 0.0
            },
            size: CGSize {
                width: 200.0,
                height: 22.0
            },
        }));
    }
}
