# akra-hookers

## Codex activity kinds and canvas visibility

Akra installs and trusts both `UserPromptSubmit` and `SubagentStart` in each detected
Codex home. `SubagentStart` records Codex's official `agent_id` and `agent_type`; it
does not infer delegated work from prompt text. Existing Codex App/CLI prompt capture
continues through the shared `UserPromptSubmit` hook.

The canvas visibility controls are independent from capture and never delete stored
activity:

- **Subagent activity** is visible by default and can be hidden independently.
- **Codex internal activity** (ambient suggestions and background checks) is hidden
  by default and can be shown independently.
- Visibility choices are kept in local browser storage. Turning capture off or hiding
  a kind does not remove historical records.

Enabling capture writes exact trust hashes for both managed hooks to the matching
Codex `config.toml`. Restart that Codex installation after a hook definition changes;
the normal **Hooks need review** prompt should not appear for Akra-managed commands.

여러 터미널에서 Codex에 지시한 작업을 로컬에 기록하고, 캔버스에서 다시 찾기 위한 도구입니다.

- Codex `UserPromptSubmit` 훅을 받아 프롬프트와 작업 위치를 로컬 SQLite에 저장합니다.
- Git worktree는 같은 프로젝트로 묶습니다.
- 활동 기록은 불변이며, 캔버스 노드·위치·연결은 자유롭게 이동하거나 삭제할 수 있습니다.
- 대시보드에서 Windows Codex App/CLI와 각 WSL Codex의 캡처를 함께 찾고, 설치별 또는 전체로 켜고 끌 수 있습니다.
- Codex 토글은 각 설치의 `hooks.json`에서 Akra 훅만 등록하거나 제거합니다. 다른 훅은 유지됩니다.

## 개인정보와 범위

데이터는 로컬 SQLite와 로컬 spool에만 저장합니다. 기본 런타임은 `127.0.0.1`에만 바인딩하며, 클라우드 전송·텔레메트리·Git 변경은 하지 않습니다.

## 필수 보안 고지: Codex 훅 자동 신뢰

이 프로젝트는 개인 로컬 설치를 전제로 하며, `setup` 또는 대시보드의 Codex 캡처 활성화 시 다음 작업을 자동으로 수행합니다.

1. Akra `UserPromptSubmit` 명령을 해당 설치의 `hooks.json`에 기록합니다.
2. Codex와 같은 정규화 규칙으로 **그 Akra 명령만의 현재 신뢰 해시**를 계산합니다.
3. 해당 설치의 `config.toml` 내 `[hooks.state]`에 `enabled = true`와 `trusted_hash`를 기록합니다.

따라서 설치 후 Codex의 **Hooks need review** 화면은 표시되지 않으며 별도 수동 승인이 필요하지 않습니다. 이 자동 신뢰는 Akra가 생성한 정확한 명령과 위치에만 적용되고, 기존의 다른 훅이나 이후 외부에서 변경된 명령을 신뢰하지 않습니다.

Codex 훅은 신뢰된 뒤 샌드박스 밖에서 실행될 수 있습니다. Akra 훅은 사용자가 Codex에 제출하는 프롬프트, 작업 경로, 세션·턴 식별자와 모델 정보를 로컬 저장소로 전달합니다. 이 동작에 동의하지 않으면 `setup`을 실행하거나 캡처 토글을 켜지 마십시오. `disable` 또는 캡처 토글 해제 시 Akra 훅과 Akra가 기록한 신뢰 항목만 제거하며, 다른 Codex 설정과 훅은 보존합니다.

Codex의 훅 신뢰 모델은 [OpenAI Codex Hooks 문서](https://learn.chatgpt.com/docs/hooks)를 참고하십시오.

## 요구 사항

- Rust 2024 toolchain
- Node.js 20 이상
- Codex CLI

## 시작하기

```bash
# 감지된 Codex 설치에 훅 설치
cargo run -p akra-app -- setup

# 로컬 런타임 시작
cargo run -p akra-app -- serve --port 3000
```

`serve` 출력의 `url`과 `token`을 이용해 대시보드를 실행합니다.
`setup`, `capture`, `serve`는 기본적으로 같은 OS 사용자 데이터 디렉터리를 사용합니다.
필요하면 모든 명령에 동일한 `--data-dir <경로>`를 지정할 수 있습니다.

```bash
cd web
npm install
VITE_AKRA_URL=http://127.0.0.1:3000 \
VITE_AKRA_TOKEN=<serve가_출력한_token> \
npm run dev
```

브라우저에서 Vite가 출력한 주소를 열면 활동 캔버스와 Codex 캡처 설정을 볼 수 있습니다.

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
cargo run -p akra-app -- serve --port 3000

# 훅 payload 수동 수집 (표준 입력)
cat codex-hook.json | cargo run -p akra-app -- capture

# Codex 캡처 비활성화
cargo run -p akra-app -- disable
```

런타임이 꺼져 있어도 활성화된 `capture`는 SQLite를 열지 않고 payload를 spool에 안전하게 저장한 뒤 즉시 종료합니다.
대시보드 또는 `disable`로 끈 뒤 늦게 호출된 기존 hook은 spool을 만들지 않고 즉시 종료합니다.
이미 spool에 수락된 payload는 이후 `serve` 시작 시 SQLite로 복구됩니다.

## 개발 검증

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm run --prefix web test
npm run --prefix web build
```
