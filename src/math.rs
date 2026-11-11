//! Math in model replies: finds TeX in Markdown, parses a practical subset of it into a
//! layout tree ([`Node`]) and turns inline formulas into Unicode text.
//!
//! [`prepare`] rewrites a reply before Markdown parsing: inline math (`$…$`, `\(…\)`)
//! becomes Unicode text, display math (`$$…$$`, `\[…\]`) becomes a fenced ```` ```math ````
//! block that the UI draws as a laid-out formula. Code spans and fences are left alone.

/// A parsed formula.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Row(Vec<Node>),
    /// A variable: drawn italic.
    Ident(char),
    Number(String),
    /// An operator or symbol, with how much space it wants around it.
    Op(String, Spacing),
    /// Upright text: `\text{…}`, function names such as `sin`.
    Text(String),
    /// `\mathbf{…}`.
    Bold(Box<Node>),
    /// A large operator (∑, ∫); in display math its scripts go above and below.
    Big(String, Limits),
    Frac(Box<Node>, Box<Node>),
    Sqrt(Option<Box<Node>>, Box<Node>),
    Scripts {
        base: Box<Node>,
        sub: Option<Box<Node>>,
        sup: Option<Box<Node>>,
    },
    /// A combining accent over the body (`\hat`, `\vec`, …).
    Accent(char, Box<Node>),
    Overline(Box<Node>),
    /// Rows of cells between optional delimiters; `aligned` columns alternate right/left.
    Matrix {
        rows: Vec<Vec<Node>>,
        open: &'static str,
        close: &'static str,
        aligned: bool,
    },
    /// Horizontal space in ems.
    Space(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spacing {
    /// `a+b` in TeX: binary operator.
    Binary,
    /// `=`, `<`, `\to`: relation, a little wider.
    Relation,
    /// `,` `;`: space after only.
    Punct,
    /// Brackets and plain symbols.
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limits {
    /// ∑ ∏ lim: scripts above and below in display math.
    Over,
    /// ∫: scripts to the side.
    Side,
}

/// Rewrites a Markdown reply so math can be shown: see the module docs.
pub fn prepare(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    let mut prose = String::new();
    let mut fence: Option<(char, usize)> = None;
    for line in markdown.split_inclusive('\n') {
        let marker = fence_marker(line);
        match (fence, marker) {
            (Some((c, n)), Some((mc, mn, rest)))
                if mc == c && mn >= n && rest.trim().is_empty() =>
            {
                out.push_str(line);
                fence = None;
            }
            (Some(_), _) => out.push_str(line),
            (None, Some((c, n, _))) => {
                out.push_str(&convert_prose(&std::mem::take(&mut prose)));
                out.push_str(line);
                fence = Some((c, n));
            }
            (None, None) => prose.push_str(line),
        }
    }
    out.push_str(&convert_prose(&prose));
    out
}

/// ```` ``` ```` or `~~~` (3+) after at most three spaces: (char, length, info string).
fn fence_marker(line: &str) -> Option<(char, usize, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let c = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = rest.chars().take_while(|x| *x == c).count();
    (n >= 3).then(|| (c, n, &rest[n..]))
}

fn convert_prose(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let b = bytes[i];
        if b == b'\\' {
            if let Some(after) = rest.strip_prefix("\\[")
                && let Some(end) = after.find("\\]")
            {
                display(&mut out, &after[..end]);
                i += 2 + end + 2;
                continue;
            }
            if let Some(after) = rest.strip_prefix("\\(")
                && let Some(end) = after.find("\\)")
                && !after[..end].contains("\n\n")
            {
                inline(&mut out, &after[..end]);
                i += 2 + end + 2;
                continue;
            }
            // Keep Markdown escapes (`\$`, `\*`) as they are.
            let len = rest.chars().nth(1).map_or(1, |c| 1 + c.len_utf8());
            out.push_str(&rest[..len]);
            i += len;
            continue;
        }
        if b == b'`' {
            let run = rest.bytes().take_while(|c| *c == b'`').count();
            let ticks = &rest[..run];
            let close = rest[run..].find(ticks).map(|at| run + at);
            let end = match close {
                // The closing run must be exactly as long.
                Some(at) if !rest[at + run..].starts_with('`') => at + run,
                _ => run,
            };
            out.push_str(&rest[..end]);
            i += end;
            continue;
        }
        if b == b'$' {
            if let Some(after) = rest.strip_prefix("$$") {
                if let Some(end) = after.find("$$") {
                    display(&mut out, &after[..end]);
                    i += 2 + end + 2;
                } else {
                    out.push_str("$$");
                    i += 2;
                }
                continue;
            }
            if let Some(end) = inline_dollar(&rest[1..]) {
                inline(&mut out, &rest[1..1 + end]);
                i += 1 + end + 1;
                continue;
            }
        }
        let c = rest.chars().next().unwrap_or_default();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// Closing `$` for an inline formula opened just before `rest`, Pandoc style: the formula
/// can't start or end with a space and the closing `$` can't be followed by a digit, so
/// "$5 and $10" stays plain text.
fn inline_dollar(rest: &str) -> Option<usize> {
    let first = rest.chars().next()?;
    if first.is_whitespace() || first == '$' {
        return None;
    }
    let mut escaped = false;
    for (at, c) in rest.char_indices() {
        match c {
            '\n' if rest[..at].ends_with('\n') => return None,
            '\\' => {
                escaped = !escaped;
                continue;
            }
            '$' if !escaped && at > 0 => {
                let before = rest[..at].chars().next_back()?;
                let after = rest[at + 1..].chars().next();
                if before.is_whitespace() || after.is_some_and(|a| a.is_ascii_digit()) {
                    return None;
                }
                return Some(at);
            }
            _ => {}
        }
        escaped = false;
    }
    None
}

fn display(out: &mut String, tex: &str) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("```math\n");
    out.push_str(tex.trim().trim_matches('\n'));
    out.push_str("\n```\n");
}

fn inline(out: &mut String, tex: &str) {
    for c in unicode(&parse(tex)).chars() {
        // Markdown would read these as emphasis, links or code.
        if matches!(c, '*' | '_' | '[' | ']' | '`' | '<' | '\\' | '~') {
            out.push('\\');
        }
        out.push(c);
    }
}

// ---------------------------------------------------------------------------------------
// Parsing

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Command(String),
    Open,
    Close,
    Sup,
    Sub,
    Amp,
    Space,
    Char(char),
}

fn tokens(tex: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = tex.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                let mut name = String::new();
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphabetic() {
                        name.push(n);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if name.is_empty()
                    && let Some(n) = chars.next()
                {
                    name.push(n);
                }
                out.push(Token::Command(name));
            }
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '^' => out.push(Token::Sup),
            '_' => out.push(Token::Sub),
            '&' => out.push(Token::Amp),
            c if c.is_whitespace() => {
                if out.last() != Some(&Token::Space) {
                    out.push(Token::Space);
                }
            }
            c => out.push(Token::Char(c)),
        }
    }
    out
}

