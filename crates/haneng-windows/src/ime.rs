//! 한글 IME 상태 질의.
//!
//! 포커스 컨트롤의 기본 IME 윈도우에 WM_IME_CONTROL/IMC_GETOPENSTATUS를
//! 보내 한글 모드 여부를 읽는다. 한글 IME가 없는 환경에서는 None.

use std::mem::{size_of, zeroed};
use windows_sys::Win32::Foundation::POINT;
use windows_sys::Win32::Graphics::Gdi::ClientToScreen;
use windows_sys::Win32::UI::Input::Ime::ImmGetDefaultIMEWnd;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::IsWindowEnabled;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetGUIThreadInfo, GetWindowLongW, GetWindowThreadProcessId,
    SendMessageTimeoutW, ES_READONLY, GUITHREADINFO, GWL_STYLE, SMTO_ABORTIFHUNG, WM_IME_CONTROL,
};

/// IME 열림 상태(한글 IME에서 = 한글 모드 여부).
const IMC_GETOPENSTATUS: usize = 0x0005;

/// 포커스 컨트롤의 IME 열림 상태를 질의한다. `None` = 응답 없음
/// (IME 창이 없거나 타임아웃) — 호출자는 키 관찰 추적으로 폴백한다.
///
/// SendMessageW 대신 타임아웃 버전을 쓴다: 대상 프로세스가 멈춰 있으면
/// 배지 갱신·변환 스레드까지 함께 멈추기 때문.
pub fn query_korean_mode() -> Option<bool> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_null() {
            return None;
        }
        // 포커스 컨트롤 기준이 창 기준보다 정확하다 (스레드가 같으면 동일).
        let thread = GetWindowThreadProcessId(foreground, std::ptr::null_mut());
        let mut info: GUITHREADINFO = zeroed();
        info.cbSize = size_of::<GUITHREADINFO>() as u32;
        let target = if GetGUIThreadInfo(thread, &mut info) != 0 && !info.hwndFocus.is_null() {
            info.hwndFocus
        } else {
            foreground
        };
        let ime = ImmGetDefaultIMEWnd(target);
        if ime.is_null() {
            return None;
        }
        let mut result: usize = 0;
        let ok = SendMessageTimeoutW(
            ime,
            WM_IME_CONTROL,
            IMC_GETOPENSTATUS,
            0,
            SMTO_ABORTIFHUNG,
            50,
            &mut result,
        );
        if ok == 0 {
            return None;
        }
        Some(result != 0)
    }
}

/// 포커스 창의 텍스트 카렛(입력 커서) 위치를 화면 좌표 `(left, top, bottom)`로
/// 돌려준다. `None` = 유효한 카렛 없음(배지를 숨기는 근거).
///
/// 1) 표준 Win32 카렛(`GUITHREADINFO.rcCaret`)을 먼저 읽고 —
///    메모장·Win32/WinForms 대화상자 등에서 가장 빠르고 정확하다.
/// 2) 그게 없으면(크롬·Electron·일부 UWP는 자체 카렛을 그려 `hwndCaret`이
///    비어 있다) UI Automation TextPattern의 선택 영역 사각형으로 폴백한다
///    (macOS 데몬이 AXSelectedTextRange/AXBoundsForRange로 읽는 것과 동형).
pub fn caret_screen_rect() -> Option<(i32, i32, i32)> {
    caret_via_win32().or_else(|| unsafe { caret_via_uia() })
}

