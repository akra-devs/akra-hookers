# akra-hookers

여러 터미널에서 Codex에 지시한 작업을 로컬에 기록하고, 캔버스에서 다시 찾기 위한 도구입니다.

- Codex `UserPromptSubmit` 훅을 받아 프롬프트와 작업 위치를 로컬 SQLite에 저장합니다.
- Git worktree는 같은 프로젝트로 묶습니다.
- 활동 기록은 불변이며, 캔버스 노드·위치·연결은 자유롭게 이동하거나 삭제할 수 있습니다.
- 대시보드에서 provider별 캡처를 켜고 끌 수 있습니다. 끄더라도 기존 기록과 실행 중인 에이전트에는 영향이 없습니다.
- Codex 토글은 전역 `~/.codex/hooks.json`의 akra hook을 직접 등록하거나 제거합니다. 다른 hook은 유지됩니다.

## 개인정보와 범위

데이터는 로컬 SQLite와 로컬 spool에만 저장합니다. 기본 런타임은 `127.0.0.1`에만 바인딩하며, 클라우드 전송·텔레메트리·Git 변경은 하지 않습니다.

## 요구 사항

- Rust 2024 toolchain
- Node.js 20 이상
- Codex CLI

## 시작하기

```bash
# Codex 전역 훅 설치 (~/.codex/hooks.json)
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

런타임이 꺼져 있어도 `capture`는 SQLite를 열지 않고 payload를 spool에 안전하게 저장한 뒤 즉시 종료합니다.
다음 `serve` 시작 시 SQLite로 복구됩니다.

## 개발 검증

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

npm run --prefix web test
npm run --prefix web build
```
