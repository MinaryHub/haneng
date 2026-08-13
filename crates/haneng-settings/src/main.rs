//! haneng-settings — 설정 창 (eframe/egui).
//!
//! 한/영 상태 표시기의 `config.txt`를 편집하고 업데이트를 확인·설치한다.
//! 데몬은 시작 시에만 설정을 읽으므로, 저장 후 데몬을 재시작해야 적용된다.
//! 실시간 배지 토글은 데몬 트레이 메뉴가 담당한다.
//!
//! egui 기본 폰트에는 한글이 없어 OS 시스템 폰트를 찾아 싣는다.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod update;

use haneng_core::{config, Config};
use std::sync::{Arc, Mutex};
use update::UpdateState;

fn main() -> eframe::Result {
    let icon = eframe::egui::IconData {
        rgba: include_bytes!("../../../assets/icon-64.rgba").to_vec(),
        width: 64,
        height: 64,
    };
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([420.0, 320.0])
            .with_title("haneng 설정")
            .with_icon(std::sync::Arc::new(icon)),
        ..Default::default()
    };
    eframe::run_native(
        "haneng-settings",
        options,
        Box::new(|cc| {
            install_korean_font(&cc.egui_ctx);
            Ok(Box::new(SettingsApp::load()))
        }),
    )
}

/// OS 시스템 한글 폰트를 찾아 egui에 등록한다. 못 찾으면 기본 폰트
/// (한글은 □로 표시)로 동작은 유지된다.
fn install_korean_font(ctx: &eframe::egui::Context) {
    use eframe::egui::{FontData, FontDefinitions, FontFamily};
    let candidates = [
        // macOS
        "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        // Windows
        "C:\\Windows\\Fonts\\malgun.ttf",
        // Linux
        "/usr/share/fonts/truetype/nanum/NanumGothic.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    ];
    let Some(bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) else {
        eprintln!("한글 폰트를 찾지 못했습니다 — 한글이 □로 보일 수 있습니다.");
        return;
    };
    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("korean".into(), FontData::from_owned(bytes).into());
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("korean".into());
    }
    ctx.set_fonts(fonts);
}

struct SettingsApp {
    config: Config,
    show_badge: bool,
    /// 질의 무응답 환경의 시작 모드: None=자동, Some(true)=한글, Some(false)=영문.
    initial_mode: Option<bool>,
    /// 배지 불투명도 퍼센트 (30~100).
    opacity: u8,
    /// 배지가 나타날 때 펄스 애니메이션으로 시선을 끌지 여부.
    animate: bool,
    status: String,
    update: Arc<Mutex<UpdateState>>,
}

impl SettingsApp {
    fn load() -> Self {
        let config = config::load_config();
        let show_badge = config.extra("hover_indicator") != Some("off");
        let initial_mode = match config.extra("initial_mode") {
            Some("korean") => Some(true),
            Some("english") => Some(false),
            _ => None,
        };
        let opacity = config.badge_opacity_percent();
        let animate = config.badge_animation();
        Self {
            config,
            show_badge,
            initial_mode,
            opacity,
            animate,
            status: String::new(),
            update: Arc::new(Mutex::new(UpdateState::Idle)),
        }
    }