/// Parses TeX math. Never fails: anything unknown is shown as written.
pub fn parse(tex: &str) -> Node {
    let tokens = tokens(tex);
    let mut parser = Parser {
        tokens: &tokens,
        at: 0,
    };
    let mut rows = parser.rows(&[]);
    // A stray `}` ends `rows` early: skip it and keep going on a new line.
    while parser.at < tokens.len() {
        parser.at += 1;
        rows.extend(parser.rows(&[]));
    }
    if rows.len() > 1 {
        // Top-level `\\`: stacked lines, like `gathered`.
        return Node::Matrix {
            rows,
            open: "",
            close: "",
            aligned: true,
        };
    }
    let mut row = rows.into_iter().next().unwrap_or_default();
    match row.len() {
        1 => row.pop().unwrap_or(Node::Row(Vec::new())),
        _ => Node::Row(row),
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn skip_space(&mut self) {
        while self.tokens.get(self.at) == Some(&Token::Space) {
            self.at += 1;
        }
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.at).cloned();
        self.at += 1;
        token
    }

    /// Rows of `&`-separated cells up to a `}` or `\end`/`\right` (left for the caller).
    fn rows(&mut self, stop: &[&str]) -> Vec<Vec<Node>> {
        let mut rows = Vec::new();
        let mut cells = Vec::new();
        let mut cell = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                None | Some(Token::Close) => break,
                Some(Token::Command(c)) if stop.contains(&c.as_str()) => break,
                Some(Token::Amp) => {
                    self.at += 1;
                    cells.push(row_node(std::mem::take(&mut cell)));
                }
                Some(Token::Command(c)) if c == "\\" || c == "cr" => {
                    self.at += 1;
                    cells.push(row_node(std::mem::take(&mut cell)));
                    rows.push(std::mem::take(&mut cells));
                }
                _ => {
                    if let Some(node) = self.atom_with_scripts() {
                        cell.push(node);
                    }
                }
            }
        }
        if !cell.is_empty() || !cells.is_empty() || rows.is_empty() {
            cells.push(row_node(cell));
            rows.push(cells);
        }
        rows
    }

    /// A `{…}` group or a single atom, as an argument.
    fn argument(&mut self) -> Node {
        self.skip_space();
        match self.peek() {
            Some(Token::Open) => {
                self.at += 1;
                let node = self.group_body();
                if self.peek() == Some(&Token::Close) {
                    self.at += 1;
                }
                node
            }
            _ => self.atom().unwrap_or(Node::Row(Vec::new())),
        }
    }

    fn group_body(&mut self) -> Node {
        let mut items = Vec::new();
        loop {
            self.skip_space();
            match self.peek() {
                None | Some(Token::Close) => break,
                // Line breaks and alignment inside a plain group: ignore.
                Some(Token::Amp) => self.at += 1,
                Some(Token::Command(c)) if c == "\\" => self.at += 1,
                _ => {
                    if let Some(node) = self.atom_with_scripts() {
                        items.push(node);
                    }
                }
            }
        }
        row_node(items)
    }

    /// Raw text of a `{…}` group, for `\text` and `\begin`.
    fn raw_group(&mut self) -> String {
        self.skip_space();
        if self.peek() != Some(&Token::Open) {
            return match self.next() {
                Some(Token::Char(c)) => c.to_string(),
                _ => String::new(),
            };
        }
        self.at += 1;
        let mut depth = 0;
        let mut text = String::new();
        while let Some(token) = self.next() {
            match token {
                Token::Open => depth += 1,
                Token::Close if depth == 0 => break,
                Token::Close => depth -= 1,
                Token::Char(c) => text.push(c),
                Token::Command(c) if c == " " || c == "," || c == ";" => text.push(' '),
                Token::Command(c) if c.len() == 1 => text.push_str(&c),
                Token::Command(c) => text.push_str(&c),
                Token::Sup => text.push('^'),
                Token::Sub => text.push('_'),
                Token::Amp => text.push('&'),
                Token::Space => text.push(' '),
            }
        }
        text
    }

    fn atom_with_scripts(&mut self) -> Option<Node> {
        let base = self.atom()?;
        let (mut sub, mut sup) = (None, None);
        loop {
            let resume = self.at;
            self.skip_space();
            match self.peek() {
                Some(Token::Sup) if sup.is_none() => {
                    self.at += 1;
                    sup = Some(Box::new(self.argument()));
                }
                Some(Token::Sub) if sub.is_none() => {
                    self.at += 1;
                    sub = Some(Box::new(self.argument()));
                }
                // A prime is a superscript.
                Some(Token::Char('\'')) if sup.is_none() => {
                    let mut primes = String::new();
                    while self.peek() == Some(&Token::Char('\'')) {
                        self.at += 1;
                        primes.push('′');
                    }
                    sup = Some(Box::new(Node::Op(primes, Spacing::None)));
                }
                _ => {
                    self.at = resume;
                    break;
                }
            }
        }
        Some(if sub.is_none() && sup.is_none() {
            base
        } else {
            Node::Scripts {
                base: Box::new(base),
                sub,
                sup,
            }
        })
    }

    /// One atom; `None` at a `}` or the end (left unconsumed).
    fn atom(&mut self) -> Option<Node> {
        self.skip_space();
        if matches!(self.peek(), None | Some(Token::Close)) {
            return None;
        }
        let token = self.next()?;
        Some(match token {
            Token::Open => {
                let node = self.group_body();
                if self.peek() == Some(&Token::Close) {
                    self.at += 1;
                }
                node
            }
            Token::Close | Token::Amp | Token::Space => Node::Row(Vec::new()),
            // A script with nothing before it applies to an empty base.
            Token::Sup | Token::Sub => {
                self.at -= 1;
                Node::Row(Vec::new())
            }
            Token::Char(c) if c.is_ascii_digit() || c == '.' => {
                let mut number = c.to_string();
                while let Some(Token::Char(n)) = self.peek()
                    && (n.is_ascii_digit() || *n == '.')
                {
                    number.push(*n);
                    self.at += 1;
                }
                Node::Number(number)
            }
            Token::Char(c) if c.is_alphabetic() => Node::Ident(c),
            Token::Char(c) => Node::Op(
                match c {
                    '-' => '−',
                    '*' => '∗',
                    '\'' => '′',
                    c => c,
                }
                .to_string(),
                spacing_of(c),
            ),
            Token::Command(name) => self.command(&name),
        })
    }

    fn command(&mut self, name: &str) -> Node {
        if let Some(symbol) = symbol(name) {
            return symbol;
        }
        match name {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let num = self.argument();
                let den = self.argument();
                Node::Frac(Box::new(num), Box::new(den))
            }
            "binom" | "dbinom" | "tbinom" => {
                let top = self.argument();
                let bottom = self.argument();
                Node::Matrix {
                    rows: vec![vec![top], vec![bottom]],
                    open: "(",
                    close: ")",
                    aligned: false,
                }
            }
            "sqrt" => {
                let index = if self.peek() == Some(&Token::Char('[')) {
                    self.at += 1;
                    let mut items = Vec::new();
                    loop {
                        self.skip_space();
                        if matches!(self.peek(), None | Some(Token::Char(']') | Token::Close)) {
                            break;
                        }
                        if let Some(node) = self.atom_with_scripts() {
                            items.push(node);
                        }
                    }
                    self.at += 1;
                    Some(Box::new(row_node(items)))
                } else {
                    None
                };
                Node::Sqrt(index, Box::new(self.argument()))
            }
            "text" | "textrm" | "mathrm" | "operatorname" | "textit" | "mbox" | "textsf"
            | "mathsf" | "texttt" | "mathtt" | "mathit" => Node::Text(self.raw_group()),
            "mathbf" | "textbf" | "boldsymbol" | "bm" => Node::Bold(Box::new(self.argument())),
            "mathbb" => styled(&self.raw_group(), double_struck),
            "mathcal" | "mathscr" => styled(&self.raw_group(), script),
            "mathfrak" => Node::Text(self.raw_group()),
            "hat" | "widehat" => self.accent('\u{302}'),
            "bar" => self.accent('\u{304}'),
            "tilde" | "widetilde" => self.accent('\u{303}'),
            "vec" | "overrightarrow" => self.accent('\u{20d7}'),
            "dot" => self.accent('\u{307}'),
            "ddot" => self.accent('\u{308}'),
            "overline" => Node::Overline(Box::new(self.argument())),
            "underline" | "boxed" | "displaystyle" | "textstyle" | "mathop" | "mathrel"
            | "mathbin" | "mathord" => self.argument(),
            "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl"
            | "Bigr" | "biggl" | "biggr" | "middle" => match self.next() {
                Some(Token::Char('.')) | None => Node::Row(Vec::new()),
                Some(Token::Char(c)) => Node::Op(c.to_string(), Spacing::None),
                Some(Token::Command(c)) => {
                    symbol(&c).unwrap_or_else(|| Node::Op(c.clone(), Spacing::None))
                }
                Some(_) => Node::Row(Vec::new()),
            },
            "begin" => self.environment(),
            "end" => {
                self.raw_group();
                Node::Row(Vec::new())
            }
            "," | ">" => Node::Space(0.17),
            ":" | ";" => Node::Space(0.22),
            " " => Node::Space(0.25),
            "!" => Node::Row(Vec::new()),
            "quad" => Node::Space(1.0),
            "qquad" => Node::Space(2.0),
            "{" | "}" | "|" | "#" | "%" | "&" | "$" | "_" => Node::Op(
                if name == "|" { "‖" } else { name }.to_owned(),
                Spacing::None,
            ),
            "\\" => Node::Row(Vec::new()),
            other => Node::Text(format!("\\{other}")),
        }
    }

    fn accent(&mut self, mark: char) -> Node {
        Node::Accent(mark, Box::new(self.argument()))
    }

    fn environment(&mut self) -> Node {
        let name = self.raw_group();
        let name = name.trim_end_matches('*');
        // `array`'s column spec.
        if name == "array" && self.peek() == Some(&Token::Open) {
            self.raw_group();
        }
        let rows = self.rows(&["end"]);
        if matches!(self.peek(), Some(Token::Command(c)) if c == "end") {
            self.at += 1;
            self.raw_group();
        }
        let (open, close) = match name {
            "pmatrix" => ("(", ")"),
            "bmatrix" => ("[", "]"),
            "Bmatrix" => ("{", "}"),
            "vmatrix" => ("|", "|"),
            "Vmatrix" => ("‖", "‖"),
            "cases" => ("{", ""),
            _ => ("", ""),
        };
        let aligned = matches!(
            name,
            "aligned" | "align" | "alignat" | "alignedat" | "split" | "eqnarray"
        );
        Node::Matrix {
            rows,
            open,
            close,
            aligned,
        }
    }
}

