# akra-hookers

## Codex activity and canvas visibility

Akra installs and trusts only `UserPromptSubmit` and `Stop` in each detected Codex home.
It does not install `SubagentStart`, and delegated-agent sessions discovered through
Codex metadata are discarded before spooling, remote delivery, or SQLite storage.
`Stop` captures a user turn's final assistant result so the local runtime can attach a
three-line summary. Upgrading removes Akra's older managed `SubagentStart` hook and
deletes historical subagent activity while preserving user and internal activity.

**문맥 기반 프롬프트 요약**은 기본으로 꺼져 있습니다. Smart mode를 명시적으로
켜면 새 user activity 중 문맥이 필요한 요청만 `gpt-5.3-codex-spark`로 한 문장,
96자 이하로 정리합니다. 이때 전송하는 문맥은 현재 요청에서 보수적으로 분리한
projected prompt와 바로 이전 turn의 저장된 3줄 결과 요약뿐입니다. 이전 요청 원문,
이전 assistant 원문, transcript는 보내지 않습니다. 수집 원문은 활동 증거로 그대로
보존하며 상세 화면의 `수집된 원문 보기`에서 확인할 수 있습니다.

The canvas visibility control is independent from capture and never deletes stored
activity:

- **Codex internal activity** (ambient suggestions and background checks) is hidden
  by default and can be shown independently.
- The visibility choice is kept in local browser storage. Turning capture off or hiding
  internal activity does not remove historical records.

## 로컬 및 원격 수집

기본 수집 대상은 `http://127.0.0.1:<port>`입니다. `localhost`, `127.0.0.0/8`,
`[::1]`만 로컬 대상으로 취급하며, 이 경우 훅 payload는 같은 머신의
`<data-dir>/spool`과 SQLite로만 들어갑니다. 외부 호스트, 사설망 주소, 도메인은
반드시 `https://` 주소여야 합니다. URL에는 경로, query, fragment, 사용자 정보
(`user@host`)를 넣을 수 없습니다.

원격 수집은 명시적인 opt-in입니다. 수집을 받는 머신에서 아래처럼 runtime을
실행하고 collector access token을 한 번 확인합니다.

```bash
# collector 머신: 기본 바인딩은 127.0.0.1입니다.
# 외부 HTTPS reverse proxy 뒤에서만 명시적으로 외부 인터페이스를 사용하세요.
cargo run -p akra-app -- serve --bind 0.0.0.0 --port 42130
cargo run -p akra-app -- collector-token
```

그 다음 **프롬프트를 입력하는 source 머신**의 대시보드에서 **Collection
destination**을 열고, collector의 `https://...` 주소와 access token을 저장합니다.
주소와 토큰은 훅 명령이나 `hooks.json`에 기록되지 않습니다. 저장 직후 적용되며,
destination만 바꿀 때는 Codex를 다시 시작하거나 훅 신뢰를 다시 승인할 필요가
없습니다. endpoint metadata는 `<data-dir>/collector.json`, access token은 별도
`<data-dir>/collector-remote.token` credential 파일에 원자적으로 저장됩니다.

원격 수집을 선택하면 source 머신은 다음 정보를 해당 HTTPS collector에 보냅니다:

- 사용자 프롬프트와 Codex provider/model 정보
- 작업 경로와 project origin, session/turn 식별자, 활동 종류
- `Stop` 훅의 최종 assistant 결과(collector에서 3줄 결과 요약을 만들기 위한 원문)

원격 `Stop` 결과의 3줄 요약은 collector 머신에서 실행됩니다. 따라서 요약까지
사용하려면 collector에 로그인된 Codex CLI와 `gpt-5.3-codex-spark`가 필요합니다.
collector에 실행 가능한 Codex runtime이 없으면 프롬프트 기록은 보존되지만 결과
요약은 실패 상태로 표시됩니다.

