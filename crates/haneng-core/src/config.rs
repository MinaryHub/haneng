//! 설정과 예외 사전의 파일 영속화 — 의존성 없는 평문 포맷.
//!
//! - `config.txt`: `키 = 값` 한 줄씩 (`auto`, `sensitivity`).
//! - `exceptions.txt`: 되돌리기로 학습한 단어 한 줄씩.
//!
//! 키 입력 자체는 어떤 경우에도 기록하지 않는다(PLAN.md N2) — 예외 사전에는
//! 사용자가 명시적으로 되돌린 단어만 들어간다.

use crate::detect::Sensitivity;
use crate::layout::Layout;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// 단어 경계 자동 교정 활성화 여부.
    pub auto: bool,
    pub sensitivity: Sensitivity,
    pub layout: Layout,
    /// 교정을 끌 앱 이름 조각들 (`disabled_apps = terminal, slack`).
    /// 실행 파일/프로세스/윈도 클래스 이름에 대한 대소문자 무시 부분 일치.
    pub disabled_apps: Vec<String>,
    /// 코어가 모르는 키들 — 플랫폼 어댑터 전용 설정(`키 = 값` 그대로).
    pub extras: Vec<(String, String)>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auto: true,
            sensitivity: Sensitivity::Balanced,
            layout: Layout::Dubeolsik,
            disabled_apps: Vec::new(),
            extras: Vec::new(),
        }
    }
}

impl Config {
    /// `키 = 값` 줄들을 파싱한다. 모르는 키/값은 무시하고 기본값 유지.
    pub fn parse(text: &str) -> Self {
        let mut config = Self::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim().to_ascii_lowercase();
            match key.trim() {
                "auto" => match value.as_str() {
                    "on" | "true" | "1" => config.auto = true,
                    "off" | "false" | "0" => config.auto = false,
                    _ => {}
                },
                "sensitivity" => match value.as_str() {
                    "conservative" => config.sensitivity = Sensitivity::Conservative,
                    "balanced" => config.sensitivity = Sensitivity::Balanced,
                    "aggressive" => config.sensitivity = Sensitivity::Aggressive,
                    _ => {}
                },
                "layout" => match value.as_str() {
                    "dubeolsik" | "2" => config.layout = Layout::Dubeolsik,
                    "sebeolsik-390" | "sebeolsik390" | "390" => {
                        config.layout = Layout::Sebeolsik390
                    }
                    "sebeolsik-final" | "sebeolsikfinal" | "3f" => {
                        config.layout = Layout::SebeolsikFinal
                    }
                    _ => {}
                },
                "disabled_apps" => {
                    config.disabled_apps = value
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect();
                }
                key => config.extras.push((key.to_string(), value)),
            }
        }
        config
    }

    /// 어댑터 전용 설정 키 조회.
    pub fn extra(&self, key: &str) -> Option<&str> {
        self.extras
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// 어댑터 전용 설정 키 쓰기 (빈 값은 키 제거).
    pub fn set_extra(&mut self, key: &str, value: &str) {
        self.extras.retain(|(k, _)| k != key);
        if !value.is_empty() {
            self.extras.push((key.to_string(), value.to_string()));
        }
    }

    /// 이 앱에서 교정을 꺼야 하는가 (대소문자 무시 부분 일치).
    pub fn app_disabled(&self, app_name: &str) -> bool {
        let name = app_name.to_lowercase();
        self.disabled_apps.iter().any(|frag| name.contains(frag))
    }

    /// 배지 불투명도 퍼센트 (`badge_opacity`, 30..=100, 기본 80).
    /// 시야를 가리지 않도록 반투명하게 표시하는 기본값.
    pub fn badge_opacity_percent(&self) -> u8 {
        self.extra("badge_opacity")
            .and_then(|v| v.trim().trim_end_matches('%').parse::<u8>().ok())
            .unwrap_or(80)
            .clamp(30, 100)
    }

    /// 배지가 나타날 때 시선을 끄는 펄스 애니메이션을 재생하는가
    /// (`badge_animation`, 기본 켜짐). `off`일 때만 끈다.
    pub fn badge_animation(&self) -> bool {
        self.extra("badge_animation") != Some("off")
    }

    /// config.txt 포맷으로 직렬화한다. 모르는 키(extras)도 보존된다.
    pub fn serialize(&self) -> String {
        let mut out = String::from("# haneng 설정 — 데몬 재시작 시 적용\n");
        out.push_str(&format!(
            "auto = {}\n",
            if self.auto { "on" } else { "off" }
        ));
        out.push_str(&format!(
            "sensitivity = {}\n",
            match self.sensitivity {
                Sensitivity::Conservative => "conservative",
                Sensitivity::Balanced => "balanced",
                Sensitivity::Aggressive => "aggressive",
            }
        ));
        out.push_str(&format!(
            "layout = {}\n",
            match self.layout {
                Layout::Dubeolsik => "dubeolsik",
                Layout::Sebeolsik390 => "sebeolsik-390",
                Layout::SebeolsikFinal => "sebeolsik-final",
            }
        ));
        if !self.disabled_apps.is_empty() {
            out.push_str(&format!(
                "disabled_apps = {}\n",
                self.disabled_apps.join(", ")
            ));
        }
        for (key, value) in &self.extras {
            out.push_str(&format!("{key} = {value}\n"));
        }
        out
    }
}