fn row_node(mut items: Vec<Node>) -> Node {
    if items.len() == 1 {
        items.pop().unwrap_or(Node::Row(Vec::new()))
    } else {
        Node::Row(items)
    }
}

fn styled(text: &str, map: fn(char) -> char) -> Node {
    Node::Text(text.chars().map(map).collect())
}

fn spacing_of(c: char) -> Spacing {
    match c {
        '+' | '-' | '*' => Spacing::Binary,
        '=' | '<' | '>' | ':' => Spacing::Relation,
        ',' | ';' => Spacing::Punct,
        _ => Spacing::None,
    }
}

fn op(s: &str, spacing: Spacing) -> Option<Node> {
    Some(Node::Op(s.to_owned(), spacing))
}

/// Single-symbol commands: Greek letters, operators, relations, arrows, function names.
fn symbol(name: &str) -> Option<Node> {
    use Spacing::{Binary as B, None as N, Punct as P, Relation as R};
    let greek = match name {
        "alpha" => 'α',
        "beta" => 'β',
        "gamma" => 'γ',
        "delta" => 'δ',
        "epsilon" => 'ϵ',
        "varepsilon" => 'ε',
        "zeta" => 'ζ',
        "eta" => 'η',
        "theta" => 'θ',
        "vartheta" => 'ϑ',
        "iota" => 'ι',
        "kappa" => 'κ',
        "lambda" => 'λ',
        "mu" => 'μ',
        "nu" => 'ν',
        "xi" => 'ξ',
        "pi" => 'π',
        "varpi" => 'ϖ',
        "rho" => 'ρ',
        "varrho" => 'ϱ',
        "sigma" => 'σ',
        "varsigma" => 'ς',
        "tau" => 'τ',
        "upsilon" => 'υ',
        "phi" => 'ϕ',
        "varphi" => 'φ',
        "chi" => 'χ',
        "psi" => 'ψ',
        "omega" => 'ω',
        "Gamma" => 'Γ',
        "Delta" => 'Δ',
        "Theta" => 'Θ',
        "Lambda" => 'Λ',
        "Xi" => 'Ξ',
        "Pi" => 'Π',
        "Sigma" => 'Σ',
        "Upsilon" => 'Υ',
        "Phi" => 'Φ',
        "Psi" => 'Ψ',
        "Omega" => 'Ω',
        "ell" => 'ℓ',
        "hbar" => 'ℏ',
        "imath" => 'ı',
        "jmath" => 'ȷ',
        _ => '\0',
    };
    if greek != '\0' {
        return Some(Node::Ident(greek));
    }
    match name {
        "sum" => Some(Node::Big("∑".into(), Limits::Over)),
        "prod" => Some(Node::Big("∏".into(), Limits::Over)),
        "coprod" => Some(Node::Big("∐".into(), Limits::Over)),
        "bigcup" => Some(Node::Big("⋃".into(), Limits::Over)),
        "bigcap" => Some(Node::Big("⋂".into(), Limits::Over)),
        "bigoplus" => Some(Node::Big("⨁".into(), Limits::Over)),
        "bigotimes" => Some(Node::Big("⨂".into(), Limits::Over)),
        "int" => Some(Node::Big("∫".into(), Limits::Side)),
        "iint" => Some(Node::Big("∬".into(), Limits::Side)),
        "iiint" => Some(Node::Big("∭".into(), Limits::Side)),
        "oint" => Some(Node::Big("∮".into(), Limits::Side)),
        "lim" | "max" | "min" | "sup" | "inf" | "limsup" | "liminf" | "argmax" | "argmin" => {
            Some(Node::Big(name.into(), Limits::Over))
        }
        "sin" | "cos" | "tan" | "cot" | "sec" | "csc" | "arcsin" | "arccos" | "arctan" | "sinh"
        | "cosh" | "tanh" | "coth" | "log" | "ln" | "lg" | "exp" | "det" | "gcd" | "deg"
        | "dim" | "ker" | "arg" | "Pr" | "hom" | "sgn" => Some(Node::Text(name.into())),
        "mod" | "bmod" => op("mod", B),
        "pmod" => op("mod", B),
        "times" => op("×", B),
        "cdot" => op("·", B),
        "pm" => op("±", B),
        "mp" => op("∓", B),
        "div" => op("÷", B),
        "ast" => op("∗", B),
        "star" => op("⋆", B),
        "circ" => op("∘", B),
        "bullet" => op("•", B),
        "oplus" => op("⊕", B),
        "otimes" => op("⊗", B),
        "cup" => op("∪", B),
        "cap" => op("∩", B),
        "setminus" => op("∖", B),
        "wedge" | "land" => op("∧", B),
        "vee" | "lor" => op("∨", B),
        "leq" | "le" => op("≤", R),
        "geq" | "ge" => op("≥", R),
        "neq" | "ne" => op("≠", R),
        "ll" => op("≪", R),
        "gg" => op("≫", R),
        "approx" => op("≈", R),
        "equiv" => op("≡", R),
        "sim" => op("∼", R),
        "simeq" => op("≃", R),
        "cong" => op("≅", R),
        "propto" => op("∝", R),
        "in" => op("∈", R),
        "notin" => op("∉", R),
        "ni" => op("∋", R),
        "subset" => op("⊂", R),
        "subseteq" => op("⊆", R),
        "supset" => op("⊃", R),
        "supseteq" => op("⊇", R),
        "perp" => op("⊥", R),
        "parallel" => op("∥", R),
        "mid" => op("∣", R),
        "to" | "rightarrow" => op("→", R),
        "gets" | "leftarrow" => op("←", R),
        "leftrightarrow" => op("↔", R),
        "Rightarrow" => op("⇒", R),
        "Leftarrow" => op("⇐", R),
        "Leftrightarrow" => op("⇔", R),
        "implies" | "Longrightarrow" => op("⟹", R),
        "impliedby" | "Longleftarrow" => op("⟸", R),
        "iff" | "Longleftrightarrow" => op("⟺", R),
        "longrightarrow" => op("⟶", R),
        "mapsto" => op("↦", R),
        "uparrow" => op("↑", R),
        "downarrow" => op("↓", R),
        "colon" => op(":", P),
        "infty" => op("∞", N),
        "partial" => op("∂", N),
        "nabla" => op("∇", N),
        "forall" => op("∀", N),
        "exists" => op("∃", N),
        "nexists" => op("∄", N),
        "emptyset" | "varnothing" => op("∅", N),
        "neg" | "lnot" => op("¬", N),
        "angle" => op("∠", N),
        "degree" => op("°", N),
        "prime" => op("′", N),
        "aleph" => op("ℵ", N),
        "Re" => op("ℜ", N),
        "Im" => op("ℑ", N),
        "ldots" | "dots" | "dotsc" | "dotso" => op("…", N),
        "cdots" | "dotsb" | "dotsm" => op("⋯", N),
        "vdots" => op("⋮", N),
        "ddots" => op("⋱", N),
        "langle" => op("⟨", N),
        "rangle" => op("⟩", N),
        "lfloor" => op("⌊", N),
        "rfloor" => op("⌋", N),
        "lceil" => op("⌈", N),
        "rceil" => op("⌉", N),
        "lvert" | "rvert" | "vert" => op("|", N),
        "lVert" | "rVert" | "Vert" => op("‖", N),
        "lbrace" => op("{", N),
        "rbrace" => op("}", N),
        "triangle" => op("△", N),
        "square" => op("□", N),
        "checkmark" => op("✓", N),
        _ => None,
    }
}