    /// 백그라운드 스레드에서 최신 릴리스를 확인한다 (UI 블로킹 방지).
    fn start_update_check(&self, ctx: &eframe::egui::Context) {
        *self.update.lock().unwrap() = UpdateState::Checking;
        let state = self.update.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            // 어떤 경우에도 스피너가 영원히 남지 않도록 패닉도 오류로 바꾼다.
            let result = std::panic::catch_unwind(update::check)
                .unwrap_or_else(|_| UpdateState::Error("확인 중 내부 오류".into()));
            *state.lock().unwrap() = result;
            ctx.request_repaint();
        });
    }

    /// 업데이트 설치 시작 (Windows: MSI 설치 후 프로세스 종료).
    fn start_install(&self, tag: String, ctx: &eframe::egui::Context) {
        *self.update.lock().unwrap() = UpdateState::Installing;
        let state = self.update.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(move || update::install(&tag))
                .unwrap_or_else(|_| Err("설치 중 내부 오류".into()));
            *state.lock().unwrap() = match result {
                Ok(()) => UpdateState::Idle,
                Err(e) => UpdateState::Error(e),
            };
            ctx.request_repaint();
        });
    }

    fn save(&mut self) {
        self.config
            .set_extra("hover_indicator", if self.show_badge { "" } else { "off" });
        self.config.set_extra(
            "initial_mode",
            match self.initial_mode {
                None => "",
                Some(true) => "korean",
                Some(false) => "english",
            },
        );
        self.config
            .set_extra("badge_opacity", &self.opacity.to_string());
        self.config
            .set_extra("badge_animation", if self.animate { "" } else { "off" });
        let result = config::save_config(&self.config);
        self.status = match result {
            Ok(()) => {
                // Windows 데몬은 불투명도를 실행 중 다시 읽어 반영한다(재시작 불필요).
                // 나머지 설정과 다른 OS는 아직 재시작이 필요하다.
                let note = if cfg!(target_os = "windows") {
                    "불투명도는 곧 반영되고, 나머지는 데몬 재시작 시 적용됩니다."
                } else {
                    "데몬을 재시작하면 적용됩니다."
                };
                format!(
                    "저장됨: {} — {note}",
                    config::config_dir()
                        .map(|d| d.display().to_string())
                        .unwrap_or_default()
                )
            }
            Err(e) => format!("저장 실패: {e}"),
        };
    }
}

impl eframe::App for SettingsApp {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        {
            ui.heading("haneng — 한/영 상태 표시기");
            ui.small("입력창 위에 마우스를 올리면 현재 한/영 모드를 표시합니다.");
            ui.add_space(12.0);

            ui.checkbox(&mut self.show_badge, "한/영 배지 표시 (시작 기본값)");
            ui.add_space(8.0);

            ui.label("IME가 상태 조회에 응답하지 않을 때의 시작 모드");
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.initial_mode, None, "자동");
                ui.radio_value(&mut self.initial_mode, Some(true), "한글");
                ui.radio_value(&mut self.initial_mode, Some(false), "영문");
            });
            ui.add_space(12.0);

            ui.label("배지 불투명도 (낮을수록 투명 — 시야를 덜 가림)");
            ui.add(
                eframe::egui::Slider::new(&mut self.opacity, 30..=100)
                    .suffix("%")
                    .step_by(5.0),
            );
            ui.add_space(12.0);

            ui.checkbox(
                &mut self.animate,
                "나타날 때 반짝임 (배지가 뜨거나 한/영이 바뀔 때 눈에 띄게)",
            );
            ui.add_space(12.0);

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("저장").clicked() {
                    self.save();
                }
                if ui.button("다시 불러오기").clicked() {
                    *self = Self::load();
                }
            });
            if !self.status.is_empty() {
                ui.add_space(4.0);
                ui.small(&self.status);
            }

            ui.add_space(12.0);
            ui.separator();
            let update_state = self.update.lock().unwrap().clone();
            ui.horizontal(|ui| {
                ui.label(format!("버전 v{}", update::CURRENT_VERSION));
                match &update_state {
                    UpdateState::Idle | UpdateState::UpToDate | UpdateState::Error(_) => {
                        if ui.button("업데이트 확인").clicked() {
                            self.start_update_check(ui.ctx());
                        }
                    }
                    UpdateState::Checking => {
                        ui.spinner();
                        ui.label("확인 중...");
                    }
                    UpdateState::Available(tag) => {
                        let label = if cfg!(windows) {
                            format!("{tag}(으)로 업데이트")
                        } else {
                            format!("{tag} 다운로드 페이지 열기")
                        };
                        if ui.button(label).clicked() {
                            self.start_install(tag.clone(), ui.ctx());
                        }
                    }
                    UpdateState::Installing => {
                        ui.spinner();
                        ui.label("설치 준비 중... (설치가 시작되면 이 창은 닫힙니다)");
                    }
                }
            });
            match &update_state {
                UpdateState::UpToDate => {
                    ui.small("최신 버전입니다.");
                }
                UpdateState::Available(tag) => {
                    ui.small(format!(
                        "새 버전 {tag} 사용 가능. 업데이트 확인은 버튼을 눌렀을 때만 네트워크에 접속합니다."
                    ));
                }
                UpdateState::Error(e) => {
                    ui.small(format!("업데이트 확인 오류: {e}"));
                }
                _ => {}
            }
        }
    }
}