/// 표준 Win32 카렛(`GUITHREADINFO.rcCaret`) → 화면 좌표.
fn caret_via_win32() -> Option<(i32, i32, i32)> {
    unsafe {
        let foreground = GetForegroundWindow();
        if foreground.is_null() {
            return None;
        }
        let thread = GetWindowThreadProcessId(foreground, std::ptr::null_mut());
        let mut info: GUITHREADINFO = zeroed();
        info.cbSize = size_of::<GUITHREADINFO>() as u32;
        if GetGUIThreadInfo(thread, &mut info) == 0 || info.hwndCaret.is_null() {
            return None;
        }
        // 입력 불가한 컨트롤엔 표시하지 않는다: 비활성(disabled)이거나 읽기 전용
        // (ES_READONLY)이면 숨긴다. 카렛을 소유한 창은 거의 항상 Edit/RichEdit라
        // 이 스타일 비트로 읽기 전용을 판별할 수 있다.
        if IsWindowEnabled(info.hwndCaret) == 0
            || GetWindowLongW(info.hwndCaret, GWL_STYLE) & ES_READONLY != 0
        {
            return None;
        }
        let r = info.rcCaret;
        // 높이 0이면 실제 카렛이 아니다 (질의 실패·스텁 값).
        if r.bottom <= r.top {
            return None;
        }
        // rcCaret은 hwndCaret 클라이언트 좌표 → 화면 좌표로 변환.
        let mut top_left = POINT {
            x: r.left,
            y: r.top,
        };
        let mut bottom = POINT {
            x: r.left,
            y: r.bottom,
        };
        if ClientToScreen(info.hwndCaret, &mut top_left) == 0
            || ClientToScreen(info.hwndCaret, &mut bottom) == 0
        {
            return None;
        }
        Some((top_left.x, top_left.y, bottom.y))
    }
}

use std::cell::RefCell;
use windows::core::Interface;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, SAFEARRAY,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, IUIAutomationTextRange,
    IUIAutomationValuePattern, TextUnit_Character, UIA_DocumentControlTypeId,
    UIA_EditControlTypeId, UIA_TextPatternId, UIA_ValuePatternId,
};

thread_local! {
    /// 폴링 스레드(메시지 루프)에서만 쓰는 UIA 인스턴스 — 최초 1회 생성 후 재사용.
    /// IUIAutomation은 Send/Sync가 아니므로 스레드 로컬에 둔다.
    static UIA: RefCell<Option<IUIAutomation>> = const { RefCell::new(None) };
}