fn double_struck(c: char) -> char {
    match c {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        'A'..='Z' => char::from_u32(0x1D538 + (c as u32 - 'A' as u32)).unwrap_or(c),
        '1' => '𝟙',
        c => c,
    }
}

fn script(c: char) -> char {
    match c {
        'B' => 'ℬ',
        'E' => 'ℰ',
        'F' => 'ℱ',
        'H' => 'ℋ',
        'I' => 'ℐ',
        'L' => 'ℒ',
        'M' => 'ℳ',
        'R' => 'ℛ',
        'A'..='Z' => char::from_u32(0x1D49C + (c as u32 - 'A' as u32)).unwrap_or(c),
        c => c,
    }
}

// ---------------------------------------------------------------------------------------
// Unicode (inline math)

/// A formula as one line of plain Unicode text: `\frac{a}{b}` → `a/b`, `x^2` → `x²`.
pub fn unicode(node: &Node) -> String {
    let mut out = String::new();
    write_unicode(node, &mut out);
    out.trim().to_owned()
}

fn write_unicode(node: &Node, out: &mut String) {
    match node {
        Node::Row(items) => {
            for (index, item) in items.iter().enumerate() {
                // A leading minus, or one after an operator or opening bracket, is unary.
                let unary = matches!(item, Node::Op(s, Spacing::Binary) if s == "−" || s == "±")
                    && (index == 0
                        || matches!(&items[index - 1], Node::Op(s, sp)
                            if *sp != Spacing::None || matches!(s.as_str(), "(" | "[" | "{")));
                if unary {
                    out.push_str(if matches!(item, Node::Op(s, _) if s == "±") {
                        "±"
                    } else {
                        "−"
                    });
                } else {
                    write_unicode(item, out);
                }
                // A function name before its argument: "sin x", "lim f".
                let named = match item {
                    Node::Text(s) | Node::Big(s, _) => {
                        s.len() > 1 && s.chars().all(|c| c.is_ascii_alphabetic())
                    }
                    Node::Scripts { base, .. } => {
                        matches!(&**base, Node::Big(s, _) if s.len() > 1 && s.is_ascii())
                    }
                    _ => false,
                };
                let argument = matches!(
                    items.get(index + 1),
                    Some(Node::Ident(_) | Node::Number(_) | Node::Scripts { .. } | Node::Frac(..))
                );
                if named && argument {
                    out.push('\u{2009}');
                }
            }
        }
        Node::Ident(c) => out.push(*c),
        Node::Number(n) => out.push_str(n),
        Node::Op(s, spacing) => match spacing {
            Spacing::Binary | Spacing::Relation => {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str(s);
                out.push(' ');
            }
            Spacing::Punct => {
                out.push_str(s);
                out.push(' ');
            }
            Spacing::None => out.push_str(s),
        },
        Node::Text(s) | Node::Big(s, _) => out.push_str(s),
        Node::Bold(body) => write_unicode(body, out),
        Node::Frac(num, den) => {
            write_grouped(num, out);
            out.push('/');
            write_grouped(den, out);
        }
        Node::Sqrt(index, body) => {
            match index.as_deref().map(unicode).as_deref() {
                None => out.push('√'),
                Some("3") => out.push('∛'),
                Some("4") => out.push('∜'),
                Some(n) => {
                    out.push_str(&scripted(n, superscript).unwrap_or_else(|| format!("({n})")));
                    out.push('√');
                }
            }
            write_grouped(body, out);
        }
        Node::Scripts { base, sub, sup } => {
            write_unicode(base, out);
            if let Some(sub) = sub {
                let text = unicode(sub);
                out.push_str(&scripted(&text, subscript).unwrap_or_else(|| {
                    if text.chars().count() == 1 {
                        format!("_{text}")
                    } else {
                        format!("_({text})")
                    }
                }));
            }
            if let Some(sup) = sup {
                let text = unicode(sup);
                out.push_str(&scripted(&text, superscript).unwrap_or_else(|| {
                    if text.chars().count() == 1 {
                        format!("^{text}")
                    } else {
                        format!("^({text})")
                    }
                }));
            }
        }
        Node::Accent(mark, body) => {
            write_unicode(body, out);
            out.push(*mark);
        }
        Node::Overline(body) => {
            for c in unicode(body).chars() {
                out.push(c);
                out.push('\u{305}');
            }
        }
        Node::Matrix {
            rows, open, close, ..
        } => {
            out.push_str(if open.is_empty() { "[" } else { open });
            let rows: Vec<String> = rows
                .iter()
                .map(|cells| cells.iter().map(unicode).collect::<Vec<_>>().join(" "))
                .collect();
            out.push_str(&rows.join("; "));
            out.push_str(if open.is_empty() { "]" } else { close });
        }
        Node::Space(em) => {
            if *em >= 1.0 {
                out.push_str("  ");
            } else if !out.ends_with(' ') {
                out.push('\u{2009}');
            }
        }
    }
}

