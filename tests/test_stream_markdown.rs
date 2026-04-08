use stream_rust::stream_markdown::{
    escape_markdownv2, md_to_telegram_markdownv2, StreamingConverter,
};

// ─── escape_markdownv2 ───────────────────────────────────────────────────────

#[test]
fn escapes_all_reserved_characters() {
    let input = r#"_*[]()~`>#+-=|{}.!\"#;
    let expected = r#"\_\*\[\]\(\)\~\`\>\#\+\-\=\|\{\}\.\!\\"#;
    assert_eq!(escape_markdownv2(input), expected);
}

#[test]
fn leaves_normal_text_unchanged() {
    assert_eq!(escape_markdownv2("Hello world"), "Hello world");
}

#[test]
fn preserves_unicode_characters() {
    assert_eq!(escape_markdownv2("Hola 🎉 ñ ü"), "Hola 🎉 ñ ü");
}

#[test]
fn escapes_empty_string() {
    assert_eq!(escape_markdownv2(""), "");
}

#[test]
fn escapes_dots_and_exclamations() {
    assert_eq!(escape_markdownv2("Hello. World!"), r"Hello\. World\!");
}

// ─── md_to_telegram_markdownv2: bold (**text**) ──────────────────────────────

#[test]
fn converts_double_asterisk_bold() {
    assert_eq!(md_to_telegram_markdownv2("**bold**"), "*bold*");
}

#[test]
fn escapes_reserved_chars_inside_bold() {
    assert_eq!(md_to_telegram_markdownv2("**hello.world!**"), r"*hello\.world\!*");
}

#[test]
fn bold_in_sentence() {
    assert_eq!(
        md_to_telegram_markdownv2("This is **important** text."),
        r"This is *important* text\."
    );
}

// ─── md_to_telegram_markdownv2: italic (*text* y _text_) ─────────────────────

#[test]
fn converts_single_asterisk_italic() {
    // Markdown estándar: *text* = italic → Telegram: _text_
    assert_eq!(md_to_telegram_markdownv2("*italic*"), "_italic_");
}

#[test]
fn converts_underscore_italic() {
    assert_eq!(md_to_telegram_markdownv2("_italic_"), "_italic_");
}

#[test]
fn multiline_single_asterisk_is_not_italic() {
    // *..* que cruza líneas no se trata como italic, se escapa
    assert_eq!(
        md_to_telegram_markdownv2("*line one\nline two*"),
        r"\*line one" .to_string() + "\n" + r"line two\*"
    );
}

// ─── md_to_telegram_markdownv2: headings ─────────────────────────────────────

#[test]
fn converts_h1_heading() {
    assert_eq!(md_to_telegram_markdownv2("# Title"), "*Title*");
}

#[test]
fn converts_h2_heading() {
    assert_eq!(md_to_telegram_markdownv2("## Section"), "*Section*");
}

#[test]
fn converts_h3_heading() {
    assert_eq!(md_to_telegram_markdownv2("### Subsection"), "*Subsection*");
}

#[test]
fn heading_with_newline() {
    assert_eq!(
        md_to_telegram_markdownv2("## Title\nBody text."),
        "*Title*\nBody text\\."
    );
}

#[test]
fn heading_escapes_reserved_chars() {
    assert_eq!(
        md_to_telegram_markdownv2("## Datos destacados:"),
        r"*Datos destacados:*"
    );
}

#[test]
fn heading_mid_text_after_newline() {
    assert_eq!(
        md_to_telegram_markdownv2("Intro\n## Section\nBody"),
        "Intro\n*Section*\nBody"
    );
}

#[test]
fn hash_not_at_line_start_is_escaped() {
    assert_eq!(md_to_telegram_markdownv2("Use # for comments"), r"Use \# for comments");
}

// ─── md_to_telegram_markdownv2: code ─────────────────────────────────────────

#[test]
fn converts_inline_code() {
    assert_eq!(md_to_telegram_markdownv2("`code`"), "`code`");
}

#[test]
fn converts_code_block() {
    let input = "```\nfn main() {}\n```";
    let expected = "```\nfn main() {}\n```";
    assert_eq!(md_to_telegram_markdownv2(input), expected);
}

#[test]
fn code_block_with_language() {
    let input = "```rust\nlet x = 1;\n```";
    let expected = "```rust\nlet x = 1;\n```";
    assert_eq!(md_to_telegram_markdownv2(input), expected);
}

// ─── md_to_telegram_markdownv2: links ────────────────────────────────────────

#[test]
fn converts_link() {
    let input = "[click](http://example.com)";
    let expected = "[click](http://example.com)";
    assert_eq!(md_to_telegram_markdownv2(input), expected);
}

#[test]
fn url_with_nested_parens_greedy_match() {
    let input = "[text](http://example.com/path_(1))";
    let expected = r"[text](http://example.com/path_(1)\)";
    assert_eq!(md_to_telegram_markdownv2(input), expected);
}

// ─── md_to_telegram_markdownv2: plain text escaping ──────────────────────────

#[test]
fn escapes_plain_text_reserved_chars() {
    assert_eq!(
        md_to_telegram_markdownv2("Price: $5.00!"),
        r"Price: $5\.00\!"
    );
}

#[test]
fn unclosed_double_bold_is_escaped() {
    assert_eq!(md_to_telegram_markdownv2("**no close"), r"\*\*no close");
}