source는 먼저 `<data-dir>/remote-outbox`에 로컬 내구성 큐를 기록한 뒤 짧게
전송을 시도합니다. collector가 꺼져 있거나 TLS/token 검증에 실패해도 Codex 훅은
실패시키지 않고 큐를 유지하며, runtime은 bounded backoff로 재시도합니다. 목적지를
변경해도 기존 큐는 새 목적지로 자동 전송되지 않습니다. 이전 collector로 다시
돌리거나 로컬에서 명시적으로 처리해 주세요. collector access token은 대시보드
capability token과 별개이며, API 응답·로그·훅 명령에 반환되지 않습니다.

원격 모드의 활동 노드와 대화 기록은 **collector 대시보드**에 저장됩니다. source
대시보드는 source 자신의 destination과 delivery 상태만 보여 주며, collector를
원격 제어하거나 원격 활동을 자동으로 복제하지 않습니다.
문맥 기반 프롬프트 요약도 활동을 보관하는 collector 대시보드에서 설정합니다. source가
원격 collector를 사용 중이면 source 화면의 요약 토글은 비활성화되고, source metadata가
collector의 Codex executable이나 `CODEX_HOME` 선택에 영향을 주지 않습니다.

Akra 자체는 TLS를 종료하지 않습니다. 공개 collector는 신뢰할 수 있는 HTTPS
reverse proxy와 방화벽/네트워크 접근 제어 뒤에 두세요. dashboard와 collector
ingress는 같은 listener를 사용하므로 raw `http://0.0.0.0:...`를 인터넷에 직접
노출하면 안 됩니다.

Enabling capture writes exact trust hashes for all three managed hooks to the matching
Codex `config.toml`. Restart that Codex installation after a hook definition changes;
the normal **Hooks need review** prompt should not appear for Akra-managed commands.

Result summarization is installation-bound. Every managed hook records its exact
`--codex-home`, and the runtime uses the Codex executable detected for that same
capture target. On Windows it probes launchable Desktop/npm native binaries and skips
inaccessible WindowsApps aliases. In WSL it invokes the detected absolute Linux
binary with `CODEX_HOME` and `AKRA_HOOKERS_SUMMARY_CHILD=1` passed through `env`, so
the summary child cannot recursively trigger Akra capture. Set
`AKRA_CODEX_EXECUTABLE` only when an explicit native Codex binary is required.

The summary subprocess has one 60-second deadline covering stdin delivery, process
completion, and bounded stdout/stderr draining. Deadline expiry kills and reaps the
child. Existing two-hook Akra manifests are treated as installed during startup
reconciliation, upgraded in place to the current three-hook contract, and never move
the user's selected installation to another detected target.

여러 터미널에서 Codex에 지시한 작업을 로컬에 기록하고, 캔버스에서 다시 찾기 위한 도구입니다.

- Codex `UserPromptSubmit` 훅을 받아 프롬프트와 작업 위치를 로컬 SQLite에 저장합니다.
- 같은 턴의 `Stop` 결과를 받아 `gpt-5.3-codex-spark`로 정확히 3줄, 세 줄 합계 180자 이하로 요약해 활동에 연결합니다.
- 선택한 Smart mode에서는 문맥이 필요한 새 user prompt만 현재 projected request와 이전 3줄 결과 요약으로 한 문장, 96자 이하로 정리해 노드에 표시합니다.
- 프로젝트의 `로그 정리`에서 사용자가 고른 최대 20개 로그만 Spark가 작업 후보로
  묶습니다. 제안은 저장 상태를 바꾸지 않으며, 사용자가 이름·소속을 검토하고
  `적용`해야 작업 노드가 생성됩니다. 같은 session은 약한 참고 신호일 뿐 같은
  작업으로 간주하지 않습니다.
- 작업 노드는 여러 프롬프트-결과 로그를 하나의 사용자 확인 작업으로 보여 줍니다.
  원본 로그는 근거로 남고, 새 작업 사이 관계선은 자동 생성하지 않으며 사용자가
  작업 지도에서 직접 연결합니다.
- Git worktree는 같은 프로젝트로 묶습니다.
- 활동 내용은 덮어쓰지 않는 근거이며, 캔버스 배치와 작업 소속은 별도입니다. 정리
  화면의 명시적 삭제 확인만 활동에 `deletedAt` tombstone을 적용해 모든 일반 조회에서
  숨기고, 작업 노드 제거는 원본 로그를 삭제하지 않고 정리 대기로 되돌립니다.
