//! haneng-core: 한/영 입력 모드 오타 감지·변환 엔진.
//!
//! 플랫폼 독립적인 순수 로직만 담는다 — 키 후킹·텍스트 주입은 플랫폼
//! 어댑터 크레이트의 몫이다. 전체 설계는 저장소 루트의 PLAN.md 참고.
//!
//! ```
//! use haneng_core::{english_to_hangul, hangul_to_english, Detector, Sensitivity, Verdict};
//!
//! assert_eq!(english_to_hangul("gksrmf"), "한글");
//! assert_eq!(hangul_to_english("ㅗ디ㅣㅐ"), "hello");
//!
//! let detector = Detector::new(Sensitivity::Balanced);
//! assert_eq!(detector.analyze("gksrmf"), Verdict::ToHangul("한글".into()));
//! assert_eq!(detector.analyze("hello"), Verdict::Keep);
//! ```

pub mod anim;
pub mod auto;
pub mod compose;
pub mod config;
pub mod decompose;
pub mod detect;
pub mod hangul;
pub mod layout;
pub mod lexicon;
#[rustfmt::skip]
mod lexicon_data;
#[rustfmt::skip]
mod sebeolsik_data;
pub mod plan;
#[cfg(unix)]
pub mod single_instance;
pub mod sync;
pub mod tracker;

pub use auto::{AutoCorrector, AutoDecision};
pub use compose::{english_to_hangul, english_to_hangul_with, Composer};
pub use config::Config;
pub use decompose::{hangul_to_english, hangul_to_english_with};
pub use detect::{english_score, Detector, Sensitivity, Verdict};
pub use layout::Layout;
pub use plan::{build_replace_plan, ReplacePlan};
pub use sync::InjectionLock;
pub use tracker::{KeyClass, Target, WordBuffer};
