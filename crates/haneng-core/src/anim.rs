//! 배지 등장 애니메이션 — 시선을 끄는 펄스(반짝임) 파형.
//!
//! Windows(layered 알파)와 macOS(NSWindow alphaValue) 모두 창 불투명도만
//! 바꾸면 되므로, 파형 계산을 여기 한 곳에 두고 두 데몬이 같은 값을 쓴다.
//! (Linux(X11) 데몬은 아직 적용하지 않는다.)

/// 펄스 한 번의 길이(ms) — 두 번 반짝이고 끝난다.
pub const PULSE_MS: u64 = 500;

/// 기본 불투명도에서 위아래로 흔드는 폭.
const SWING: f64 = 0.75;
/// 가장 어두울 때의 하한 — 완전히 사라져 깜빡 꺼진 것처럼 보이지 않게 한다.
const MIN_ALPHA: f64 = 0.08;

/// 진행도 `t`(0.0=시작, 1.0=끝)에서의 창 불투명도(0.0~1.0).
///
/// `base`는 사용자가 설정한 평상시 불투명도다. 파형은 코사인 2주기라
/// **밝게 → 어둡게 → 밝게 → 어둡게**로 두 번 반짝이고, 진폭이 선형으로
/// 잦아들어 `t=1`에서 정확히 `base`로 수렴한다(끝날 때 값이 튀지 않는다).
pub fn pulse_alpha(base: f64, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let swing = (t * 2.0 * std::f64::consts::TAU).cos();
    let envelope = 1.0 - t;
    (base + envelope * swing * SWING).clamp(MIN_ALPHA, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_bright_and_ends_at_base() {
        // 시작은 최대 밝기(눈에 띄게), 끝은 평상시 값으로 정확히 수렴.
        assert_eq!(pulse_alpha(0.8, 0.0), 1.0);
        assert!((pulse_alpha(0.8, 1.0) - 0.8).abs() < 1e-9);
        assert!((pulse_alpha(0.3, 1.0) - 0.3).abs() < 1e-9);
    }

    #[test]
    fn dips_and_flashes_twice() {
        let base = 0.8;
        // 1/4·3/4 지점에서 어두워지고, 1/2 지점에서 다시 밝아진다.
        assert!(pulse_alpha(base, 0.25) < base);
        assert!(pulse_alpha(base, 0.75) < base);
        assert!(pulse_alpha(base, 0.5) > base);
        // 두 번째 반짝임은 첫 번째보다 약하다(진폭이 잦아든다).
        assert!(pulse_alpha(base, 0.75) > pulse_alpha(base, 0.25));
    }

    #[test]
    fn stays_in_valid_range() {
        for base in [0.3, 0.5, 0.8, 1.0] {
            for step in 0..=100 {
                let a = pulse_alpha(base, step as f64 / 100.0);
                assert!((MIN_ALPHA..=1.0).contains(&a), "base={base} t={step} a={a}");
            }
        }
    }

    #[test]
    fn clamps_progress_outside_range() {
        assert_eq!(pulse_alpha(0.8, -1.0), pulse_alpha(0.8, 0.0));
        assert_eq!(pulse_alpha(0.8, 2.0), pulse_alpha(0.8, 1.0));
    }
}
