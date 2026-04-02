# Simple Research Example

백엔드 없이 amure-do를 순수한 구조적 사고 도구로 사용하는 최소 설정.
문헌 리뷰, 논증 분석, 의사결정 정리 등에 적합합니다.

## 구조

```
simple-research/
├── amure-do.toml    # backend type = "none", claim_gate만 활성화
├── scenarios.md     # 원격 근무 생산성 문헌 리뷰 튜토리얼
└── README.md
```

## Quickstart

백엔드 설치 없이 바로 시작:

```bash
cd examples/simple-research
amure-do serve
```

`http://localhost:8080` → Research 탭에서 Thesis를 입력하거나
Knowledge 탭에서 Claim을 직접 추가합니다.

---

## 이런 용도에 적합합니다

- **문헌 리뷰**: 논문별 주장을 Claim-Reason-Evidence로 구조화
- **논증 분석**: 어떤 주장의 지지/반박 근거를 체계적으로 정리
- **의사결정**: 선택지별 Pro/Con을 evidence와 함께 기록
- **아이디어 검증**: LLM에게 논리적 허점을 찾게 하는 "rubber duck" 용도

---

## 설정

`amure-do.toml` 핵심 설정:

```toml
[backend]
type = "none"          # 백엔드 프로세스 불필요

[gates]
enabled = ["claim_gate"]  # 가장 기본적인 LLM 판정 gate만 사용

[llm]
default_provider = "ollama"
default_model = "llama3.1"
```

### LLM 교체

로컬 Ollama 대신 다른 provider 사용:

```toml
[llm]
default_provider = "claude_cli"   # Anthropic Claude
# default_provider = "openai"     # OpenAI GPT
# default_provider = "gemini"     # Google Gemini
```

---

## Knowledge System 간단 정리

```
[Claim] 검증할 명제
  ├── [Support Reason]  지지 근거
  │     └── [Evidence]  논문, 데이터, 인용문
  └── [Rebut Reason]    반증 근거
        └── [Evidence]
```

충분한 Evidence가 붙으면 **[Request Verdict]** → `claim_gate`(LLM)가 판정:
- **ACCEPT** — 증거가 충분히 지지함
- **CONDITIONAL ACCEPT** — 특정 조건 하에 유효
- **REJECT** — 반증이 더 강함
- **PENDING** — 증거 부족, 추가 탐색 필요

---

## 튜토리얼

`scenarios.md`에서 "원격 근무 생산성" 문헌 리뷰 예시를 따라가며
전체 워크플로우를 익힐 수 있습니다.
