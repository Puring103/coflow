#[derive(Debug)]
pub(crate) struct FunctionSyntaxError {
    pub(crate) message: String,
    pub(crate) offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Word,
    Number,
    String,
    Symbol,
    End,
}

#[derive(Debug, Clone)]
struct Token {
    kind: Kind,
    text: String,
    offset: usize,
}

pub(crate) fn validate_function_body(source: &str) -> Result<(), FunctionSyntaxError> {
    let mut parser = Parser {
        tokens: lex(source)?,
        pos: 0,
    };
    parser.contents(None)?;
    parser.expect_end()
}

fn lex(source: &str) -> Result<Vec<Token>, FunctionSyntaxError> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < source.len() {
        let ch = source[pos..].chars().next().unwrap_or(' ');
        if ch.is_whitespace() {
            pos += ch.len_utf8();
            continue;
        }
        if ch == '#' {
            while pos < source.len() && !source[pos..].starts_with('\n') {
                pos += source[pos..].chars().next().map_or(0, char::len_utf8);
            }
            continue;
        }
        let start = pos;
        if ch == '$' {
            pos += 1;
            let Some(first) = source[pos..].chars().next() else {
                return Err(error("expected metadata name after `$`", start));
            };
            if first != '_' && !first.is_alphabetic() {
                return Err(error("expected metadata name after `$`", start));
            }
            pos += first.len_utf8();
            while let Some(next) = source[pos..].chars().next() {
                if next == '_' || next.is_alphanumeric() {
                    pos += next.len_utf8();
                } else {
                    break;
                }
            }
            out.push(token(Kind::Word, &source[start..pos], start));
            continue;
        }
        if ch == '_' || ch.is_alphabetic() {
            pos += ch.len_utf8();
            while let Some(next) = source[pos..].chars().next() {
                if next == '_' || next.is_alphanumeric() {
                    pos += next.len_utf8();
                } else {
                    break;
                }
            }
            out.push(token(Kind::Word, &source[start..pos], start));
            continue;
        }
        if ch.is_ascii_digit() {
            pos += 1;
            while source[pos..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
            {
                pos += 1;
            }
            if source[pos..].starts_with('.')
                && source[pos + 1..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
            {
                pos += 1;
                while source[pos..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
                {
                    pos += 1;
                }
            }
            if source[pos..].starts_with(['e', 'E']) {
                pos += 1;
                if source[pos..].starts_with(['+', '-']) {
                    pos += 1;
                }
                let digits = pos;
                while source[pos..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_digit())
                {
                    pos += 1;
                }
                if pos == digits {
                    return Err(error("expected exponent digits", pos));
                }
            }
            out.push(token(Kind::Number, &source[start..pos], start));
            continue;
        }
        if ch == '"' {
            pos += 1;
            let mut interpolation = 0_usize;
            let mut closed = false;
            while pos < source.len() {
                let current = source[pos..].chars().next().unwrap_or(' ');
                pos += current.len_utf8();
                match current {
                    '\\' => {
                        let Some(escaped) = source[pos..].chars().next() else {
                            return Err(error("unterminated string escape", start));
                        };
                        if !matches!(escaped, '"' | '\\' | 'n' | 'r' | 't') {
                            return Err(error(
                                format!("unsupported string escape `\\{escaped}`"),
                                pos - 1,
                            ));
                        }
                        pos += escaped.len_utf8();
                    }
                    '{' if source[pos..].starts_with('{') => pos += 1,
                    '{' => interpolation += 1,
                    '}' if source[pos..].starts_with('}') => pos += 1,
                    '}' if interpolation > 0 => interpolation -= 1,
                    '}' => return Err(error("unmatched `}` in string", pos - 1)),
                    '"' if interpolation == 0 => {
                        closed = true;
                        break;
                    }
                    _ => {}
                }
            }
            if !closed || interpolation != 0 {
                return Err(error("unterminated string literal or interpolation", start));
            }
            out.push(token(Kind::String, &source[start..pos], start));
            continue;
        }
        const SYMBOLS: &[&str] = &[
            "..=", "->", "=>", "::", "//", "**", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||",
            "+=", "-=", "*=", "/=", "..",
        ];
        if let Some(symbol) = SYMBOLS
            .iter()
            .find(|symbol| source[pos..].starts_with(**symbol))
        {
            pos += symbol.len();
            out.push(token(Kind::Symbol, symbol, start));
            continue;
        }
        if "(){}[],:;.?+-*/%~!&|^<>= ".contains(ch) {
            pos += ch.len_utf8();
            out.push(token(Kind::Symbol, &source[start..pos], start));
            continue;
        }
        return Err(error(
            format!("unexpected character `{ch}` in function body"),
            start,
        ));
    }
    out.push(Token {
        kind: Kind::End,
        text: String::new(),
        offset: source.len(),
    });
    Ok(out)
}

fn token(kind: Kind, text: &str, offset: usize) -> Token {
    Token {
        kind,
        text: text.to_string(),
        offset,
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn contents(&mut self, end: Option<&str>) -> Result<(), FunctionSyntaxError> {
        while !end.is_some_and(|end| self.at(end)) && self.peek().kind != Kind::End {
            if self.word("var") {
                self.bump();
                self.expect_kind(Kind::Word, "variable name")?;
                if self.eat(":") {
                    self.ty()?;
                }
                self.expect("=")?;
                self.expr(0, false)?;
                self.expect(";")?;
            } else if self.word("return") {
                self.bump();
                if !self.at(";") {
                    self.expr(0, false)?;
                }
                self.expect(";")?;
            } else if self.word("break") || self.word("continue") {
                self.bump();
                self.expect(";")?;
            } else if self.word("while") {
                self.bump();
                self.expr(0, true)?;
                self.block()?;
                let _ = self.eat(";");
            } else if self.word("for") {
                self.bump();
                self.expect_kind(Kind::Word, "loop binding")?;
                if self.eat(",") {
                    self.expect_kind(Kind::Word, "second loop binding")?;
                }
                self.expect_word("in")?;
                self.expr(0, true)?;
                self.block()?;
                let _ = self.eat(";");
            } else {
                self.expr(0, false)?;
                if self.eat(";") {
                    continue;
                }
                if !end.is_some_and(|end| self.at(end)) && self.peek().kind != Kind::End {
                    return Err(self.fail("expected `;` or end of function block"));
                }
            }
        }
        if end.is_some() && self.peek().kind == Kind::End {
            return Err(self.fail("unterminated function block"));
        }
        Ok(())
    }

    fn block(&mut self) -> Result<(), FunctionSyntaxError> {
        self.expect("{")?;
        self.contents(Some("}"))?;
        self.expect("}")
    }

    fn expr(&mut self, minimum: u8, stop_at_brace: bool) -> Result<(), FunctionSyntaxError> {
        self.prefix(stop_at_brace)?;
        loop {
            if stop_at_brace && self.at("{") {
                break;
            }
            if self.at("(") {
                self.arguments()?;
            } else if self.eat("[") {
                self.expr(0, false)?;
                self.expect("]")?;
            } else if self.eat(".") || self.eat("::") {
                self.expect_kind(Kind::Word, "member name")?;
            } else if self.eat("?") {
            } else if let Some(precedence) = self.precedence() {
                if precedence < minimum {
                    break;
                }
                let right = matches!(
                    self.peek().text.as_str(),
                    "=" | "+=" | "-=" | "*=" | "/=" | "**"
                );
                self.bump();
                self.expr(
                    if right { precedence } else { precedence + 1 },
                    stop_at_brace,
                )?;
            } else {
                break;
            }
        }
        Ok(())
    }

    fn prefix(&mut self, stop_at_brace: bool) -> Result<(), FunctionSyntaxError> {
        if self.peek().kind == Kind::Symbol
            && matches!(self.peek().text.as_str(), "!" | "~" | "-" | "&")
        {
            self.bump();
            return self.expr(12, stop_at_brace);
        }
        if self.word("if") {
            return self.if_expr();
        }
        if self.word("match") {
            return self.match_expr();
        }
        if self.word("fn") {
            return self.lambda();
        }
        match self.peek().kind.clone() {
            Kind::Word => {
                self.bump();
                if !stop_at_brace && self.at("{") {
                    self.object()?;
                }
                Ok(())
            }
            Kind::Number | Kind::String => {
                self.bump();
                Ok(())
            }
            Kind::Symbol if self.at("(") => {
                self.bump();
                if !self.eat(")") {
                    self.expr(0, false)?;
                    self.expect(")")?;
                }
                Ok(())
            }
            Kind::Symbol if self.at("[") => self.array(),
            Kind::Symbol if self.at("{") => self.dict(),
            _ => Err(self.fail("expected expression")),
        }
    }

    fn if_expr(&mut self) -> Result<(), FunctionSyntaxError> {
        self.bump();
        self.expr(0, true)?;
        self.block()?;
        if self.word("else") {
            self.bump();
            if self.word("if") {
                self.if_expr()
            } else {
                self.block()
            }
        } else {
            Ok(())
        }
    }

    fn match_expr(&mut self) -> Result<(), FunctionSyntaxError> {
        self.bump();
        self.expr(0, true)?;
        self.expect("{")?;
        while !self.at("}") {
            let mut depth = 0_usize;
            while !self.at("=>") || depth != 0 {
                if self.peek().kind == Kind::End || (self.at("}") && depth == 0) {
                    return Err(self.fail("expected `=>` in match arm"));
                }
                if self.at("(") {
                    depth += 1;
                } else if self.at(")") {
                    depth = depth.saturating_sub(1);
                }
                self.bump();
            }
            self.bump();
            if self.at("{") {
                self.block()?;
            } else {
                self.expr(0, false)?;
            }
            if !self.eat(",") && !self.at("}") {
                return Err(self.fail("expected `,` after match arm"));
            }
        }
        self.bump();
        Ok(())
    }

    fn lambda(&mut self) -> Result<(), FunctionSyntaxError> {
        self.bump();
        self.expect("(")?;
        if !self.eat(")") {
            loop {
                self.expect_kind(Kind::Word, "lambda parameter")?;
                self.expect(":")?;
                self.ty()?;
                if self.eat(")") {
                    break;
                }
                self.expect(",")?;
            }
        }
        self.expect("->")?;
        self.ty()?;
        self.block()
    }

    fn ty(&mut self) -> Result<(), FunctionSyntaxError> {
        let _ = self.eat("&");
        if self.eat("(") {
            return self.expect(")");
        }
        if self.eat("[") {
            self.ty()?;
            return self.expect("]");
        }
        if self.eat("{") {
            self.ty()?;
            self.expect(":")?;
            self.ty()?;
            return self.expect("}");
        }
        let is_function = self.word("fn");
        self.expect_kind(Kind::Word, "type name")?;
        while self.eat("::") {
            self.expect_kind(Kind::Word, "qualified type name")?;
        }
        if is_function {
            self.expect("(")?;
            if !self.eat(")") {
                loop {
                    let saved = self.pos;
                    if self.peek().kind == Kind::Word {
                        self.bump();
                    }
                    if self.eat(":") {
                        self.ty()?;
                    } else {
                        self.pos = saved;
                        self.ty()?;
                    }
                    if self.eat(")") {
                        break;
                    }
                    self.expect(",")?;
                }
            }
            self.expect("->")?;
            return self.ty();
        }
        if self.eat("<") {
            self.ty()?;
            if self.eat(",") {
                self.ty()?;
            }
            self.expect(">")?;
        }
        Ok(())
    }

    fn arguments(&mut self) -> Result<(), FunctionSyntaxError> {
        self.expect("(")?;
        if self.eat(")") {
            return Ok(());
        }
        loop {
            self.expr(0, false)?;
            if self.eat(")") {
                return Ok(());
            }
            self.expect(",")?;
        }
    }

    fn array(&mut self) -> Result<(), FunctionSyntaxError> {
        self.expect("[")?;
        if self.eat("]") {
            return Ok(());
        }
        loop {
            self.expr(0, false)?;
            if self.eat("]") {
                return Ok(());
            }
            self.expect(",")?;
        }
    }

    fn dict(&mut self) -> Result<(), FunctionSyntaxError> {
        self.expect("{")?;
        if self.eat("}") {
            return Ok(());
        }
        loop {
            self.expr(0, false)?;
            self.expect(":")?;
            self.expr(0, false)?;
            if self.eat("}") {
                return Ok(());
            }
            self.expect(",")?;
        }
    }

    fn object(&mut self) -> Result<(), FunctionSyntaxError> {
        self.expect("{")?;
        if self.eat("}") {
            return Ok(());
        }
        loop {
            self.expect_kind(Kind::Word, "object field")?;
            self.expect(":")?;
            self.expr(0, false)?;
            if self.eat("}") {
                return Ok(());
            }
            self.expect(",")?;
        }
    }

    fn precedence(&self) -> Option<u8> {
        if self.word("is") {
            return Some(5);
        }
        Some(match self.peek().text.as_str() {
            "=" | "+=" | "-=" | "*=" | "/=" => 1,
            "||" => 2,
            "&&" => 3,
            "==" | "!=" | "<" | "<=" | ">" | ">=" => 5,
            ".." | "..=" => 6,
            "|" => 7,
            "^" => 8,
            "&" => 9,
            "<<" | ">>" | "+" | "-" => 10,
            "*" | "/" | "//" | "%" => 11,
            "**" => 12,
            _ => return None,
        })
    }

    fn word(&self, value: &str) -> bool {
        self.peek().kind == Kind::Word && self.peek().text == value
    }
    fn at(&self, value: &str) -> bool {
        self.peek().text == value
    }
    fn eat(&mut self, value: &str) -> bool {
        if self.at(value) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, value: &str) -> Result<(), FunctionSyntaxError> {
        if self.eat(value) {
            Ok(())
        } else {
            Err(self.fail(format!("expected `{value}`")))
        }
    }
    fn expect_word(&mut self, value: &str) -> Result<(), FunctionSyntaxError> {
        if self.word(value) {
            self.bump();
            Ok(())
        } else {
            Err(self.fail(format!("expected `{value}`")))
        }
    }
    fn expect_kind(&mut self, kind: Kind, expected: &str) -> Result<(), FunctionSyntaxError> {
        if self.peek().kind == kind {
            self.bump();
            Ok(())
        } else {
            Err(self.fail(format!("expected {expected}")))
        }
    }
    fn expect_end(&self) -> Result<(), FunctionSyntaxError> {
        if self.peek().kind == Kind::End {
            Ok(())
        } else {
            Err(self.fail("unexpected text after function body"))
        }
    }
    fn bump(&mut self) {
        self.pos = (self.pos + 1).min(self.tokens.len().saturating_sub(1));
    }
    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn fail(&self, message: impl Into<String>) -> FunctionSyntaxError {
        error(message, self.peek().offset)
    }
}

fn error(message: impl Into<String>, offset: usize) -> FunctionSyntaxError {
    FunctionSyntaxError {
        message: message.into(),
        offset,
    }
}

#[cfg(test)]
mod tests {
    use super::validate_function_body;

    #[test]
    fn rejects_invalid_expressions() {
        for body in [
            "+",
            "var value = ;",
            "if true { 1 } trailing",
            "match value { Some(x) x }",
        ] {
            assert!(validate_function_body(body).is_err(), "{body}");
        }
    }
}