- 대시보드에서 Windows Codex App/CLI와 각 WSL Codex의 캡처를 함께 찾고, 설치별 또는 전체로 켜고 끌 수 있습니다.
- Codex 토글은 각 설치의 `hooks.json`에서 Akra 훅만 등록하거나 제거합니다. 다른 훅은 유지됩니다.

## 개인정보와 범위

사용자 입력 프롬프트, 작업 위치와 활동 데이터는 기본적으로 로컬 SQLite와 로컬 spool에 저장합니다. 기본 런타임은 `127.0.0.1`에만 바인딩하며 텔레메트리나 Git 변경을 수행하지 않습니다. 사용자가 Collection destination에 외부 HTTPS collector와 access token을 명시적으로 저장한 경우에만, 위의 원격 수집 범위에 적은 데이터가 그 collector로 전송됩니다.

결과 요약은 예외입니다. Codex의 최종 assistant 결과(`Stop.last_assistant_message`)만 해당 활동을 저장한 인증된 Codex summary runtime의 `codex exec --model gpt-5.3-codex-spark`에 전달합니다. 저장된 사용자 입력 프롬프트는 **결과 요약 요청**에 포함하지 않습니다. 원문 결과는 자동 재시도와 사용자가 명시적으로 누르는 `재생성`을 위해 로컬에 최대 24시간만 일시 보관됩니다. 요약 성공 시 즉시 삭제하며, 실패 상태여도 24시간이 지나면 다음 runtime recovery 또는 재생성 요청에서 먼저 삭제합니다. 따라서 이미 원문이 삭제된 과거 기록에는 재생성 버튼이 나타나지 않습니다. 장기 저장되는 결과 데이터는 정확히 3줄이며, 앞뒤 공백을 제거한 세 줄의 Unicode scalar 수 합계가 180자 이하인 경우만 허용됩니다. 줄 구분자는 합계에서 제외합니다. Spark를 사용할 수 없거나 인증·네트워크·출력 검증에 실패하면 다른 모델로 대체하지 않고 요약 상태를 실패로 표시합니다.

문맥 기반 프롬프트 요약은 별도 opt-in입니다. Smart mode에서는 현재 user request의
결정론적 projection과, 필요할 때 같은 session의 바로 이전 user activity에 이미
저장된 3줄 결과 요약만 Spark에 전달합니다. 이전 user prompt 원문, 이전 assistant
원문, Codex transcript는 전달하지 않습니다. 출력은 Markdown·줄바꿈 없는 한국어 한
문장이고 Unicode scalar 기준 96자 이하만 저장됩니다. 생성 실패, timeout, 출력 검증
실패 시 다른 모델로 대체하거나 원문을 덮어쓰지 않고 projected prompt를 표시합니다.
설정을 켠 뒤 새 user activity부터 적용하며 기존 기록을 일괄 backfill하지 않습니다.

작업 후보 생성도 별도의 명시적 요청입니다. 선택된 로그의 96자 이하 프롬프트 요약
(요약이 없으면 결정론적으로 압축한 최대 96자 요청 미리보기), 저장된 3줄 결과 요약,
익명화된 session group, 같은 프로젝트에서 로컬로 추린 최대 5개 기존 작업만
`gpt-5.3-codex-spark`에 보냅니다. 전체 수집 원문, assistant 원문, transcript,
실제 session/turn ID와 작업 경로는 보내지 않습니다. 한 요청은 최대
20개 로그·64 KiB이고, 동일 fingerprint의 제안은 저장된 결과를 재사용합니다.
모델은 로그를 삭제하거나 작업 관계선을 생성할 권한이 없으며, 사용자가 검토 후
적용하기 전에는 작업·로그 소속이 변경되지 않습니다.

## 필수 보안 고지: Codex 훅 자동 신뢰

이 프로젝트는 개인 로컬 설치를 전제로 하며, `setup` 또는 대시보드의 Codex 캡처 활성화 시 다음 작업을 자동으로 수행합니다.

