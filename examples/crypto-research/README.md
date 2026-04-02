# Crypto Factor Research Example

amure-do를 이용한 가설 기반 암호화폐 alpha factor 발굴 예시.
Python HTTP 백엔드로 실제 분석 코드를 실행할 수 있습니다.

## 구조

```
crypto-research/
├── amure-do.toml          # 프로젝트 설정 (HTTP backend → localhost:5000)
├── scenarios.md           # 전체 워크플로우 튜토리얼
└── backend/
    ├── server.py          # Flask HTTP 백엔드 (port 5000)
    ├── sample_data.py     # 합성 OHLCV + 펀딩비 데이터 생성기
    └── requirements.txt   # Python 의존성
```

## Quickstart

### 1. Python 백엔드 시작

```bash
cd examples/crypto-research/backend
pip install -r requirements.txt
python server.py
```

정상 실행 시:
```
=== Crypto Research Backend ===
amure-do HTTP backend on http://localhost:5000
Available helpers: load_ohlcv, compute_factor, cross_sectional_rank, compute_ic, factor_summary
```

백엔드 상태 확인:
```bash
curl http://localhost:5000/health
# {"status": "ok", "backend": "crypto-research-python"}
```

### 2. amure-do 엔진 시작

```bash
cd examples/crypto-research
amure-do serve
```

`http://localhost:8080` 접속 → Research 탭에서 시작.

---

## Knowledge System

```
Thesis (자유 서술)
  └─ Claim (검증할 명제)
       ├─ Support Reason (지지 근거)
       │    └─ Evidence (실험 결과, 논문 등)
       └─ Rebut Reason (반증 근거)
            └─ Evidence
```

각 단계에서 LLM gate가 논리적 엄밀성을 검증합니다:
`claim_gate → thesis_gate → experiment_gate → argument_gate → validity_gate → dsr_gate → judge_gate`

---

## 백엔드 헬퍼 함수

실험 코드에서 아래 함수를 직접 호출할 수 있습니다:

| 함수 | 설명 |
|------|------|
| `load_ohlcv(symbol, timeframe)` | BTC/ETH/SOL 등 합성 OHLCV 데이터 로드 |
| `compute_factor(df, name)` | momentum / volatility / mean_reversion / funding / volume_surge |
| `cross_sectional_rank(factors)` | 여러 심볼 팩터를 percentile rank로 정규화 |
| `compute_ic(factor, returns)` | Spearman/Pearson IC 계산 |
| `factor_summary(factor, returns)` | mean IC, IC std, IR, hit ratio, t-stat |
| `load_universe(timeframe)` | 전체 유니버스 (BTC,ETH,SOL,BNB,ARB,OP,AVAX,DOGE) |

### 빠른 테스트

```python
# 백엔드 직접 테스트
curl -X POST http://localhost:5000/exec \
  -H "Content-Type: application/json" \
  -d '{"code": "df = load_ohlcv(\"BTC\")\nprint(df.tail(3))\nf = compute_factor(df, \"funding\")\nprint(factor_summary(f, df[\"close\"].pct_change()))"}'
```

---

## LLM 설정

`amure-do.toml`의 `[llm]` 섹션에서 provider를 교체합니다:

```toml
[llm]
default_provider = "claude_cli"   # claude_cli, openai, ollama, gemini 등 11개 지원
max_tokens = 4096
```

---

## 튜토리얼

전체 워크플로우는 `scenarios.md`를 참고하세요.
"펀딩비 반전" 가설을 처음부터 끝까지 따라가는 step-by-step 가이드입니다.