#[test]
fn unclosed_single_asterisk_is_escaped() {
    assert_eq!(md_to_telegram_markdownv2("*no close"), r"\*no close");
}

#[test]
fn unclosed_backtick_is_escaped() {
    assert_eq!(md_to_telegram_markdownv2("`no close"), r"\`no close");
}

// ─── md_to_telegram_markdownv2: mixed / realistic ────────────────────────────

#[test]
fn converts_mixed_formatting() {
    let input = "Hello **bold** and _italic_ done.";
    let expected = r"Hello *bold* and _italic_ done\.";
    assert_eq!(md_to_telegram_markdownv2(input), expected);
}

#[test]
fn realistic_claude_response() {
    let input = "## California\n\n\
                  **California** es el estado más poblado.\n\n\
                  ### Datos destacados:\n\
                  - Ciudad más grande: Los Ángeles\n\
                  - Ubicado en la costa oeste";
    let result = md_to_telegram_markdownv2(input);
    // ## → bold
    assert!(result.starts_with("*California*\n"));
    // **California** → *California*
    assert!(result.contains("*California*"));
    // ### → bold
    assert!(result.contains("*Datos destacados:*"));
    // - se escapa
    assert!(result.contains(r"\- Ciudad más grande: Los Ángeles"));
    // . se escapa
    assert!(result.contains(r"Ángeles"));
}

// ─── StreamingConverter ──────────────────────────────────────────────────────

#[test]
fn streaming_complete_tokens() {
    let mut conv = StreamingConverter::new();
    let out = conv.push("Hello **bold** world.");
    let flush = conv.flush();
    let full = format!("{}{}", out, flush);
    assert_eq!(full, md_to_telegram_markdownv2("Hello **bold** world."));
}

#[test]
fn streaming_split_bold_token() {
    let mut conv = StreamingConverter::new();
    let out1 = conv.push("Hello **bol");
    let out2 = conv.push("d** world.");
    let flush = conv.flush();
    let full = format!("{}{}{}", out1, out2, flush);
    assert_eq!(full, md_to_telegram_markdownv2("Hello **bold** world."));
}

#[test]
fn streaming_split_italic_token() {
    let mut conv = StreamingConverter::new();
    let out1 = conv.push("Hello _ital");
    let out2 = conv.push("ic_ end.");
    let flush = conv.flush();
    let full = format!("{}{}{}", out1, out2, flush);
    assert_eq!(full, md_to_telegram_markdownv2("Hello _italic_ end."));
}

#[test]
fn streaming_split_code_block() {
    let mut conv = StreamingConverter::new();
    let out1 = conv.push("```\ncode her");
    let out2 = conv.push("e\n```");
    let flush = conv.flush();
    let full = format!("{}{}{}", out1, out2, flush);
    assert_eq!(full, md_to_telegram_markdownv2("```\ncode here\n```"));
}

#[test]
fn streaming_split_inline_code() {
    let mut conv = StreamingConverter::new();
    let out1 = conv.push("text `co");
    let out2 = conv.push("de` end.");
    let flush = conv.flush();
    let full = format!("{}{}{}", out1, out2, flush);
    assert_eq!(full, md_to_telegram_markdownv2("text `code` end."));
}

#[test]
fn streaming_flush_incomplete_token() {
    let mut conv = StreamingConverter::new();
    let out1 = conv.push("Hello **unclosed");
    let flush = conv.flush();
    let full = format!("{}{}", out1, flush);
    assert_eq!(full, md_to_telegram_markdownv2("Hello **unclosed"));
}

#[test]
fn streaming_empty_chunks() {
    let mut conv = StreamingConverter::new();
    assert_eq!(conv.push(""), "");
    assert_eq!(conv.push(""), "");
    let out = conv.push("Hello.");
    let flush = conv.flush();
    let full = format!("{}{}", out, flush);
    assert_eq!(full, md_to_telegram_markdownv2("Hello."));
}

#[test]
fn streaming_many_chunks_accumulation() {
    let chunks = vec![
        "Hello ",
        "**bol",
        "d text** ",
        "and _ital",
        "ic_ world.\n",
        "`cod",
        "e here`\n",
        "End.",
    ];
    let full_input: String = chunks.iter().copied().collect();

    let mut conv = StreamingConverter::new();
    let mut accumulated = String::new();
    for chunk in &chunks {
        accumulated.push_str(&conv.push(chunk));
    }
    accumulated.push_str(&conv.flush());

    assert_eq!(accumulated, md_to_telegram_markdownv2(&full_input));
}

#[test]
fn streaming_link_split() {
    let mut conv = StreamingConverter::new();
    let out1 = conv.push("See [cli");
    let out2 = conv.push("ck](http://example.com) here.");
    let flush = conv.flush();
    let full = format!("{}{}{}", out1, out2, flush);
    assert_eq!(
        full,
        md_to_telegram_markdownv2("See [click](http://example.com) here.")
    );
}

#[test]
fn streaming_heading_split() {
    let mut conv = StreamingConverter::new();
    let out1 = conv.push("Intro\n## Cali");
    let out2 = conv.push("fornia\nBody.");
    let flush = conv.flush();
    let full = format!("{}{}{}", out1, out2, flush);
    assert_eq!(
        full,
        md_to_telegram_markdownv2("Intro\n## California\nBody.")
    );
}