/// Wraps compound parts in parentheses so `a+b/c` keeps its meaning as `(a+b)/c`.
fn write_grouped(node: &Node, out: &mut String) {
    if is_simple(node) {
        write_unicode(node, out);
    } else {
        out.push('(');
        out.push_str(&unicode(node));
        out.push(')');
    }
}

fn is_simple(node: &Node) -> bool {
    match node {
        Node::Ident(_) | Node::Number(_) | Node::Text(_) | Node::Op(..) | Node::Big(..) => true,
        Node::Scripts { base, .. } => is_simple(base),
        Node::Accent(_, body) | Node::Bold(body) | Node::Overline(body) => is_simple(body),
        Node::Sqrt(..) | Node::Matrix { .. } => true,
        Node::Row(items) => {
            items
                .iter()
                .all(|i| !matches!(i, Node::Op(_, s) if *s != Spacing::None))
                && items.len() <= 3
                && items
                    .iter()
                    .all(|i| matches!(i, Node::Ident(_) | Node::Number(_)))
        }
        Node::Frac(..) | Node::Space(_) => false,
    }
}

/// `text` written with `map` if every character has a script form.
fn scripted(text: &str, map: fn(char) -> Option<char>) -> Option<String> {
    text.chars()
        .filter(|c| *c != ' ')
        .map(map)
        .collect::<Option<String>>()
        .filter(|s| !s.is_empty())
}

