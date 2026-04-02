/// Message Hook — intent parsing + routing.
/// Intercepts messages to route to appropriate handler (Local Proc or LLM).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedIntent {
    pub action: String,         // "backtest", "evaluate", "search", "analyze", "chat"
    pub symbol: Option<String>,
    pub direction: Option<String>,
    pub params: std::collections::HashMap<String, String>,
    pub requires_llm: bool,
    pub requires_julia: bool,
}

/// Parse a user message into a structured intent.
/// Deterministic — no LLM needed.
pub fn parse_intent(message: &str) -> ParsedIntent {
    let lower = message.to_lowercase();
    let mut intent = ParsedIntent {
        action: "chat".into(),
        symbol: None,
        direction: None,
        params: std::collections::HashMap::new(),
        requires_llm: true,
        requires_julia: false,
    };

    // Detect action (order matters — more specific first)
    if contains_any(&lower, &["평가", "evaluate", "검증", "검수"]) {
        intent.action = "evaluate".into();
        intent.requires_julia = true;
    } else if contains_any(&lower, &["백테스트", "backtest", "테스트", "실험", "돌려"]) {
        intent.action = "backtest".into();
        intent.requires_julia = true;
    } else if contains_any(&lower, &["검색", "search", "찾아", "관련"]) {
        intent.action = "search".into();
        intent.requires_llm = false;
    } else if contains_any(&lower, &["분석", "analyze", "모멘텀"]) && !contains_any(&lower, &["평가", "검증"]) {
        intent.action = "analyze".into();
        intent.requires_julia = true;
    } else if contains_any(&lower, &["채택", "기각", "accept", "reject", "judge"]) {
        intent.action = "judge".into();
        intent.requires_llm = true;
    }

    // Detect symbol
    let symbols = extract_symbols(&lower);
    if let Some(sym) = symbols.first() {
        intent.symbol = Some(sym.clone());
    }

    // Detect direction
    if contains_any(&lower, &["short", "숏", "하락"]) {
        intent.direction = Some("short".into());
    } else if contains_any(&lower, &["long", "롱", "상승"]) {
        intent.direction = Some("long".into());
    }

    // Detect parameters
    if let Some(val) = extract_number_after(&lower, "px") {
        intent.params.insert("px_threshold".into(), val);
    }
    if let Some(val) = extract_number_after(&lower, "oi") {
        intent.params.insert("oi_threshold".into(), val);
    }
    if contains_any(&lower, &["oi_flip", "oi flip"]) {
        intent.params.insert("exit_mode".into(), "oi_flip".into());
    }
    if contains_any(&lower, &["sign_change", "sign change"]) {
        intent.params.insert("exit_mode".into(), "sign_change".into());
    }

    intent
}

/// Pre-process hook: enrich message with context before sending to LLM.
pub fn build_hook_context(intent: &ParsedIntent) -> String {
    let mut ctx = String::new();

    match intent.action.as_str() {
        "backtest" => {
            ctx.push_str("[Hook: backtest detected]\n");
            ctx.push_str("Julia 서버에서 실행 가능. /api/julia/exec 사용.\n");
            if let Some(sym) = &intent.symbol {
                ctx.push_str(&format!("Target symbol: {}\n", sym));
            }
            if let Some(dir) = &intent.direction {
                ctx.push_str(&format!("Direction: {}\n", dir));
            }
            ctx.push_str("코드 작성 시 /api/julia/review로 look-ahead 체크 필수.\n");
        }
        "evaluate" => {
            ctx.push_str("[Hook: evaluation detected]\n");
            ctx.push_str("POST /api/evaluate 또는 /api/manager/run 사용.\n");
            ctx.push_str("5단계 deterministic 평가: time/universe/regime/knowledge/composite.\n");
        }
        "search" => {
            ctx.push_str("[Hook: knowledge search detected]\n");
            ctx.push_str("POST /api/knowledge/insights/search 사용.\n");
        }
        "judge" => {
            ctx.push_str("[Hook: judgment detected]\n");
            ctx.push_str("POST /api/judge/hypotheses/{id} 사용.\n");
            ctx.push_str("경제적 메커니즘 타당성 + 일반화 가능성 기준.\n");
        }
        _ => {}
    }

    ctx
}

fn contains_any(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|p| text.contains(p))
}

fn extract_symbols(text: &str) -> Vec<String> {
    let known = ["btcusdt", "ethusdt", "aapl", "xrpusdt", "solusdt", "dogeusdt",
        "ondousdt", "hypeusdt", "btc", "eth", "aapl"];
    let mut found = Vec::new();
    for sym in &known {
        if text.contains(sym) {
            let upper = sym.to_uppercase();
            if !upper.ends_with("USDT") && !found.contains(&format!("{}USDT", upper)) {
                found.push(format!("{}USDT", upper));
            } else {
                found.push(upper);
            }
        }
    }
    found.dedup();
    found
}

fn extract_number_after(text: &str, prefix: &str) -> Option<String> {
    if let Some(pos) = text.find(prefix) {
        let rest = &text[pos + prefix.len()..];
        let num: String = rest.chars()
            .skip_while(|c| !c.is_ascii_digit() && *c != '.')
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if !num.is_empty() { Some(num) } else { None }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backtest_intent() {
        let intent = parse_intent("BTCUSDT short 백테스트 해줘");
        assert_eq!(intent.action, "backtest");
        assert_eq!(intent.symbol, Some("BTCUSDT".into()));
        assert_eq!(intent.direction, Some("short".into()));
        assert!(intent.requires_julia);
    }

    #[test]
    fn test_search_intent() {
        let intent = parse_intent("momentum 관련 지식 검색해줘");
        assert_eq!(intent.action, "search");
        assert!(!intent.requires_llm);
    }

    #[test]
    fn test_evaluate_intent() {
        let intent = parse_intent("이 실험 결과 평가해줘");
        assert_eq!(intent.action, "evaluate");
    }

    #[test]
    fn test_chat_fallback() {
        let intent = parse_intent("안녕하세요");
        assert_eq!(intent.action, "chat");
    }

    #[test]
    fn test_params_extraction() {
        let intent = parse_intent("px 0.1 oi 0.25 oi_flip으로 backtest");
        assert_eq!(intent.params.get("px_threshold"), Some(&"0.1".into()));
        assert_eq!(intent.params.get("oi_threshold"), Some(&"0.25".into()));
        assert_eq!(intent.params.get("exit_mode"), Some(&"oi_flip".into()));
    }
}