1. Akra `UserPromptSubmit`, `Stop` 명령을 해당 설치의 `hooks.json`에 기록합니다. 기존 Akra 관리 `SubagentStart` 항목은 제거합니다.
2. Codex와 같은 정규화 규칙으로 **그 Akra 명령만의 현재 신뢰 해시**를 계산합니다.
3. 해당 설치의 `config.toml` 내 `[hooks.state]`에 `enabled = true`와 `trusted_hash`를 기록합니다.

따라서 설치 후 Codex의 **Hooks need review** 화면은 표시되지 않으며 별도 수동 승인이 필요하지 않습니다. 이 자동 신뢰는 Akra가 생성한 정확한 명령과 위치에만 적용되고, 기존의 다른 훅이나 이후 외부에서 변경된 명령을 신뢰하지 않습니다.

Codex 훅은 신뢰된 뒤 샌드박스 밖에서 실행될 수 있습니다. Akra 훅은 사용자가 Codex에 제출하는 프롬프트, 작업 경로, 세션·턴 식별자, 모델 정보와 최종 assistant 결과를 로컬 runtime으로 전달합니다. runtime은 위 개인정보 계약에 따라 최종 결과만 기본 Spark 요약에 사용하며, 사용자가 Smart mode를 켠 경우에만 제한된 projected request와 직전 3줄 결과 요약을 prompt Spark 요약에 사용합니다. 이 동작에 동의하지 않으면 `setup`을 실행하거나 캡처 토글을 켜지 마십시오. `disable` 또는 캡처 토글 해제 시 Akra 훅과 Akra가 기록한 신뢰 항목만 제거하며, 다른 Codex 설정과 훅은 보존합니다.

