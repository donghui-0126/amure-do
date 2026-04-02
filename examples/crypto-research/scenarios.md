# Crypto Factor Research — Walkthrough

amure-do를 이용한 가설 기반 암호화폐 팩터 연구 가이드.
"펀딩비 반전" 가설을 예시로 전체 워크플로우를 따라갑니다.

---

## Scenario: "펀딩비 반전(Funding Rate Reversal)" 가설 검증

### 배경

선물 시장의 펀딩비(funding rate)는 롱/숏 포지션 간 수요 불균형을 나타냅니다.
극단적으로 높은 펀딩비 → 과도한 레버리지 롱 포지션 → 청산 캐스케이드로 인한 가격 반전.
이 가설이 실제로 alpha를 제공하는지 체계적으로 검증합니다.

---

### Step 1: 시스템 시작

```bash
# 터미널 1: Python 백엔드 시작
cd examples/crypto-research/backend
pip install -r requirements.txt
python server.py
# → http://localhost:5000 에서 대기

# 터미널 2: amure-do 엔진 시작
cd examples/crypto-research
amure-do serve
# → http://localhost:8080 에서 대기
```

백엔드가 정상 작동하는지 확인:
```bash
curl http://localhost:5000/health
# {"status": "ok", "backend": "crypto-research-python"}
```

---

### Step 2: Research 탭 — Thesis 작성

브라우저에서 `http://localhost:8080` 접속 → **Research** 탭 선택.

**Thesis 입력창에 아이디어를 자유롭게 서술:**

> "높은 펀딩비는 과도한 레버리지를 의미하며, 이후 가격 반전이 발생한다.
> 특히 펀딩비가 극단적으로 높거나 낮을 때(상위/하위 10%) 이후 8시간 수익률이
> 역방향을 보이는 패턴이 존재한다."

**[Analyze Thesis]** 버튼을 누르면 `thesis_gate`가 LLM을 통해 아래를 자동 생성합니다:

- **Claim**: "극단적 펀딩비 이후 8시간 내 cross-sectional 평균 회귀가 발생한다"
- **Support Reason**: "레버리지 청산 메커니즘이 역방향 가격 압력을 유도"
- **Rebut Reason**: "강한 추세장에서는 높은 펀딩비가 지속되며 추세 방향으로 가격 진행"

---

### Step 3: Knowledge 탭 — Claim 구조 확인

**Knowledge** 탭으로 이동하면 방금 생성된 클레임 트리가 보입니다:

```
[Claim] 극단적 펀딩비 이후 8시간 내 평균 회귀 발생
  ├── [Support] 청산 메커니즘이 가격 반전 유도
  │     └── [Evidence] (실험 결과가 여기에 추가됨)
  └── [Rebut]  강한 추세장에서는 펀딩비 지속
        └── [Evidence] (반증 데이터가 여기에 추가됨)
```

각 노드를 클릭하면 상세 내용 편집, Evidence 추가, Verdict 요청이 가능합니다.

---

### Step 4: Lab 탭 — 실험 설계

**Lab** 탭에서 Support Reason에 대한 실험을 설계합니다.
실험 코드는 백엔드 Python namespace에서 실행됩니다.

#### 실험 1: Cross-sectional IC 분석

```python
# 전체 유니버스에서 funding 팩터의 IC를 계산합니다
universe = load_universe("1h", n_bars=500)

factor_values = {}
return_values = {}

for sym, df in universe.items():
    factor_values[sym] = compute_factor(df, "funding", window=24)
    # 8시간 forward return
    return_values[sym] = df["close"].pct_change(8).shift(-8)

# Cross-sectional rank IC
ranked = cross_sectional_rank(factor_values)

ic_by_time = []
for ts in ranked.index[100:]:
    f = ranked.loc[ts]
    r = pd.Series({sym: return_values[sym].get(ts, np.nan) for sym in universe})
    ic = compute_ic(f, r, method="spearman")
    ic_by_time.append(ic)

ic_arr = [x for x in ic_by_time if not np.isnan(x)]
print(f"Mean IC:     {np.mean(ic_arr):.4f}")
print(f"IC Std:      {np.std(ic_arr):.4f}")
print(f"IR:          {np.mean(ic_arr)/np.std(ic_arr):.4f}")
print(f"Hit Ratio:   {np.mean(np.array(ic_arr) > 0):.2%}")
```

#### 실험 2: 조건부 레짐 분석 (bull/bear/sideways)

