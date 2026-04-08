/// Conversión de Markdown estándar (output de Claude/Bedrock) a Telegram MarkdownV2.
///
/// Markdown estándar (Claude)  →  Telegram MarkdownV2
/// ─────────────────────────────────────────────────────
/// `**bold**`                  →  `*bold*`
/// `*italic*`                  →  `_italic_`
/// `_italic_`                  →  `_italic_`
/// `## Heading`                →  `*Heading*` (bold)
/// `` `code` ``                →  `` `code` ``
/// ```` ```block``` ````       →  ```` ```block``` ````
/// `[text](url)`               →  `[text](url)`
/// Chars reservados            →  escapados con `\`

pub fn escape_markdownv2(text: &str) -> String {
    let mut result = String::with_capacity(text.len() * 2);
    for c in text.chars() {
        match c {
            '_' | '*' | '[' | ']' | '(' | ')' | '~' | '`' | '>' | '#'
            | '+' | '-' | '=' | '|' | '{' | '}' | '.' | '!' | '\\' => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
    }
    result
}

/// Inside code blocks: only backtick and backslash need escaping
fn escape_code(text: &str) -> String {
    text.replace('\\', "\\\\").replace('`', "\\`")
}

/// Inside URLs: only `)` and `\` need escaping
fn escape_url(url: &str) -> String {
    url.replace('\\', "\\\\").replace(')', "\\)")
}

/// Convierte Markdown estándar (output de Claude) a Telegram MarkdownV2.
pub fn md_to_telegram_markdownv2(input: &str) -> String {
    let mut result = String::new();
    let mut i = 0;

    while i < input.len() {
        let remaining = &input[i..];

        // ``` pre-formatted block (máxima prioridad)
        if remaining.starts_with("```") {
            let after = &remaining[3..];
            if let Some(end) = after.find("```") {
                result.push_str("```");
                result.push_str(&escape_code(&after[..end]));
                result.push_str("```");
                i += 3 + end + 3;
                continue;
            }
        }

        // `inline code`
        if remaining.starts_with('`') {
            let after = &remaining[1..];
            if let Some(end) = after.find('`') {
                result.push('`');
                result.push_str(&escape_code(&after[..end]));
                result.push('`');
                i += 1 + end + 1;
                continue;
            }
        }

        // ## Heading (inicio de línea → bold en Telegram)
        if remaining.starts_with('#') && (i == 0 || input.as_bytes()[i - 1] == b'\n') {
            let hashes = remaining.bytes().take_while(|&b| b == b'#').count();
            if hashes <= 6
                && remaining.len() > hashes
                && remaining.as_bytes()[hashes] == b' '
            {
                let text_start = hashes + 1;
                let line_end = remaining[text_start..]
                    .find('\n')
                    .map(|p| text_start + p)
                    .unwrap_or(remaining.len());
                let heading_text = remaining[text_start..line_end].trim();
                if !heading_text.is_empty() {
                    result.push('*');
                    result.push_str(&escape_markdownv2(heading_text));
                    result.push('*');
                }
                i += line_end;
                if i < input.len() && input.as_bytes()[i] == b'\n' {
                    result.push('\n');
                    i += 1;
                }
                continue;
            }
        }

        // **bold** → *bold* en Telegram (antes de * simple)
        if remaining.starts_with("**") {
            let after = &remaining[2..];
            if let Some(end) = after.find("**") {
                if end > 0 {
                    result.push('*');
                    result.push_str(&escape_markdownv2(&after[..end]));
                    result.push('*');
                    i += 2 + end + 2;
                    continue;
                }
            }
        }

        // *italic* → _italic_ en Telegram (solo single-line)
        if remaining.starts_with('*') && !remaining.starts_with("**") {
            let after = &remaining[1..];
            // Buscar cierre * que no sea **
            if let Some(end) = after.find('*') {
                if end > 0 && !after[..end].contains('\n') {
                    result.push('_');
                    result.push_str(&escape_markdownv2(&after[..end]));
                    result.push('_');
                    i += 1 + end + 1;
                    continue;
                }
            }
        }

        // _italic_
        if remaining.starts_with('_') {
            let after = &remaining[1..];
            if let Some(end) = after.find('_') {
                if end > 0 && !after[..end].contains('\n') {
                    result.push('_');
                    result.push_str(&escape_markdownv2(&after[..end]));
                    result.push('_');
                    i += 1 + end + 1;
                    continue;
                }
            }
        }

        // [text](url)
        if remaining.starts_with('[') {
            if let Some(bracket_end) = remaining[1..].find("](") {
                let link_text = &remaining[1..1 + bracket_end];
                let url_start = 1 + bracket_end + 2;
                if let Some(paren_end) = remaining[url_start..].find(')') {
                    let url = &remaining[url_start..url_start + paren_end];
                    result.push('[');
                    result.push_str(&escape_markdownv2(link_text));
                    result.push_str("](");
                    result.push_str(&escape_url(url));
                    result.push(')');
                    i += url_start + paren_end + 1;
                    continue;
                }
            }
        }

        // Plain character — escapar si es reservado en MarkdownV2
        let c = remaining.chars().next().unwrap();
        match c {
            '_' | '*' | '[' | ']' | '(' | ')' | '~' | '`' | '>' | '#'
            | '+' | '-' | '=' | '|' | '{' | '}' | '.' | '!' | '\\' => {
                result.push('\\');
                result.push(c);
            }
            _ => result.push(c),
        }
        i += c.len_utf8();
    }

    result
}

pub struct StreamingConverter {
    buffer: String,
}

impl StreamingConverter {
    pub fn new() -> Self {
        Self { buffer: String::new() }
    }

    /// Llamar con cada chunk del stream. Devuelve el texto listo para enviar.
    pub fn push(&mut self, chunk: &str) -> String {
        self.buffer.push_str(chunk);

        // Encontrar hasta dónde es seguro convertir
        let safe_end = self.find_safe_boundary();

        if safe_end == 0 {
            return String::new();
        }

        let to_convert: String = self.buffer[..safe_end].to_string();
        self.buffer = self.buffer[safe_end..].to_string();

        md_to_telegram_markdownv2(&to_convert)
    }

    /// Llamar al final del stream para vaciar lo que quede en el buffer.
    pub fn flush(&mut self) -> String {
        let remaining = self.buffer.clone();
        self.buffer.clear();
        md_to_telegram_markdownv2(&remaining)
    }

    /// Retorna el índice hasta donde es seguro procesar.
    /// Retiene todo lo que podría ser un token incompleto.
    fn find_safe_boundary(&self) -> usize {
        let s = &self.buffer;
        let len = s.len();

        // Detectar ``` incompleto
        if let Some(pos) = self.find_unclosed("```", "```") {
            return pos;
        }
        // Detectar ` incompleto
        if let Some(pos) = self.find_unclosed("`", "`") {
            return pos;
        }
        // Detectar ** incompleto (antes de *)
        if let Some(pos) = self.find_unclosed("**", "**") {
            return pos;
        }
        // Detectar * incompleto
        if let Some(pos) = self.find_unclosed("*", "*") {
            return pos;
        }
        // Detectar _ incompleto
        if let Some(pos) = self.find_unclosed("_", "_") {
            return pos;
        }
        // Detectar [ sin cerrar con ](
        if let Some(pos) = find_open_link(s) {
            return pos;
        }
        // Detectar heading incompleto (línea que empieza con # sin \n)
        if let Some(pos) = find_incomplete_heading(s) {
            return pos;
        }

        // Si el texto termina con un carácter que podría ser inicio de token,
        // retener los últimos bytes por si acaso
        let tail_risk = self.tail_is_risky();
        if tail_risk > 0 {
            return len.saturating_sub(tail_risk);
        }

        len
    }

    /// Encuentra la posición del token de apertura si no tiene cierre.
    fn find_unclosed(&self, open: &str, close: &str) -> Option<usize> {
        let s = &self.buffer;
        let mut search_from = 0;

        while let Some(start) = s[search_from..].find(open) {
            let abs_start = search_from + start;
            let after_open = abs_start + open.len();

            if let Some(end) = s[after_open..].find(close) {
                // Token cerrado, seguir buscando
                search_from = after_open + end + close.len();
            } else {
                // Token abierto sin cerrar: retener desde aquí
                return Some(abs_start);
            }
        }
        None
    }

    /// Si el texto termina con caracteres que pueden ser inicio de token.
    fn tail_is_risky(&self) -> usize {
        let s = &self.buffer;
        if s.ends_with("``") { return 2; }
        if s.ends_with('`') { return 1; }
        if s.ends_with("**") { return 2; }
        if s.ends_with('*') { return 1; }
        if s.ends_with('_') { return 1; }
        if s.ends_with('[') { return 1; }
        if s.ends_with('#') { return 1; }
        0
    }
}

/// Detecta un heading incompleto: línea que empieza con # sin \n al final.
fn find_incomplete_heading(s: &str) -> Option<usize> {
    // Buscar la última línea del buffer
    let line_start = s.rfind('\n').map(|p| p + 1).unwrap_or(0);
    let last_line = &s[line_start..];
    if last_line.starts_with('#') && !last_line.contains('\n') {
        // Verificar que es un heading válido (# seguido de espacio)
        let hashes = last_line.bytes().take_while(|&b| b == b'#').count();
        if hashes <= 6 && (last_line.len() == hashes || last_line.as_bytes().get(hashes) == Some(&b' ')) {
            return Some(line_start);
        }
    }
    None
}

fn find_open_link(s: &str) -> Option<usize> {
    let mut i = 0;
    while let Some(pos) = s[i..].find('[') {
        let abs = i + pos;
        let after = &s[abs..];
        if let Some(b) = after.find("](") {
            let url_start = abs + b + 2;
            if s[url_start..].find(')').is_none() {
                return Some(abs);
            }
            i = url_start;
        } else {
            return Some(abs); // [ sin cierre
        }
    }
    None
}