/// OS 관례에 따른 설정 디렉터리 (`…/haneng`).
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join("Library/Application Support/haneng"))
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("haneng"))
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("haneng"))
    }
}

pub fn load_config() -> Config {
    config_dir()
        .and_then(|dir| fs::read_to_string(dir.join("config.txt")).ok())
        .map(|text| Config::parse(&text))
        .unwrap_or_default()
}

pub fn load_exceptions() -> Vec<String> {
    config_dir()
        .and_then(|dir| fs::read_to_string(dir.join("exceptions.txt")).ok())
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// 설정을 config.txt에 저장한다 (디렉터리 없으면 생성).
pub fn save_config(config: &Config) -> std::io::Result<()> {
    let Some(dir) = config_dir() else {
        return Err(std::io::Error::other("설정 디렉터리를 찾을 수 없음"));
    };
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("config.txt"), config.serialize())
}

/// 예외 사전 전체를 다시 쓴다 (설정 UI의 항목 삭제용).
pub fn save_exceptions(words: &[String]) -> std::io::Result<()> {
    let Some(dir) = config_dir() else {
        return Err(std::io::Error::other("설정 디렉터리를 찾을 수 없음"));
    };
    fs::create_dir_all(&dir)?;
    let mut text = words.join("\n");
    if !text.is_empty() {
        text.push('\n');
    }
    fs::write(dir.join("exceptions.txt"), text)
}

/// 되돌리기로 학습한 단어를 예외 사전 파일에 추가한다.
pub fn append_exception(word: &str) -> std::io::Result<()> {
    let Some(dir) = config_dir() else {
        return Err(std::io::Error::other("설정 디렉터리를 찾을 수 없음"));
    };
    fs::create_dir_all(&dir)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("exceptions.txt"))?;
    writeln!(file, "{word}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_config_and_ignores_junk() {
        let config = Config::parse(
            "# 주석\n\
             auto = off\n\
             sensitivity = aggressive\n\
             disabled_apps = Terminal, slack\n\
             linux_toggle_keycodes = 130,108\n\
             malformed line\n",
        );
        assert!(!config.auto);
        assert_eq!(config.sensitivity, Sensitivity::Aggressive);
        assert_eq!(config.extra("linux_toggle_keycodes"), Some("130,108"));
        assert_eq!(config.extra("missing"), None);
        assert!(config.app_disabled("com.apple.Terminal"));
        assert!(config.app_disabled("Slack Helper"));
        assert!(!config.app_disabled("Safari"));
    }

    #[test]
    fn badge_opacity_defaults_and_clamps() {
        assert_eq!(Config::default().badge_opacity_percent(), 80);
        assert_eq!(
            Config::parse("badge_opacity = 50").badge_opacity_percent(),
            50
        );
        assert_eq!(
            Config::parse("badge_opacity = 90%").badge_opacity_percent(),
            90
        );
        assert_eq!(
            Config::parse("badge_opacity = 5").badge_opacity_percent(),
            30
        );
        assert_eq!(
            Config::parse("badge_opacity = 200").badge_opacity_percent(),
            100
        );
        // 애니메이션은 기본 켜짐 — off로만 끈다.
        assert!(Config::default().badge_animation());
        assert!(!Config::parse("badge_animation = off").badge_animation());
        assert!(Config::parse("badge_animation = on").badge_animation());
        assert_eq!(
            Config::parse("badge_opacity = x").badge_opacity_percent(),
            80
        );
    }

    #[test]
    fn empty_text_gives_defaults() {
        assert_eq!(Config::parse(""), Config::default());
    }

    #[test]
    fn serialize_parse_roundtrip() {
        let config = Config {
            auto: false,
            sensitivity: Sensitivity::Aggressive,
            layout: Layout::Sebeolsik390,
            disabled_apps: vec!["terminal".into(), "slack".into()],
            extras: vec![("linux_toggle_keycodes".into(), "130,108".into())],
        };
        assert_eq!(Config::parse(&config.serialize()), config);
    }
}
