//! 文字處理：腳本解析與長文分段。

use serde::{Deserialize, Serialize};

/// 腳本裡的一行台詞。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptLine {
    /// 第幾段（從 1 開始），對應時間軸上的位置。
    pub index: usize,
    /// `@聲音名：` 指定的聲音；沒寫就用專案預設聲音。
    pub voice: Option<String>,
    /// `{語氣指示}` 裡的內容，例如 `{溫柔、放慢}`。
    pub instruct: Option<String>,
    pub text: String,
}

/// 解析腳本。規則刻意簡單，讓人和 AI agent 都寫得出來：
///
/// - 一個段落（空行分隔）是一行台詞。
/// - 開頭寫 `@聲音名：` 或 `@聲音名:` 指定聲音。
/// - 接著可以寫 `{語氣指示}`，只有支援的模型會理會。
/// - `#` 開頭的行是註解，不會唸出來。
pub fn parse_script(script: &str) -> Vec<ScriptLine> {
    let mut lines = Vec::new();
    let mut buf = String::new();
    let flush = |buf: &mut String, lines: &mut Vec<ScriptLine>| {
        let paragraph = buf.trim();
        if !paragraph.is_empty() {
            let (voice, rest) = take_voice(paragraph);
            let (instruct, rest) = take_instruct(rest);
            let text = rest.trim().to_string();
            if !text.is_empty() {
                lines.push(ScriptLine { index: lines.len() + 1, voice, instruct, text });
            }
        }
        buf.clear();
    };
    for raw in script.lines() {
        let line = raw.trim();
        if line.starts_with('#') {
            continue;
        }
        if line.is_empty() {
            flush(&mut buf, &mut lines);
        } else {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(line);
        }
    }
    flush(&mut buf, &mut lines);
    lines
}

fn take_voice(s: &str) -> (Option<String>, &str) {
    let Some(rest) = s.strip_prefix('@').or_else(|| s.strip_prefix('＠')) else {
        return (None, s);
    };
    let end = rest.find(['：', ':', '\n']);
    match end {
        Some(i) if rest[i..].starts_with(['：', ':']) => {
            let name = rest[..i].trim();
            let sep_len = rest[i..].chars().next().map_or(1, char::len_utf8);
            if name.is_empty() { (None, s) } else { (Some(name.to_string()), &rest[i + sep_len..]) }
        }
        _ => (None, s),
    }
}

fn take_instruct(s: &str) -> (Option<String>, &str) {
    let t = s.trim_start();
    let (open, close) = if t.starts_with('{') {
        ('{', '}')
    } else if t.starts_with('｛') {
        ('｛', '｝')
    } else {
        return (None, s);
    };
    let inner = &t[open.len_utf8()..];
    match inner.find(close) {
        Some(i) => {
            let instr = inner[..i].trim();
            let rest = &inner[i + close.len_utf8()..];
            if instr.is_empty() { (None, rest) } else { (Some(instr.to_string()), rest) }
        }
        None => (None, s),
    }
}

const STRONG_BREAKS: &[char] = &['。', '！', '？', '!', '?', '；', ';', '\n', '…'];
const WEAK_BREAKS: &[char] = &['，', '、', ',', '：', ':', '）', ')', '」', '』'];

/// 把長文切成不超過 `max_chars` 個字元的段落，優先在句號、問號等強停頓切，
/// 其次在逗號等弱停頓切，真的沒有標點才硬切。不會產生空段。
pub fn split_for_synthesis(text: &str, max_chars: usize) -> Vec<String> {
    let max_chars = max_chars.max(8);
    let sentences = split_keep(text, STRONG_BREAKS);
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    for sentence in sentences {
        let pieces = if sentence.chars().count() > max_chars {
            split_long_sentence(&sentence, max_chars)
        } else {
            vec![sentence]
        };
        for piece in pieces {
            if !buf.is_empty() && buf.chars().count() + piece.chars().count() > max_chars {
                push_trimmed(&mut out, &buf);
                buf.clear();
            }
            buf.push_str(&piece);
        }
    }
    push_trimmed(&mut out, &buf);
    out
}