fn superscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        '+' => '⁺',
        '−' | '-' => '⁻',
        '=' => '⁼',
        '(' => '⁽',
        ')' => '⁾',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        'a' => 'ᵃ',
        'b' => 'ᵇ',
        'c' => 'ᶜ',
        'd' => 'ᵈ',
        'e' => 'ᵉ',
        'f' => 'ᶠ',
        'g' => 'ᵍ',
        'h' => 'ʰ',
        'j' => 'ʲ',
        'k' => 'ᵏ',
        'l' => 'ˡ',
        'm' => 'ᵐ',
        'o' => 'ᵒ',
        'p' => 'ᵖ',
        'r' => 'ʳ',
        's' => 'ˢ',
        't' => 'ᵗ',
        'u' => 'ᵘ',
        'v' => 'ᵛ',
        'w' => 'ʷ',
        'x' => 'ˣ',
        'y' => 'ʸ',
        'z' => 'ᶻ',
        'T' => 'ᵀ',
        '′' => '′',
        '∗' => '*',
        _ => return None,
    })
}

fn subscript(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀',
        '1' => '₁',
        '2' => '₂',
        '3' => '₃',
        '4' => '₄',
        '5' => '₅',
        '6' => '₆',
        '7' => '₇',
        '8' => '₈',
        '9' => '₉',
        '+' => '₊',
        '−' | '-' => '₋',
        '=' => '₌',
        '(' => '₍',
        ')' => '₎',
        'a' => 'ₐ',
        'e' => 'ₑ',
        'h' => 'ₕ',
        'i' => 'ᵢ',
        'j' => 'ⱼ',
        'k' => 'ₖ',
        'l' => 'ₗ',
        'm' => 'ₘ',
        'n' => 'ₙ',
        'o' => 'ₒ',
        'p' => 'ₚ',
        'r' => 'ᵣ',
        's' => 'ₛ',
        't' => 'ₜ',
        'u' => 'ᵤ',
        'v' => 'ᵥ',
        'x' => 'ₓ',
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inline_of(tex: &str) -> String {
        unicode(&parse(tex))
    }

    #[test]
    fn inline_formulas_become_unicode() {
        assert_eq!(inline_of(r"x^2 + y^2 = r^2"), "x² + y² = r²");
        assert_eq!(inline_of(r"A = \pi r^2"), "A = πr²");
        assert_eq!(inline_of(r"\frac{a+b}{2}"), "(a + b)/2");
        assert_eq!(inline_of(r"\frac{1}{n}"), "1/n");
        assert_eq!(inline_of(r"\sqrt{x+1}"), "√(x + 1)");
        assert_eq!(inline_of(r"\sqrt[3]{8} = 2"), "∛8 = 2");
        assert_eq!(inline_of(r"a_1, a_2, \ldots, a_n"), "a₁, a₂, …, aₙ");
        assert_eq!(inline_of(r"e^{i\pi} + 1 = 0"), "e^(iπ) + 1 = 0");
        assert_eq!(
            inline_of(r"\lim_{n \to \infty} x_n"),
            "lim_(n → ∞)\u{2009}xₙ"
        );
        assert_eq!(inline_of(r"x \leq y \neq z"), "x ≤ y ≠ z");
        assert_eq!(inline_of(r"-x"), "−x");
        assert_eq!(inline_of(r"f'(x)"), "f′(x)");
        assert_eq!(inline_of(r"\mathbb{R}^n"), "ℝⁿ");
        assert_eq!(inline_of(r"\sin x"), "sin\u{2009}x");
        assert_eq!(inline_of(r"\text{if } x"), "if x");
    }

    #[test]
    fn display_math_becomes_a_math_fence() {
        let out = prepare("Area:\n\n$$A = \\pi r^2$$\n\nDone.");
        assert_eq!(out, "Area:\n\n```math\nA = \\pi r^2\n```\n\n\nDone.");
        let out = prepare("Area: \\[\nA = \\pi r^2\n\\] done");
        assert_eq!(out, "Area: \n```math\nA = \\pi r^2\n```\n done");
    }

    #[test]
    fn inline_math_is_converted_in_place() {
        assert_eq!(prepare("so $x^2$ grows"), "so x² grows");
        assert_eq!(prepare(r"so \(a_i\) is"), "so aᵢ is");
        // Markdown characters produced by the conversion are escaped.
        assert_eq!(prepare(r"$\text{a_b}$"), r"a\_b");
        assert_eq!(prepare("$a*b$"), "a ∗ b");
    }

    #[test]
    fn prices_and_code_are_left_alone() {
        assert_eq!(prepare("It costs $5 and $10."), "It costs $5 and $10.");
        assert_eq!(prepare("Just $5."), "Just $5.");
        assert_eq!(prepare("use `$x$` here"), "use `$x$` here");
        assert_eq!(prepare("``a ` $x$ b``"), "``a ` $x$ b``");
        let fenced = "```bash\necho $HOME $$\n```\n";
        assert_eq!(prepare(fenced), fenced);
        assert_eq!(prepare(r"\$x\$"), r"\$x\$");
        // An unclosed formula (still streaming) stays as typed.
        assert_eq!(prepare("so $$x = "), "so $$x = ");
    }

    #[test]
    fn parses_structure() {
        assert_eq!(
            parse(r"\frac{a}{b}"),
            Node::Frac(Box::new(Node::Ident('a')), Box::new(Node::Ident('b')))
        );
        assert!(matches!(
            parse(r"\sum_{i=1}^{n} i"),
            Node::Row(items) if matches!(&items[0], Node::Scripts { base, sub: Some(_), sup: Some(_) }
                if matches!(**base, Node::Big(_, Limits::Over)))
        ));
        let Node::Matrix {
            rows, open, close, ..
        } = parse(r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}")
        else {
            panic!("matrix");
        };
        assert_eq!((open, close), ("(", ")"));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].len(), 2);
        let Node::Matrix { rows, aligned, .. } = parse(r"a &= b \\ &= c") else {
            panic!("lines");
        };
        assert!(aligned);
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn broken_tex_never_panics() {
        for tex in [
            r"\frac{a",
            r"}}{{",
            r"x^",
            r"_",
            r"\begin{pmatrix} a &",
            r"\sqrt[",
            r"\left( x",
            r"\",
            r"\unknown{x}",
            "",
        ] {
            let _ = unicode(&parse(tex));
        }
        assert_eq!(inline_of(r"\foo"), r"\foo");
    }
}