Codex의 훅 신뢰 모델은 [OpenAI Codex Hooks 문서](https://learn.chatgpt.com/docs/hooks)를 참고하십시오.

## 요구 사항

- Rust 2024 toolchain
- Node.js 20 이상
- 로그인된 Codex CLI와 최신 모델 카탈로그의 `gpt-5.3-codex-spark`

## 시작하기

```bash
# 감지된 Codex 설치에 훅 설치
cargo run -p akra-app -- setup

# 로컬 런타임 시작
cargo run -p akra-app -- serve --port 42130
```

`serve` 출력의 `url`과 `token`을 이용해 대시보드를 실행합니다.
`setup`, `capture`, `serve`는 기본적으로 같은 OS 사용자 데이터 디렉터리를 사용합니다.
필요하면 모든 명령에 동일한 `--data-dir <경로>`를 지정할 수 있습니다.

- Windows 네이티브 CLI와 공식 portable: `%LOCALAPPDATA%\akra-hookers`
- Linux/Ubuntu: `$XDG_DATA_HOME/akra-hookers`, 미설정 시 `$HOME/.local/share/akra-hookers`
- macOS: `$HOME/Library/Application Support/akra-hookers`
- iOS: 앱 sandbox의 `$HOME/Library/Application Support/akra-hookers`; 향후 네이티브 host는
  OS가 제공한 Application Support 경로를 `AKRA_HOOKERS_DATA_DIR`로 명시할 수 있습니다.

`AKRA_HOOKERS_DATA_DIR`를 설정하면 모든 OS 기본값보다 우선합니다. 따라서 한 OS 안의
CLI, 데스크톱 shell과 hook은 동일한 값을 사용해야 하며, 서로 다른 OS의 로컬 경로를
공유 경로로 간주하지 않습니다.

```bash
cd web
npm install
VITE_AKRA_URL=http://127.0.0.1:42130 \
VITE_AKRA_TOKEN=<serve가_출력한_token> \
npm run dev # http://127.0.0.1:42131
```

브라우저에서 Vite가 출력한 주소를 열면 원본 활동 로그, 사용자 확인 작업 지도,
로그 정리 워크스페이스와 Codex 캡처 설정을 볼 수 있습니다.

## Electron 데스크톱 앱

Windows 데스크톱 빌드는 현재 React 대시보드와 Rust runtime을 하나의 Electron 앱으로
묶습니다. 앱은 두 구성 요소를 임의의 `127.0.0.1` 포트에서만 실행하며 API token은
URL·빌드 파일·렌더러 저장소에 기록하지 않고 제한된 preload bridge로 전달합니다.
ZIP 배포본도 설치 위치와 무관하게 `%LOCALAPPDATA%\akra-hookers`를 사용합니다. SQLite,
spool과 collector 설정은 이 루트에, Electron 설정은 `electron`, 안정된 sidecar는 `bin`
하위에 저장됩니다. 따라서 CLI와 portable이 같은 기록을 보며 ZIP을 옮기거나 새 버전을
다른 폴더에 풀어도 데이터 경로와 Codex hook 명령은 바뀌지 않습니다. 이전 배포본이
만든 실행 파일 옆 `Akra Hookers Data`는 자동 삭제하지 않습니다.

```powershell
cd desktop
npm install
npm run build
```

실행 가능한 portable 앱은 `desktop/dist/Akra Hookers-win32-x64/Akra Hookers.exe`에
생성됩니다. 로컬 개발 실행은 `npm start`를 사용합니다. 현재 산출물은 코드 서명되지
않았으므로 다른 PC에 배포할 때는 Windows 코드 서명과 설치 프로그램 단계를 추가해야
합니다. macOS 빌드와 서명·notarization은 macOS 호스트에서 Application Support 기반의
동일한 sidecar 계약으로 진행합니다. Electron은 iOS를 지원하지 않으므로 iOS 분기는
향후 네이티브 shell/runtime의 sandbox 데이터 경로 계약입니다.

Windows 네이티브 Codex App과 CLI는 `%USERPROFILE%\.codex`를 하나의 대상으로 사용합니다.
둘은 같은 `hooks.json`을 공유하므로 설치 토글도 하나이며, 대시보드는 실제로 수집된 프롬프트 증거를 기준으로 App과 CLI의 캡처 확인 상태를 각각 표시합니다.
WSL은 배포판별 Linux `~/.codex`를 별도 대상으로 감지하며 Docker 내부 배포판은 제외합니다.
Windows 또는 WSL에 별도 `CODEX_HOME`이 있으면 함께 감지합니다. Windows와 WSL이 같은 물리 경로를 공유하면 하나의 대상으로 합치되, manifest에는 Linux용 `command`와 Windows용 `commandWindows`를 각각 기록합니다. `--home`을 명시한 CLI 작업만 해당 경로로 격리됩니다.
WSL 자동 검색이 필요 없는 격리 환경에서는 `AKRA_HOOKERS_SKIP_WSL=1`로 명시적으로 끌 수 있습니다.
훅을 새로 설치하거나 변경한 뒤에는 해당 Codex를 다시 시작하십시오. Akra가 현재 훅 정의의 신뢰 해시까지 함께 갱신하므로 `/hooks`에서 별도로 승인할 필요가 없습니다.

## 주요 명령

```bash
# 설치/상태
cargo run -p akra-app -- setup
cargo run -p akra-app -- status

# 로컬 런타임
cargo run -p akra-app -- serve --port 42130

# collector 머신에서 source에 전달할 별도 수집 토큰 확인
cargo run -p akra-app -- collector-token

# 훅 payload 수동 수집 (표준 입력)
cat codex-hook.json | cargo run -p akra-app -- capture

# Codex 캡처 비활성화
cargo run -p akra-app -- disable
```

런타임이 꺼져 있어도 활성화된 `capture`는 SQLite를 열지 않고 payload를 spool에 안전하게 저장한 뒤 즉시 종료합니다.
대시보드 또는 `disable`로 끈 뒤 늦게 호출된 기존 hook은 spool을 만들지 않고 즉시 종료합니다.
이미 spool에 수락된 payload는 이후 `serve` 시작 시 SQLite로 복구됩니다.
요약 child는 hook 재귀를 막기 위해 `--disable hooks`, 격리된 빈 작업 디렉터리,
read-only sandbox, 임시 세션을 사용하며 shell·app·plugin·multi-agent·web search를 비활성화합니다.

## 개발 검증

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm run --prefix web test
npm run --prefix web build
```