fn split_long_sentence(sentence: &str, max_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    for clause in split_keep(sentence, WEAK_BREAKS) {
        if clause.chars().count() > max_chars {
            push_trimmed(&mut out, &buf);
            buf.clear();
            let chars: Vec<char> = clause.chars().collect();
            for chunk in chars.chunks(max_chars) {
                push_trimmed(&mut out, &chunk.iter().collect::<String>());
            }
            continue;
        }
        if !buf.is_empty() && buf.chars().count() + clause.chars().count() > max_chars {
            push_trimmed(&mut out, &buf);
            buf.clear();
        }
        buf.push_str(&clause);
    }
    push_trimmed(&mut out, &buf);
    out
}

fn split_keep(text: &str, breaks: &[char]) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        buf.push(c);
        if breaks.contains(&c) {
            // 連續的標點（例如「？！」「……」與後面的引號）黏在同一段。
            while let Some(&next) = chars.peek() {
                if breaks.contains(&next) || matches!(next, '」' | '』' | '"' | '”' | '）' | ')') {
                    buf.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            out.push(std::mem::take(&mut buf));
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

fn push_trimmed(out: &mut Vec<String>, s: &str) {
    let t = s.trim();
    if !t.is_empty() && t.chars().any(char::is_alphanumeric) {
        out.push(t.to_string());
    }
}

/// 粗估朗讀秒數，用在 UI 的預估與 MCP 的提示：中文約每秒 4 字，英文約每秒 2.5 詞。
pub fn estimate_seconds(text: &str) -> f64 {
    let cjk = text.chars().filter(|c| is_cjk(*c)).count() as f64;
    let words = text
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .count() as f64;
    cjk / 4.0 + words / 2.5
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_voice_instruct_and_comments() {
        let script = "# 第一幕\n@旁白：{低沉、緩慢}夜深了。\n港口只剩下燈。\n\n@小美: 你還在嗎？\n\n沒有指定聲音的一段。";
        let lines = parse_script(script);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].voice.as_deref(), Some("旁白"));
        assert_eq!(lines[0].instruct.as_deref(), Some("低沉、緩慢"));
        assert_eq!(lines[0].text, "夜深了。\n港口只剩下燈。");
        assert_eq!(lines[1].voice.as_deref(), Some("小美"));
        assert_eq!(lines[1].text, "你還在嗎？");
        assert_eq!(lines[2].voice, None);
        assert_eq!(lines[2].index, 3);
    }

    #[test]
    fn at_sign_without_colon_is_text() {
        let lines = parse_script("@everyone 大家好");
        assert_eq!(lines[0].voice, None);
        assert_eq!(lines[0].text, "@everyone 大家好");
    }

    #[test]
    fn splits_on_strong_breaks_first() {
        let text = "今天天氣很好。我們去高雄的駁二走走吧！你覺得呢？";
        let parts = split_for_synthesis(text, 12);
        assert_eq!(parts, vec!["今天天氣很好。", "我們去高雄的駁二走走吧！", "你覺得呢？"]);
        assert_eq!(parts.concat(), text);
    }

    #[test]
    fn merges_short_sentences_up_to_limit() {
        let parts = split_for_synthesis("好。是。對。嗯。", 100);
        assert_eq!(parts, vec!["好。是。對。嗯。"]);
    }

    #[test]
    fn long_sentence_falls_back_to_commas_then_hard_split() {
        let text = "第一個子句很長很長，第二個子句也很長很長，第三個子句還是很長很長。";
        let parts = split_for_synthesis(text, 12);
        assert!(parts.iter().all(|p| p.chars().count() <= 12), "{parts:?}");
        assert_eq!(parts.concat(), text);

        let no_punct = "一".repeat(30);
        let parts = split_for_synthesis(&no_punct, 10);
        assert_eq!(parts.len(), 3);
    }

    #[test]
    fn keeps_trailing_quotes_with_sentence() {
        let parts = split_for_synthesis("她說：「走吧！」然後離開了。", 8);
        assert!(parts.iter().any(|p| p.ends_with("走吧！」")), "{parts:?}");
    }

    #[test]
    fn drops_punctuation_only_segments() {
        assert!(split_for_synthesis("。。。", 10).is_empty());
        assert!(split_for_synthesis("   ", 10).is_empty());
    }

    #[test]
    fn estimates_reading_time() {
        assert!((estimate_seconds("一二三四") - 1.0).abs() < 1e-9);
        assert!((estimate_seconds("hello world again more words") - 2.0).abs() < 1e-9);
    }
}
