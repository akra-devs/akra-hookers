# Hook Capture Performance Improvement Plan

- 상태: 1번 구현 완료
- 작성일: 2026-08-24
- 구현 상태: hook 원격 전송 분리 및 relay 후보 필터 순서 반영
- 범위: Codex hook capture와 remote collector 전달 경로

## 1. 승인된 결정: hook과 원격 전송 분리

### 현재 동작

원격 수집 모드의 capture hook은 payload를 durable outbox에 기록한 뒤 `relay_once()`를 호출하고 최대 850ms 동안 결과를 기다린다.

이 구조에는 두 가지 문제가 있다.

1. 원격 collector가 응답하지 않으면 prompt와 `Stop` hook이 각각 최대 850ms 지연될 수 있다.
2. relay가 정렬된 pending 항목 중 앞의 8개를 먼저 선택한 다음 retry 시각과 destination을 검사한다. 앞의 항목이 전송 불가능한 상태면 뒤의 전송 가능한 항목을 검사하지 못한다.

관련 코드:

- `crates/akra-app/src/main.rs`: remote outbox enqueue 뒤 850ms inline relay
- `crates/akra-app/src/collector.rs`: relay candidate 선택, retry, destination 검사
- `crates/akra-app/src/spool/queue.rs`: pending 항목 정렬

### 승인된 동작

Capture hook은 다음 작업까지만 수행한다.

1. payload를 검증하고 `CaptureEnvelope`를 만든다.
2. remote outbox에 durable enqueue를 완료한다.
3. 네트워크 응답을 기다리지 않고 종료한다.

장기 실행 중인 `serve` relay worker가 원격 전송을 담당한다.

1. 현재 destination에 속한 항목을 찾는다.
2. retry 시각이 지난 항목만 남긴다.
3. 전송 가능한 항목 중 최대 8개를 선택한다.
4. 성공한 항목을 acknowledge한다.
5. 실패한 항목에 bounded backoff를 기록한다.

Batch 제한은 destination과 retry 조건을 적용한 뒤 계산한다. 이전 destination 항목이나 retry 대기 항목이 뒤의 전송 가능한 항목을 막아서는 안 된다.

### 유지할 계약

- Hook이 종료되기 전에 outbox 기록을 완료한다.
- 원격 collector 장애가 capture 실패나 데이터 손실로 이어지지 않는다.
- `Stop` hook의 `{}` 출력 계약을 유지한다.
- local collector 경로는 변경하지 않는다.
- 목적지가 바뀌어도 기존 outbox 항목의 destination binding을 유지한다.
- 재시도 지연은 현재 bounded backoff 계약을 유지한다.

### 구현 대상

- `crates/akra-app/src/main.rs`
  - capture branch의 850ms inline `relay_once()` 제거
- `crates/akra-app/src/collector.rs`
  - destination과 retry 조건을 batch 제한보다 먼저 적용
  - 한 번의 pass에서 전송 가능한 항목 최대 8개 처리
- 관련 unit 및 integration tests
  - 원격 collector 중단 중 hook이 네트워크를 기다리지 않는지 검증
  - 앞에 retry 대기 항목이 8개 이상 있어도 뒤의 due 항목을 전송하는지 검증
  - 이전 destination 항목이 현재 destination 항목을 막지 않는지 검증
  - 실패 항목의 backoff와 durable 보존 검증

### 완료 조건

- Remote capture hook의 성공 조건은 durable outbox enqueue 완료다.
- Hook 프로세스는 원격 HTTP 요청을 시작하지 않는다.
- Relay worker는 필터링한 후보에 batch 제한을 적용한다.
- 원격 collector 중단 중에도 새 capture가 즉시 outbox에 누적된다.
- Collector가 복구되면 due 항목을 batch 단위로 전달한다.
- 기존 local capture, remote receipt, dedupe, retry 테스트가 통과한다.

## 2. 구현 결과

Capture hook은 durable outbox enqueue까지만 수행하고 종료한다. 원격 HTTP 전달은 `serve`의 relay worker가 담당하며, relay batch는 retry 시각과 destination을 먼저 검사한 뒤 최대 8개로 제한한다.
