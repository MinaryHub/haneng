//! 포커스된 입력 요소의 카렛 위치를 Accessibility API로 읽는다.
//!
//! **손쉬운 사용(Accessibility) 권한**이 필요하다 — 없으면 항상 None.
//! 마우스와 무관하게, 포커스된 텍스트 요소가 있으면 그 카렛에 배지를 띄운다.

use core_foundation::base::{CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::{CFString, CFStringRef};
use std::ffi::c_void;

type AXUIElementRef = *mut c_void;
type AXValueRef = CFTypeRef;
type AXError = i32;
const AX_SUCCESS: AXError = 0;

/// AXValueType 상수 (값 불변 — 헤더의 kAXValueCGRectType/kAXValueCFRangeType).
const AXVALUE_CGRECT: u32 = 3;
const AXVALUE_CFRANGE: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGPoint {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub struct CGSize {
    // width는 FFI 레이아웃 유지용 — 위치 계산엔 height만 쓴다.
    #[allow(dead_code)]
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
    let mut rect = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    };
    let ok = AXValueGetValue(value, AXVALUE_CGRECT, &mut rect as *mut _ as *mut c_void);
    CFRelease(value);
    if ok {
        Some(rect)
    } else {
        None
    }
}

/// **포커스된** 텍스트 요소의 캐럿 사각형(top-left 원점 화면 좌표)을 돌려준다.
/// 카렛을 못 읽으면 `None`.
///
/// 카렛은 마우스 아래 요소가 아니라 **포커스된 요소**에 있다. 마우스 아래
/// 요소(`AXUIElementCopyElementAtPosition`)는 포커스가 아니거나 하위 요소라
/// `AXSelectedTextRange`가 없을 수 있어, 시스템 전역의 AXFocusedUIElement를
/// 쓴다. 순서: 포커스 요소 → AXSelectedTextRange → AXBoundsForRange.
/// 길이 0 범위가 사각형을 안 주는 앱을 위해 길이 1 범위로 폴백한다.
pub fn focused_caret_bounds() -> Option<CGRect> {
    unsafe {
        let system = AXUIElementCreateSystemWide();
        if system.is_null() {
            return None;
        }
        let focused_key = CFString::new("AXFocusedUIElement");
        let mut focused: CFTypeRef = std::ptr::null_mut();
        let err =
            AXUIElementCopyAttributeValue(system, focused_key.as_concrete_TypeRef(), &mut focused);
        CFRelease(system as CFTypeRef);
        if err != AX_SUCCESS || focused.is_null() {
            return None;
        }
        let element = focused as AXUIElementRef;

        // 현재 선택/캐럿 범위.
        let sel_key = CFString::new("AXSelectedTextRange");
        let mut sel_val: CFTypeRef = std::ptr::null_mut();
        let err =
            AXUIElementCopyAttributeValue(element, sel_key.as_concrete_TypeRef(), &mut sel_val);
        if err != AX_SUCCESS || sel_val.is_null() {
            CFRelease(element as CFTypeRef);
            return None;
        }
        let mut range = CFRange {
            location: 0,
            length: 0,
        };
        let ok = AXValueGetValue(
            sel_val,
            AXVALUE_CFRANGE,
            &mut range as *mut _ as *mut c_void,
        );
        CFRelease(sel_val);
        if !ok {
            CFRelease(element as CFTypeRef);
            return None;
        }

        let loc = range.location;
        // 길이 0 캐럿 사각형을 먼저 시도(대부분 유효한 높이 반환), 안 되면
        // 길이 1 글자 사각형으로 폴백(문서 끝이면 직전 글자).
        let rect = bounds_for_range(element, loc, 0)
            .filter(|r| r.size.height > 0.0)
            .or_else(|| bounds_for_range(element, loc, 1))
            .or_else(|| {
                if loc > 0 {
                    bounds_for_range(element, loc - 1, 1)
                } else {
                    None
                }
            });
        CFRelease(element as CFTypeRef);
        rect
    }
}
