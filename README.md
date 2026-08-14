# haneng — 한/영 상태 표시기

지금 한글 모드인지 영문 모드인지, 타이핑하기 전에는 알 수 없어서
`gksrmf`라고 쳐 본 적 있다면 — 이 도구가 알려줍니다.

haneng은 **입력 카렛(입력 커서) 옆에 현재 입력 상태 배지**를 띄우는 상주
유틸리티입니다. **Windows 전용**입니다.

- 파란 **한** — 한글 모드
- 회색 **a** — 영문 소문자
- 주황 **A** — 영문 + Caps Lock (실수로 켜둔 Caps Lock을 한눈에)

## 동작 방식과 프라이버시

- 표시 위치: 포커스된 입력 컨트롤의 카렛(`GUITHREADINFO`). 자체 카렛을
  그리는 앱(크롬·Electron 등)은 UI Automation으로 폴백합니다. 마우스
  위치와는 무관합니다.
- 모드 판별: 포커스 컨트롤의 IME 상태 조회(`IMC_GETOPENSTATUS`),
  Caps Lock은 `GetKeyState` 토글 비트.
- **키보드를 관찰하지 않습니다** — 입력 훅이 없습니다.
- **텍스트를 지우거나 입력하지 않습니다.** 카렛 위치와 모드만 읽습니다.
- **네트워크 없음** — 설정 창에서 업데이트 확인 버튼을 눌렀을 때만 예외.

> 참고: v0.3.x까지 있던 한↔영 자동/수동 변환 기능은 v0.4.0에서,
> macOS·Linux 버전은 v0.9.0에서 제거했습니다. 전체 구현은 각각 git 태그
> `v0.3.2`·`v0.8.13`에 보존되어 있습니다.

## 설치

**[📦 최신 릴리스 다운로드](https://github.com/knoxxr/haneng/releases/latest)**

| OS | 파일 | 참고 |
|---|---|---|
| Windows 10/11 | `haneng-windows.msi` (권장) / `.zip` | 설치 즉시 실행 + 로그인 자동 시작. SmartScreen 경고 시 "추가 정보 → 실행" |

트레이 아이콘: 배지 표시 토글 · 설정 · 종료.
설정 창의 **업데이트 확인** 버튼으로 새 버전을 원클릭 설치할 수 있습니다.

## 설정

설정 창에서 편집하거나 `%APPDATA%\haneng\config.txt`를 직접 수정합니다:

```ini
hover_indicator = on     # off = 배지 끔
initial_mode = korean    # 모드 조회 불가 환경의 초기 표시 (korean|english)
ime_query = on           # off = 실시간 IME 조회 끔
badge_opacity = 80       # 배지 불투명도 퍼센트 (30~100)
badge_animation = on     # off = 나타날 때 반짝임 끔
debug_log = off          # on = 표시/숨김 판정 이유를 debug.log에 기록(진단용)
```

## 알려진 한계

- 서명/공증이 없어 OS 보안 경고가 뜹니다.
- 표준 카렛도 UI Automation 텍스트 패턴도 제공하지 않는 앱에서는 배지가
  뜨지 않습니다.
- 파워포인트처럼 편집이 끝나도 카렛을 남기는 앱에서 배지가 늦게 사라질 수
  있습니다(`debug_log = on`으로 판정 이유를 남길 수 있습니다).

## 개발

```sh
cargo test                                              # 전체 테스트
cargo clippy --all-targets                              # 린트
cargo run -p haneng-settings                            # 설정 창
cargo check -p haneng-windows --target x86_64-pc-windows-msvc   # 다른 OS에서 교차 확인
```

아키텍처는 [CLAUDE.md](CLAUDE.md), 서드파티 데이터 출처는 [NOTICE.md](NOTICE.md) 참고.

## 버전 관리 / 릴리스

- [SemVer](https://semver.org/lang/ko/), 루트 `Cargo.toml`의
  `[workspace.package] version`이 단일 출처. 이력은 [CHANGELOG.md](CHANGELOG.md).
- 릴리스: `scripts/release.sh <버전>` 후 `git push origin main --tags`
  → CI가 Windows 패키지를 빌드해 드래프트 릴리스에 첨부.

## 라이선스

MIT — [LICENSE](LICENSE)