```python
# BTC 추세를 기준으로 레짐을 구분합니다
btc = load_ohlcv("BTC", "1h", n_bars=500)
btc_ret = btc["close"].pct_change(48)  # 48시간 수익률로 레짐 정의

regime = pd.cut(btc_ret, bins=[-np.inf, -0.03, 0.03, np.inf],
                labels=["bear", "sideways", "bull"])

funding_factor = compute_factor(btc, "funding", window=24)
fwd_returns = btc["close"].pct_change(8).shift(-8)

for r in ["bull", "bear", "sideways"]:
    mask = regime == r
    ic = compute_ic(funding_factor[mask], fwd_returns[mask])
    print(f"[{r:>8}] IC = {ic:.4f}  (n={mask.sum()})")
```

#### 실험 3: Multi-horizon IC Decay

```python
# 신호가 얼마나 오래 유효한지 확인합니다
btc = load_ohlcv("BTC", "1h", n_bars=500)
factor = compute_factor(btc, "funding", window=24)
returns = btc["close"].pct_change()

summary = factor_summary(factor, returns, horizons=[1, 2, 4, 8, 16, 24, 48])
print("=== Factor Summary ===")
for k, v in summary.items():
    print(f"  {k:<12} {v}")
```

---

### Step 5: 실험 실행

각 실험 코드 블록 옆의 **[Run]** 버튼을 클릭합니다.
amure-do가 `POST /exec`로 코드를 백엔드에 전송하고 결과를 표시합니다.

실험 결과 예시:
```
Mean IC:     0.0412
IC Std:      0.1834
IR:          0.2247
Hit Ratio:   54.30%

[    bull] IC =  0.0183  (n=142)
[    bear] IC =  0.0731  (n=98)   ← bear 레짐에서 가장 강함
[sideways] IC =  0.0289  (n=162)

ic_h1   : 0.0108
ic_h4   : 0.0312
ic_h8   : 0.0412  ← peak
ic_h16  : 0.0289
ic_h24  : 0.0175
ic_h48  : 0.0021  ← decay 확인
```

---

### Step 6: Evidence 추가 & Verdict 요청

실험 결과가 나왔으면:

1. **Knowledge** 탭으로 돌아가 해당 Reason 노드 클릭
2. **[Add Evidence]** → 실험 ID, 결과 요약, 해석을 입력
3. **[Request Verdict]** → `argument_gate`가 LLM으로 판단:

> "IC = 0.041, IR = 0.225로 통계적으로 유의미하나 경제적 유의성은 제한적.
> Bear 레짐에서 효과가 집중되므로 조건부 유효성을 인정. Validity: CONDITIONAL"

---

### Step 7: Rebut Reason 검증

같은 방법으로 Rebut Reason("추세장에서는 펀딩비가 지속됨")도 실험합니다:

```python
# bull 레짐에서 펀딩비가 mean-reversion보다 momentum을 따르는지 확인
btc = load_ohlcv("BTC", "1h")
funding = compute_factor(btc, "funding", window=24)
momentum = compute_factor(btc, "momentum", window=24)
fwd = btc["close"].pct_change(8).shift(-8)

btc_trend = btc["close"].pct_change(48)
bull_mask = btc_trend > 0.03

print("=== Bull 레짐 내 팩터 비교 ===")
print(f"Funding IC  (bull): {compute_ic(funding[bull_mask], fwd[bull_mask]):.4f}")
print(f"Momentum IC (bull): {compute_ic(momentum[bull_mask], fwd[bull_mask]):.4f}")
```

반증이 확인되면 Rebut Reason에 Evidence를 추가하고 Verdict를 받습니다.

---

### Step 8: Claim 최종 Verdict

모든 Reason에 Evidence와 Verdict가 달리면 `judge_gate`가 최종 판단:

> "Support(IC=0.041, bear 특화)와 Rebut(bull에서 무효) 증거를 종합.
> 클레임을 CONDITIONAL ACCEPT으로 판정:
> **'Bear/Sideways 레짐에서 극단적 펀딩비는 8시간 반전 시그널로 유효'**
> Validity Conditions: [bear_regime, |funding_rate| > 0.001]"

---

### Step 9: Knowledge 축적

승인된 Claim은 **지식 베이스**로 승격됩니다.
Settings 탭에서 knowledge export를 통해 다음 연구의 Prior로 활용할 수 있습니다.

```
Knowledge Base:
  ✓ Funding reversal (CONDITIONAL) → bear_regime + |FR| > 0.001
  ✗ Momentum (REJECTED) → 8h horizon에서 IC < 0.01
  ✓ Low-vol anomaly (ACCEPTED) → all regimes, IC = 0.067
```

---

## 다음 단계

- `compute_factor(df, "momentum")` 등 다른 팩터로 동일 과정 반복
- 여러 팩터를 조합해 복합 시그널 실험 (`cross_sectional_rank`)
- `amure-do.toml`의 `gates` 목록에서 불필요한 gate 제거해 속도 향상
- LLM provider를 `openai` / `gemini`로 교체해 verdict 품질 비교
