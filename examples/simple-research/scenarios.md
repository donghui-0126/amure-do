# Structured Research — Literature Review Walkthrough

백엔드 없이 amure-do를 순수한 구조적 사고 도구로 활용하는 예시.
"원격 근무 생산성" 문헌 리뷰를 예제로 사용합니다.

---

## Scenario: "원격 근무는 사무실 근무보다 생산성이 높은가?"

### 배경

코로나 이후 원격 근무 연구가 폭발적으로 늘었지만, 결과가 상충합니다.
amure-do의 Claim-Reason-Evidence 구조로 문헌을 정리하고
LLM의 도움을 받아 논리의 빈틈을 찾아봅니다.

---

### Step 1: amure-do 시작 (백엔드 불필요)

```bash
cd examples/simple-research
amure-do serve
```

`amure-do.toml`의 `[backend] type = "none"` 설정 덕분에
백엔드 프로세스 없이 바로 시작됩니다.

브라우저에서 `http://localhost:8080` 접속.

---

### Step 2: Research 탭 — 연구 질문 입력

Thesis 입력창에 연구 질문을 서술합니다:

> "Stanford, Microsoft, Owl Labs 등의 연구를 보면 원격 근무자가 더 높은
> 개인 생산성을 보고하지만, 동시에 팀 협업과 혁신 지표는 낮아지는 경향이 있다.
> 이 두 효과를 종합하면 원격 근무의 순효과는 무엇인가?"

**[Analyze]** 클릭 → `thesis_gate`(LLM)이 아래 Claim들을 제안합니다:

- **Claim A**: "원격 근무는 개인 집중 업무의 생산성을 높인다"
- **Claim B**: "원격 근무는 팀 협업과 암묵지 전달을 저해한다"
- **Claim C**: "원격 근무의 생산성 효과는 직무 유형에 따라 다르다"

원하는 Claim을 선택해 Knowledge 베이스에 추가합니다.

---

### Step 3: Knowledge 탭 — Reason & Evidence 추가

#### Claim A: "원격 근무 → 개인 생산성 향상"

**Support Reasons:**

1. **통근 시간 절약** → 하루 평균 72분 회수
   - Evidence: U.S. Census Bureau (2021), 평균 통근 36분
   - Evidence: Bloom et al. (2015), 생산성 13% 향상 (콜센터 실험)

2. **집중 환경 제어 가능** → 방해 요소 감소
   - Evidence: Buffer "State of Remote Work 2023" — 원격 근무자 68%가 집중력 향상 보고

**Rebut Reasons:**

1. **가정 내 방해 요소** → 자녀, 소음, 공간 부족
   - Evidence: Barrero et al. (2021) — 집 환경이 좋을수록 효과 차이

2. **자기통제 실패** → Procrastination 증가 가능
   - Evidence: 일부 연구에서 juniors의 생산성 하락 관찰

---

#### Claim B: "원격 근무 → 협업/혁신 저해"

**Support Reasons:**

1. **비공식 상호작용 감소** → 복도 대화, 우연한 아이디어 교환 소멸
   - Evidence: Yang et al. (2022) Microsoft 연구 — 약한 ties 급감

2. **화상 피로** → 집중 세션 후 인지 부하 증가
   - Evidence: Bailenson (2021) Stanford "Zoom Fatigue" 연구

**Rebut Reasons:**

1. **async 도구 발달** → Notion, Slack, Loom으로 협업 품질 유지 가능
   - Evidence: GitLab all-remote 리포트 (2023)

---

### Step 4: Lab 탭 — LLM 보조 분석

백엔드가 없으므로 코드 실행은 안 되지만,
**Lab** 탭의 LLM 프롬프트 기능을 활용할 수 있습니다.

#### 논리 비판 요청:

```
현재까지 수집한 Claim A의 Support/Rebut Reasons를 검토해줘.
다음을 분석해:
1. 어떤 반론이 가장 치명적인가?
2. 내가 놓친 중요한 변수(moderator)가 있는가?
3. 각 Evidence의 외적 타당도 위협은?
```

LLM 응답 예시:
> "가장 치명적인 반론은 '직무 유형'입니다. Bloom (2015)의 13% 향상은
> 콜센터 업무에 한정된 결과입니다. 지식 집약적 창의 업무에서는
> Yang et al. (2022)가 반대 결과를 보여줍니다.
> Claim A를 직무 유형으로 조건화하면 Claim C와 통합됩니다."

이 인사이트를 바탕으로 Claim을 수정하거나 새 Reason을 추가합니다.

---

### Step 5: 갭 분석 — 빠진 증거 찾기

Lab에서 추가 프롬프트:

```
내 논증 구조에서 증거가 약한 부분은 어디인가?
추가로 찾아봐야 할 연구 방향을 제안해줘.
```

응답:
> "아직 다루지 않은 중요 변수:
> - 경력 수준 (junior vs senior)
> - 산업군 (제조업 vs 소프트웨어)
> - 원격 비율 (fully remote vs hybrid)
> 이 조절변수들을 고려하면 Claim C('직무 유형 의존성')가
> 핵심 명제가 될 가능성이 높습니다."

발견된 갭을 Evidence 슬롯에 "Missing" 상태로 추가해 추후 보완합니다.

---

### Step 6: Verdict 요청

충분한 Evidence가 쌓이면 각 Claim의 **[Request Verdict]** 클릭.
`claim_gate`가 LLM으로 판정합니다:

**Claim A 판정:**
> "Bloom (2015)의 13% 향상은 고도로 통제된 환경.
> 현실 knowledge worker 대상 연구는 혼합 결과.
> **CONDITIONAL ACCEPT** — 집중 업무 비중 높은 직무에 한해 유효"

**Claim B 판정:**
> "Yang et al. (2022)의 large-scale 증거가 강력.
> **ACCEPT** — 협업 저해 효과는 연구 간 일관성 높음"

**Claim C 판정:**
> "Evidence 부족. Junior/senior, 산업별 비교 연구 필요.
> **PENDING** — 추가 문헌 탐색 후 재판정 권장"

---

### Step 7: Knowledge 베이스 완성

최종 상태:

```
Research Question: 원격 근무의 생산성 효과
─────────────────────────────────────────
✓ Claim A: 개인 생산성 향상 [CONDITIONAL]
    Condition: 집중 업무 비중 > 60%인 직무

✓ Claim B: 팀 협업/혁신 저해 [ACCEPTED]
    Strength: Strong (large-scale evidence)

? Claim C: 직무 유형 의존성 [PENDING]
    Missing: Junior/senior, industry breakdown
```

이 구조를 논문 Discussion 섹션이나 의사결정 보고서의 skeleton으로 활용합니다.

---

## 활용 팁

- **빠른 시작**: Thesis 없이 Claim을 직접 입력해도 됩니다
- **LLM 교체**: `amure-do.toml`에서 `default_provider = "claude_cli"` 등으로 변경
- **내보내기**: Settings 탭 → Export로 Markdown/JSON 형태로 저장
- **재사용**: 완성된 Knowledge 베이스를 다른 연구의 Background로 import