/// UI Automation으로 포커스 요소의 캐럿(선택 영역 시작) 사각형을 읽는다.
///
/// Win32 카렛이 없는 앱(크롬 등) 전용 폴백. 텍스트 패턴이 없는 요소(바탕화면
/// 등)에서는 각 단계가 자연스럽게 실패해 `None` → 배지를 숨긴다.
///
/// 주의: 다른 프로세스에 대한 크로스 프로세스 COM 호출이라 대상이 바쁘면
/// 잠깐 지연될 수 있다(Win32 카렛 질의처럼 타임아웃을 걸 API가 없다).
/// 표준 카렛이 있는 앱은 위 `caret_via_win32`에서 이미 처리되므로, 이 경로는
/// 카렛을 못 읽는 앱에서만 탄다.
unsafe fn caret_via_uia() -> Option<(i32, i32, i32)> {
    UIA.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            // 이 스레드는 메시지 루프(STA)다. 이미 초기화돼 있으면 실패를
            // 무시한다 — COM만 살아 있으면 CoCreateInstance는 동작한다.
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            match CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) {
                Ok(a) => *slot = Some(a),
                Err(_) => return None,
            }
        }
        let uia = slot.as_ref()?;

        let element = uia.GetFocusedElement().ok()?;
        // GetFocusedElement는 시스템 전역이라, 다른 앱으로 전환해도(예: 크롬에서
        // Alt+Tab) 직전 텍스트 필드를 계속 돌려준다 → 배지가 남는다. Win32 카렛
        // 경로는 포그라운드 기준이라 저절로 숨지만 이 폴백은 그렇지 않으므로,
        // 포커스 요소가 포그라운드 창과 같은 프로세스가 아니면 포커스를 잃은
        // 것으로 보고 숨긴다.
        let foreground = GetForegroundWindow();
        if foreground.is_null() {
            return None;
        }
        let mut fg_pid: u32 = 0;
        GetWindowThreadProcessId(foreground, &mut fg_pid);
        if element.CurrentProcessId().ok()? as u32 != fg_pid {
            return None;
        }
        // 편집 가능한 텍스트 필드에서만 배지를 띄운다. 편집 컨트롤(Edit)이나
        // 리치 편집기(Document)만 허용 — 일반 웹 페이지의 단순 텍스트 선택에서
        // 배지가 뜨는 오탐을 줄인다.
        let ctype = element.CurrentControlType().ok()?;
        if ctype != UIA_EditControlTypeId && ctype != UIA_DocumentControlTypeId {
            return None;
        }
        // 입력 불가한 필드엔 표시하지 않는다: 비활성(disabled)이거나, ValuePattern이
        // 읽기 전용을 보고하면 숨긴다. 클릭이 편집 불가 영역으로 옮겨가면(예: 웹
        // 페이지의 읽기 전용 본문) 여기서 걸러진다. ValuePattern 미지원 요소(일부
        // 리치 편집기)는 편집 가능으로 간주해 통과시킨다.
        if !element.CurrentIsEnabled().ok()?.as_bool() {
            return None;
        }
        if let Ok(unknown) = element.GetCurrentPattern(UIA_ValuePatternId) {
            if let Ok(value) = unknown.cast::<IUIAutomationValuePattern>() {
                if value
                    .CurrentIsReadOnly()
                    .map(|b| b.as_bool())
                    .unwrap_or(false)
                {
                    return None;
                }
            }
        }
        // 텍스트 패턴 미지원이면 GetCurrentPattern이 널 → from_abi가 Err 처리.
        let pattern: IUIAutomationTextPattern = element
            .GetCurrentPattern(UIA_TextPatternId)
            .ok()?
            .cast()
            .ok()?;
        let selection = pattern.GetSelection().ok()?;
        if selection.Length().ok()? < 1 {
            return None;
        }
        let range = selection.GetElement(0).ok()?;

        // 접힌 캐럿(선택 없음)은 사각형이 빌 수 있다 → 한 글자만큼 넓혀 재시도.
        if let Some(rect) = first_rect(&range) {
            return Some(rect);
        }
        range.ExpandToEnclosingUnit(TextUnit_Character).ok()?;
        first_rect(&range)
    })
}

/// 텍스트 범위의 첫 경계 사각형을 화면 좌표 `(left, top, bottom)`로.
/// GetBoundingRectangles는 [left, top, width, height, ...] double SAFEARRAY.
unsafe fn first_rect(range: &IUIAutomationTextRange) -> Option<(i32, i32, i32)> {
    let sa = range.GetBoundingRectangles().ok()?;
    if sa.is_null() {
        return None;
    }
    let rect = read_first_rect(sa);
    let _ = SafeArrayDestroy(sa);
    rect
}

unsafe fn read_first_rect(sa: *const SAFEARRAY) -> Option<(i32, i32, i32)> {
    let lbound = SafeArrayGetLBound(sa, 1).ok()?;
    let ubound = SafeArrayGetUBound(sa, 1).ok()?;
    if ubound - lbound + 1 < 4 {
        return None; // 사각형 하나도 없음.
    }
    let mut data: *mut std::ffi::c_void = std::ptr::null_mut();
    SafeArrayAccessData(sa, &mut data).ok()?;
    let f = data as *const f64;
    let left = *f;
    let top = *f.add(1);
    let height = *f.add(3);
    let _ = SafeArrayUnaccessData(sa);
    let l = left.round() as i32;
    let t = top.round() as i32;
    let b = (top + height).round() as i32;
    // 높이가 0/음수면 최소 1px로 — 호출자는 top<bottom을 기대한다.
    Some((l, t, if b > t { b } else { t + 1 }))
}
