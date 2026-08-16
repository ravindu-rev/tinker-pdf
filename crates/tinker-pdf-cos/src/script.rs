//! A bounded ECMAScript subset, for the scripts a form carries (gap 27).
//!
//! **A form script is untrusted input from a document.** Everything in this
//! module is written to that premise: the source is attacker-controlled text,
//! it arrives through the same door a malformed `cmap` does, and ruling 1
//! applies to it exactly as it applies to the lexer. Nothing here panics, and
//! nothing here runs unbounded.
//!
//! # The boundary is the feature
//!
//! What is here: number and string literals, arithmetic, comparison, the
//! logical and conditional operators, `if`/`else`, `var`, `while` and C-style
//! `for`, blocks, `return`, array literals and `new Array(...)`, member
//! access, indexing, calls, the `event` object, `getField`, and the Acrobat
//! helpers `AFSimple_Calculate`, `AFNumber_Format`, `AFPercent_Format`,
//! `AFDate_Format` and `AFSpecial_Format`.
//!
//! What is not, and is refused rather than approximated: `eval`, `function`,
//! `try`, `switch`, `for...in`, regular expressions, objects, prototypes, the
//! DOM, any file or network surface, and every `app.*` beyond an inert stub.
//! An unknown name is [`ScriptError::UnknownName`] and an unknown member is
//! [`ScriptError::UnknownMember`] — both refuse, because a calculation that
//! guesses is a form that lies.
//!
//! # How a hostile script terminates
//!
//! Three separate bounds, because none of them substitutes for another.
//!
//! - **Depth.** The parser refuses to build a tree nested deeper than
//!   [`limits::MAX_SCRIPT_DEPTH`], counted once per statement and once per
//!   `unary` — which is the one production every expression cycle passes
//!   through, so counting there bounds every recursion without charging ten
//!   levels for one pair of brackets. The evaluator walks the tree the parser
//!   built, so that single cap bounds its stack too and there is no second cap
//!   to get wrong.
//! - **Work.** *A depth cap is not a work cap when the structure branches.*
//!   `while (true) {}` is depth one and costs everything. Every statement
//!   executed and every expression node evaluated charges one step against
//!   [`Budget`], and a script that runs out stops with
//!   [`ScriptError::OutOfSteps`]. The budget is a parameter rather than a
//!   constant so that a *pass* over many scripts is bounded as well as each
//!   script, which is the same reason `MAX_TILE_WORK` is a total.
//! - **Size.** Source length, token count, string length, array length and
//!   variable count each have a cap, so a script cannot turn twenty thousand
//!   cheap steps into gigabytes: repeated concatenation doubles, and twenty
//!   thousand doublings is not a length anything can hold.
//!
//! # PDF-free on purpose
//!
//! Nothing in this module knows what a PDF is. Values reach it through
//! [`Host`], which is two methods over field names and strings, so the
//! interpreter is testable and fuzzable without a document — and so the code
//! that decides *whether a write is allowed* stays in one place, next to the
//! transaction primitive, rather than being re-decided here.

use crate::limits;

/// Why a script would not run.
///
/// Every variant means the script produced **no** value: a caller that gets
/// one of these has nothing to write.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptError {
    /// Source past [`limits::MAX_SCRIPT_LEN`], or a `/JS` the field model
    /// already reported as [`crate::form::Script::Oversize`].
    TooLong,
    /// A character the subset's grammar has no token for.
    BadToken,
    /// More tokens than [`limits::MAX_SCRIPT_TOKENS`].
    TooManyTokens,
    /// Syntax the subset does not accept, including every construct it
    /// deliberately excludes.
    Syntax,
    /// Nesting past [`limits::MAX_SCRIPT_DEPTH`].
    TooDeep,
    /// The step budget ran out. The script does not terminate cheaply, which
    /// for a document-supplied script is the same as not terminating.
    OutOfSteps,
    /// A name the subset does not define.
    UnknownName,
    /// A member the subset does not define on that value.
    UnknownMember,
    /// A call to something that is not a function.
    NotCallable,
    /// An argument of the wrong shape, or a missing one.
    BadArgument,
    /// A field name that is not in the form (12.7.3.2).
    NoSuchField,
    /// The host refused the write — read-only, over `/MaxLen`, or not an
    /// option the field offers.
    FieldRefused,
    /// Assignment to something that is not a place a value can go.
    NotAssignable,
    /// A value that is not a finite number where one is required, or a value
    /// with no sensible text form to store.
    NotStorable,
    /// A string past [`limits::MAX_SCRIPT_STRING`].
    StringTooLong,
    /// An array past [`limits::MAX_SCRIPT_ARRAY`].
    ArrayTooLong,
    /// More variables than [`limits::MAX_SCRIPT_VARS`].
    TooManyVars,
}

impl core::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            ScriptError::TooLong => "the script is too long to run",
            ScriptError::BadToken => "a character the subset has no token for",
            ScriptError::TooManyTokens => "too many tokens",
            ScriptError::Syntax => "syntax outside the supported subset",
            ScriptError::TooDeep => "nested too deeply",
            ScriptError::OutOfSteps => "the step budget ran out",
            ScriptError::UnknownName => "a name the subset does not define",
            ScriptError::UnknownMember => "a member the subset does not define",
            ScriptError::NotCallable => "not a function",
            ScriptError::BadArgument => "an argument of the wrong shape",
            ScriptError::NoSuchField => "no such field",
            ScriptError::FieldRefused => "the field would not take that value",
            ScriptError::NotAssignable => "not a place a value can go",
            ScriptError::NotStorable => "no value a field could hold",
            ScriptError::StringTooLong => "the string is too long",
            ScriptError::ArrayTooLong => "the array is too long",
            ScriptError::TooManyVars => "too many variables",
        })
    }
}

/// The work one run may do, in steps.
///
/// Shared across a whole recalculation pass so that a document choosing to
/// carry a thousand scripts cannot multiply its way past the per-script cap.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    used: u32,
    limit: u32,
}

impl Budget {
    /// A budget of `limit` steps.
    #[must_use]
    pub fn new(limit: u32) -> Budget {
        Budget { used: 0, limit }
    }

    /// Charges one step.
    fn step(&mut self) -> Result<(), ScriptError> {
        self.used = self.used.saturating_add(1);
        if self.used > self.limit {
            return Err(ScriptError::OutOfSteps);
        }
        Ok(())
    }

    /// Charges `steps` at once, for a caller folding one run's spend into a
    /// larger total.
    ///
    /// # Errors
    ///
    /// [`ScriptError::OutOfSteps`] when the budget is exhausted.
    pub fn charge(&mut self, steps: u32) -> Result<(), ScriptError> {
        self.used = self.used.saturating_add(steps);
        if self.used > self.limit {
            return Err(ScriptError::OutOfSteps);
        }
        Ok(())
    }

    /// How many steps have been spent.
    #[must_use]
    pub fn used(&self) -> u32 {
        self.used
    }

    /// Whether the budget is gone.
    #[must_use]
    pub fn is_spent(&self) -> bool {
        self.used >= self.limit
    }
}

/// What the interpreter can see and change.
///
/// Two methods, over names and strings: the interpreter never learns what a
/// PDF is, and the decision about whether a write is *allowed* stays with the
/// implementor, next to the transaction it belongs in.
pub trait Host {
    /// The current value of a field, or `None` when the form has no field of
    /// that fully qualified name (12.7.3.2).
    fn field(&self, name: &str) -> Option<String>;

    /// Records a value for a field. `false` when the field will not take it,
    /// which stops the script with [`ScriptError::FieldRefused`].
    fn set_field(&mut self, name: &str, value: &str) -> bool;
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Word(String),
    Punct(&'static str),
}

/// Longest first, so that `===` never lexes as `==` then `=`.
const PUNCT: &[&str] = &[
    "===", "!==", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "(", ")", "{", "}",
    "[", "]", ",", ";", ".", "+", "-", "*", "/", "%", "=", "<", ">", "!", "?", ":",
];

fn is_word_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == '$'
}

fn is_word_part(c: char) -> bool {
    is_word_start(c) || c.is_ascii_digit()
}

fn lex(source: &str) -> Result<Vec<Tok>, ScriptError> {
    if source.len() > limits::MAX_SCRIPT_LEN {
        return Err(ScriptError::TooLong);
    }
    let chars: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut i = 0usize;

    while i < chars.len() {
        if out.len() > limits::MAX_SCRIPT_TOKENS {
            return Err(ScriptError::TooManyTokens);
        }
        let c = chars[i];

        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // Comments. `/` is also division, so the next character decides.
        if c == '/' && i + 1 < chars.len() {
            if chars[i + 1] == '/' {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
                continue;
            }
            if chars[i + 1] == '*' {
                i += 2;
                let mut closed = false;
                while i + 1 < chars.len() {
                    if chars[i] == '*' && chars[i + 1] == '/' {
                        i += 2;
                        closed = true;
                        break;
                    }
                    i += 1;
                }
                if !closed {
                    // An unterminated comment swallows the rest of the source,
                    // which is what every reader does with one.
                    i = chars.len();
                }
                continue;
            }
        }

        if c.is_ascii_digit() || (c == '.' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit())
        {
            let start = i;
            while i < chars.len() && chars[i].is_ascii_digit() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '.' {
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mark = i;
                i += 1;
                if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                    i += 1;
                }
                if i < chars.len() && chars[i].is_ascii_digit() {
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                } else {
                    // `1e` with no exponent is a number followed by a word.
                    i = mark;
                }
            }
            let text: String = chars[start..i].iter().collect();
            let value = text.parse::<f64>().map_err(|_| ScriptError::BadToken)?;
            out.push(Tok::Num(value));
            continue;
        }

        if is_word_start(c) {
            let start = i;
            while i < chars.len() && is_word_part(chars[i]) {
                i += 1;
            }
            out.push(Tok::Word(chars[start..i].iter().collect()));
            continue;
        }

        if c == '"' || c == '\'' {
            i += 1;
            let mut text = String::new();
            let mut closed = false;
            while i < chars.len() {
                let ch = chars[i];
                if ch == c {
                    i += 1;
                    closed = true;
                    break;
                }
                if ch == '\\' {
                    i += 1;
                    let Some(esc) = chars.get(i) else {
                        return Err(ScriptError::BadToken);
                    };
                    match esc {
                        'n' => text.push('\n'),
                        't' => text.push('\t'),
                        'r' => text.push('\r'),
                        'b' => text.push('\u{8}'),
                        'f' => text.push('\u{c}'),
                        '0' => text.push('\0'),
                        'u' => {
                            let mut value = 0u32;
                            for offset in 1..=4 {
                                let Some(digit) =
                                    chars.get(i + offset).and_then(|d| d.to_digit(16))
                                else {
                                    return Err(ScriptError::BadToken);
                                };
                                value = value * 16 + digit;
                            }
                            i += 4;
                            // A lone surrogate is not a character; the
                            // replacement is what a decoder puts there.
                            text.push(char::from_u32(value).unwrap_or('\u{fffd}'));
                        }
                        'x' => {
                            let mut value = 0u32;
                            for offset in 1..=2 {
                                let Some(digit) =
                                    chars.get(i + offset).and_then(|d| d.to_digit(16))
                                else {
                                    return Err(ScriptError::BadToken);
                                };
                                value = value * 16 + digit;
                            }
                            i += 2;
                            text.push(char::from_u32(value).unwrap_or('\u{fffd}'));
                        }
                        other => text.push(*other),
                    }
                    i += 1;
                    if text.len() > limits::MAX_SCRIPT_STRING {
                        return Err(ScriptError::StringTooLong);
                    }
                    continue;
                }
                text.push(ch);
                i += 1;
                if text.len() > limits::MAX_SCRIPT_STRING {
                    return Err(ScriptError::StringTooLong);
                }
            }
            if !closed {
                return Err(ScriptError::BadToken);
            }
            out.push(Tok::Str(text));
            continue;
        }

        let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
        let Some(punct) = PUNCT.iter().find(|p| rest.starts_with(**p)) else {
            return Err(ScriptError::BadToken);
        };
        i += punct.chars().count();
        out.push(Tok::Punct(punct));
    }

    if out.len() > limits::MAX_SCRIPT_TOKENS {
        return Err(ScriptError::TooManyTokens);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Syntax tree
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

#[derive(Clone, Debug)]
enum Expr {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    Undefined,
    Name(String),
    Array(Vec<Expr>),
    Not(Box<Expr>),
    Neg(Box<Expr>),
    Pos(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Cond(Box<Expr>, Box<Expr>, Box<Expr>),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    /// `place = value`, or `place op= value` when the operator is present.
    Assign(Box<Expr>, Option<BinOp>, Box<Expr>),
}

#[derive(Clone, Debug)]
enum Stmt {
    Empty,
    Expr(Expr),
    Var(Vec<(String, Option<Expr>)>),
    Block(Vec<Stmt>),
    If(Expr, Box<Stmt>, Option<Box<Stmt>>),
    While(Expr, Box<Stmt>),
    For(Option<Box<Stmt>>, Option<Expr>, Option<Expr>, Box<Stmt>),
    Return(Option<Expr>),
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser<'a> {
    toks: &'a [Tok],
    pos: usize,
    depth: u32,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn at_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }

    fn at_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Word(q)) if q == w)
    }

    fn eat_punct(&mut self, p: &str) -> bool {
        if self.at_punct(p) {
            self.pos += 1;
            return true;
        }
        false
    }

    fn eat_word(&mut self, w: &str) -> bool {
        if self.at_word(w) {
            self.pos += 1;
            return true;
        }
        false
    }

    fn expect(&mut self, p: &str) -> Result<(), ScriptError> {
        if self.eat_punct(p) {
            Ok(())
        } else {
            Err(ScriptError::Syntax)
        }
    }

    fn enter(&mut self) -> Result<(), ScriptError> {
        self.depth += 1;
        if self.depth > limits::MAX_SCRIPT_DEPTH {
            return Err(ScriptError::TooDeep);
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    fn program(&mut self) -> Result<Vec<Stmt>, ScriptError> {
        let mut out = Vec::new();
        while self.pos < self.toks.len() {
            out.push(self.statement()?);
        }
        Ok(out)
    }

    fn statement(&mut self) -> Result<Stmt, ScriptError> {
        self.enter()?;
        let result = self.statement_inner();
        self.leave();
        result
    }

    fn statement_inner(&mut self) -> Result<Stmt, ScriptError> {
        if self.eat_punct(";") {
            return Ok(Stmt::Empty);
        }
        if self.eat_punct("{") {
            let mut body = Vec::new();
            while !self.eat_punct("}") {
                if self.pos >= self.toks.len() {
                    return Err(ScriptError::Syntax);
                }
                body.push(self.statement()?);
            }
            return Ok(Stmt::Block(body));
        }
        if self.eat_word("var") {
            let decls = self.declarations()?;
            self.eat_punct(";");
            return Ok(Stmt::Var(decls));
        }
        if self.eat_word("if") {
            self.expect("(")?;
            let cond = self.expression()?;
            self.expect(")")?;
            let then = Box::new(self.statement()?);
            let otherwise = if self.eat_word("else") {
                Some(Box::new(self.statement()?))
            } else {
                None
            };
            return Ok(Stmt::If(cond, then, otherwise));
        }
        if self.eat_word("while") {
            self.expect("(")?;
            let cond = self.expression()?;
            self.expect(")")?;
            let body = Box::new(self.statement()?);
            return Ok(Stmt::While(cond, body));
        }
        if self.eat_word("for") {
            self.expect("(")?;
            let init = if self.at_punct(";") {
                self.pos += 1;
                None
            } else if self.eat_word("var") {
                let decls = self.declarations()?;
                self.expect(";")?;
                Some(Box::new(Stmt::Var(decls)))
            } else {
                let expr = self.expression()?;
                self.expect(";")?;
                Some(Box::new(Stmt::Expr(expr)))
            };
            let cond = if self.at_punct(";") {
                None
            } else {
                Some(self.expression()?)
            };
            self.expect(";")?;
            let step = if self.at_punct(")") {
                None
            } else {
                Some(self.expression()?)
            };
            self.expect(")")?;
            let body = Box::new(self.statement()?);
            return Ok(Stmt::For(init, cond, step, body));
        }
        if self.eat_word("return") {
            if self.eat_punct(";") {
                return Ok(Stmt::Return(None));
            }
            if self.at_punct("}") || self.pos >= self.toks.len() {
                return Ok(Stmt::Return(None));
            }
            let value = self.expression()?;
            self.eat_punct(";");
            return Ok(Stmt::Return(Some(value)));
        }

        let expr = self.expression()?;
        self.eat_punct(";");
        Ok(Stmt::Expr(expr))
    }

    fn declarations(&mut self) -> Result<Vec<(String, Option<Expr>)>, ScriptError> {
        let mut out = Vec::new();
        loop {
            let Some(Tok::Word(name)) = self.peek().cloned() else {
                return Err(ScriptError::Syntax);
            };
            if is_reserved(&name) {
                return Err(ScriptError::Syntax);
            }
            self.pos += 1;
            let init = if self.eat_punct("=") {
                Some(self.assignment()?)
            } else {
                None
            };
            out.push((name, init));
            if out.len() > limits::MAX_SCRIPT_VARS {
                return Err(ScriptError::TooManyVars);
            }
            if !self.eat_punct(",") {
                break;
            }
        }
        Ok(out)
    }

    fn expression(&mut self) -> Result<Expr, ScriptError> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr, ScriptError> {
        let left = self.conditional()?;
        let op = match self.peek() {
            Some(Tok::Punct("=")) => Some(None),
            Some(Tok::Punct("+=")) => Some(Some(BinOp::Add)),
            Some(Tok::Punct("-=")) => Some(Some(BinOp::Sub)),
            Some(Tok::Punct("*=")) => Some(Some(BinOp::Mul)),
            Some(Tok::Punct("/=")) => Some(Some(BinOp::Div)),
            _ => None,
        };
        let Some(op) = op else {
            return Ok(left);
        };
        self.pos += 1;
        let right = self.assignment()?;
        Ok(Expr::Assign(Box::new(left), op, Box::new(right)))
    }

    fn conditional(&mut self) -> Result<Expr, ScriptError> {
        let cond = self.logical_or()?;
        if !self.eat_punct("?") {
            return Ok(cond);
        }
        let then = self.assignment()?;
        self.expect(":")?;
        let otherwise = self.assignment()?;
        Ok(Expr::Cond(
            Box::new(cond),
            Box::new(then),
            Box::new(otherwise),
        ))
    }

    fn logical_or(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.logical_and()?;
        while self.eat_punct("||") {
            let right = self.logical_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn logical_and(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.equality()?;
        while self.eat_punct("&&") {
            let right = self.equality()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn equality(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.relational()?;
        loop {
            // `===` is `==` here: the subset has no value whose type
            // distinction survives a form field, and pretending otherwise
            // would make `"1" === 1` mean something this engine cannot
            // honour.
            let op = match self.peek() {
                Some(Tok::Punct("==" | "===")) => BinOp::Eq,
                Some(Tok::Punct("!=" | "!==")) => BinOp::Ne,
                _ => break,
            };
            self.pos += 1;
            let right = self.relational()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn relational(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.additive()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Punct("<")) => BinOp::Lt,
                Some(Tok::Punct("<=")) => BinOp::Le,
                Some(Tok::Punct(">")) => BinOp::Gt,
                Some(Tok::Punct(">=")) => BinOp::Ge,
                _ => break,
            };
            self.pos += 1;
            let right = self.additive()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn additive(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.multiplicative()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Punct("+")) => BinOp::Add,
                Some(Tok::Punct("-")) => BinOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let right = self.multiplicative()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn multiplicative(&mut self) -> Result<Expr, ScriptError> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Punct("*")) => BinOp::Mul,
                Some(Tok::Punct("/")) => BinOp::Div,
                Some(Tok::Punct("%")) => BinOp::Rem,
                _ => break,
            };
            self.pos += 1;
            let right = self.unary()?;
            left = Expr::Bin(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, ScriptError> {
        self.enter()?;
        let result = self.unary_inner();
        self.leave();
        result
    }

    fn unary_inner(&mut self) -> Result<Expr, ScriptError> {
        if self.eat_punct("!") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.eat_punct("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat_punct("+") {
            return Ok(Expr::Pos(Box::new(self.unary()?)));
        }
        self.postfix()
    }

    fn postfix(&mut self) -> Result<Expr, ScriptError> {
        let mut base = self.primary_inner()?;
        loop {
            if self.eat_punct(".") {
                let Some(Tok::Word(name)) = self.peek().cloned() else {
                    return Err(ScriptError::Syntax);
                };
                self.pos += 1;
                base = Expr::Member(Box::new(base), name);
                continue;
            }
            if self.eat_punct("[") {
                let index = self.expression()?;
                self.expect("]")?;
                base = Expr::Index(Box::new(base), Box::new(index));
                continue;
            }
            if self.eat_punct("(") {
                let args = self.arguments()?;
                base = Expr::Call(Box::new(base), args);
                continue;
            }
            break;
        }
        Ok(base)
    }

    fn arguments(&mut self) -> Result<Vec<Expr>, ScriptError> {
        let mut out = Vec::new();
        if self.eat_punct(")") {
            return Ok(out);
        }
        loop {
            out.push(self.assignment()?);
            if out.len() > limits::MAX_SCRIPT_ARRAY {
                return Err(ScriptError::ArrayTooLong);
            }
            if self.eat_punct(",") {
                continue;
            }
            self.expect(")")?;
            break;
        }
        Ok(out)
    }

    fn primary_inner(&mut self) -> Result<Expr, ScriptError> {
        match self.peek().cloned() {
            Some(Tok::Num(value)) => {
                self.pos += 1;
                Ok(Expr::Num(value))
            }
            Some(Tok::Str(text)) => {
                self.pos += 1;
                Ok(Expr::Str(text))
            }
            Some(Tok::Word(word)) => {
                self.pos += 1;
                match word.as_str() {
                    "true" => Ok(Expr::Bool(true)),
                    "false" => Ok(Expr::Bool(false)),
                    "null" => Ok(Expr::Null),
                    "undefined" => Ok(Expr::Undefined),
                    // `new Array(a, b)` is what Acrobat itself writes into a
                    // generated calculate script, so it is a production of
                    // this grammar rather than general `new`, which the
                    // subset has no objects for.
                    "new" => {
                        if !self.eat_word("Array") {
                            return Err(ScriptError::Syntax);
                        }
                        if !self.eat_punct("(") {
                            return Ok(Expr::Array(Vec::new()));
                        }
                        Ok(Expr::Array(self.arguments()?))
                    }
                    other if is_reserved(other) => Err(ScriptError::Syntax),
                    other => Ok(Expr::Name(other.to_string())),
                }
            }
            Some(Tok::Punct("(")) => {
                self.pos += 1;
                let inner = self.expression()?;
                self.expect(")")?;
                Ok(inner)
            }
            Some(Tok::Punct("[")) => {
                self.pos += 1;
                let mut items = Vec::new();
                if self.eat_punct("]") {
                    return Ok(Expr::Array(items));
                }
                loop {
                    items.push(self.assignment()?);
                    if items.len() > limits::MAX_SCRIPT_ARRAY {
                        return Err(ScriptError::ArrayTooLong);
                    }
                    if self.eat_punct(",") {
                        if self.eat_punct("]") {
                            break;
                        }
                        continue;
                    }
                    self.expect("]")?;
                    break;
                }
                Ok(Expr::Array(items))
            }
            _ => Err(ScriptError::Syntax),
        }
    }
}

/// Words the subset refuses outright, so that a script using one is reported
/// rather than reinterpreted. `eval` is here because an interpreter that
/// silently treats it as an undefined name is one rename away from running it.
fn is_reserved(word: &str) -> bool {
    matches!(
        word,
        "eval"
            | "function"
            | "try"
            | "catch"
            | "finally"
            | "throw"
            | "switch"
            | "case"
            | "default"
            | "do"
            | "in"
            | "instanceof"
            | "typeof"
            | "delete"
            | "void"
            | "with"
            | "break"
            | "continue"
            | "class"
            | "let"
            | "const"
            | "import"
            | "export"
    )
}

// ---------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------

/// The builtins the subset offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Builtin {
    GetField,
    SimpleCalculate,
    NumberFormat,
    PercentFormat,
    DateFormat,
    SpecialFormat,
}

#[derive(Clone, Debug)]
enum Value {
    Num(f64),
    Str(String),
    Bool(bool),
    Null,
    Undefined,
    Array(Vec<Value>),
    /// A field handle, from `getField(...)` or `event.target`.
    Field(String),
    /// The `event` object.
    Event,
    /// `this`, whose only member is `getField`.
    Doc,
    /// `app` and `console`: present so that a script calling one runs, inert
    /// so that calling one does nothing.
    Inert,
    Builtin(Builtin),
}

/// JavaScript's number-to-string, near enough for a field value.
fn number_text(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if n == 0.0 {
        // Rust prints -0 for negative zero; JavaScript prints 0, and a form
        // field showing "-0" is a bug report.
        return "0".to_string();
    }
    format!("{n}")
}

/// A field's stored text as a number.
///
/// Tolerant of what a formatted value looks like when it comes back around:
/// spaces, thousands separators and the four currency symbols a form is
/// likely to carry. An unreadable value is zero, which is what Acrobat's own
/// helpers do with an empty or non-numeric field — refusing would make an
/// untouched form fail to compute its first total.
fn text_number(text: &str) -> f64 {
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, ' ' | ',' | '$' | '\u{a3}' | '\u{20ac}' | '\u{a5}'))
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return 0.0;
    }
    cleaned.parse::<f64>().unwrap_or(0.0)
}

fn truthy(value: &Value) -> bool {
    match value {
        Value::Num(n) => *n != 0.0 && !n.is_nan(),
        Value::Str(s) => !s.is_empty(),
        Value::Bool(b) => *b,
        Value::Null | Value::Undefined => false,
        _ => true,
    }
}

fn to_number(value: &Value) -> f64 {
    match value {
        Value::Num(n) => *n,
        Value::Str(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                0.0
            } else {
                trimmed.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Null => 0.0,
        _ => f64::NAN,
    }
}

fn to_text(value: &Value) -> String {
    match value {
        Value::Num(n) => number_text(*n),
        Value::Str(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Array(items) => items.iter().map(to_text).collect::<Vec<_>>().join(","),
        Value::Field(name) => name.clone(),
        Value::Event => "[event]".to_string(),
        Value::Doc => "[doc]".to_string(),
        Value::Inert | Value::Builtin(_) => "undefined".to_string(),
    }
}

/// The text a field would be given, or a refusal.
///
/// `undefined`, `null` and the non-finite numbers are refused rather than
/// stringified. A total that reads "undefined" or "NaN" is the document that
/// lies which this whole feature exists to avoid, and it is exactly what an
/// inert `app.*` stub produces if its result is ever assigned.
fn to_storable(value: &Value) -> Result<String, ScriptError> {
    match value {
        Value::Num(n) if n.is_finite() => Ok(number_text(*n)),
        Value::Str(s) => Ok(s.clone()),
        Value::Bool(b) => Ok(b.to_string()),
        _ => Err(ScriptError::NotStorable),
    }
}

fn loose_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Null | Value::Undefined, Value::Null | Value::Undefined) => true,
        (Value::Null | Value::Undefined, _) | (_, Value::Null | Value::Undefined) => false,
        _ => {
            let (x, y) = (to_number(a), to_number(b));
            x == y && !x.is_nan()
        }
    }
}

// ---------------------------------------------------------------------------
// Interpreter
// ---------------------------------------------------------------------------

enum Flow {
    Normal,
    Return,
}

/// Where a value can be put.
enum Place {
    Var(String),
    EventValue,
    Field(String),
}

struct Interp<'h, H: Host> {
    host: &'h mut H,
    budget: &'h mut Budget,
    vars: Vec<(String, Value)>,
    target: String,
    event_value: Value,
    event_assigned: bool,
    wrote: Vec<String>,
}

/// What running one script produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// The text `event.value` was left holding, when the script assigned it.
    /// `None` means the script never touched it, which is a field left as it
    /// was rather than a field set to nothing.
    pub value: Option<String>,
    /// The fields the script wrote directly, through
    /// `getField("x").value = ...`, in the order it wrote them.
    pub wrote: Vec<String>,
}

/// Runs one script against a host.
///
/// `target` is the field whose action this is — `event.target` — and
/// `current` is the value the event starts with, which is the field's own
/// value for both the calculate and the format event.
///
/// The same function serves both events, because they differ only in what the
/// caller does with [`Outcome::value`]: a calculate action's result is the
/// field's new `/V`, and a format action's result is a display string that
/// must **not** become `/V` (12.7.3.3 keeps the value and its appearance
/// apart, and a `/V` of "GBP 1,234.00" is a form whose export is unusable).
///
/// # Errors
///
/// Every [`ScriptError`] means the script produced nothing. A caller must
/// write nothing in that case; the whole point of this module's contract is
/// that a refusal and a partial application are not the same thing.
pub fn run(
    source: &str,
    target: &str,
    current: &str,
    host: &mut impl Host,
    budget: &mut Budget,
) -> Result<Outcome, ScriptError> {
    let toks = lex(source)?;
    let mut parser = Parser {
        toks: &toks,
        pos: 0,
        depth: 0,
    };
    let program = parser.program()?;

    let mut interp = Interp {
        host,
        budget,
        vars: Vec::new(),
        target: target.to_string(),
        event_value: Value::Str(current.to_string()),
        event_assigned: false,
        wrote: Vec::new(),
    };
    for stmt in &program {
        if matches!(interp.exec(stmt)?, Flow::Return) {
            break;
        }
    }

    let value = if interp.event_assigned {
        Some(to_storable(&interp.event_value)?)
    } else {
        None
    };
    Ok(Outcome {
        value,
        wrote: interp.wrote,
    })
}

impl<H: Host> Interp<'_, H> {
    fn step(&mut self) -> Result<(), ScriptError> {
        self.budget.step()
    }

    fn exec(&mut self, stmt: &Stmt) -> Result<Flow, ScriptError> {
        self.step()?;
        match stmt {
            Stmt::Empty => Ok(Flow::Normal),
            Stmt::Expr(expr) => {
                self.eval(expr)?;
                Ok(Flow::Normal)
            }
            Stmt::Var(decls) => {
                for (name, init) in decls {
                    let value = match init {
                        Some(expr) => self.eval(expr)?,
                        None => Value::Undefined,
                    };
                    self.declare(name, value)?;
                }
                Ok(Flow::Normal)
            }
            Stmt::Block(body) => {
                for stmt in body {
                    if matches!(self.exec(stmt)?, Flow::Return) {
                        return Ok(Flow::Return);
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::If(cond, then, otherwise) => {
                if truthy(&self.eval(cond)?) {
                    self.exec(then)
                } else if let Some(otherwise) = otherwise {
                    self.exec(otherwise)
                } else {
                    Ok(Flow::Normal)
                }
            }
            Stmt::While(cond, body) => {
                // No iteration cap of its own: the step budget is the cap, and
                // it has to be, because a loop that runs a thousand cheap
                // iterations and one that runs ten expensive ones cost the
                // same and neither an iteration count nor a depth cap can tell
                // them apart.
                while truthy(&self.eval(cond)?) {
                    self.step()?;
                    if matches!(self.exec(body)?, Flow::Return) {
                        return Ok(Flow::Return);
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::For(init, cond, step, body) => {
                if let Some(init) = init {
                    self.exec(init)?;
                }
                loop {
                    if let Some(cond) = cond {
                        if !truthy(&self.eval(cond)?) {
                            break;
                        }
                    }
                    self.step()?;
                    if matches!(self.exec(body)?, Flow::Return) {
                        return Ok(Flow::Return);
                    }
                    if let Some(step) = step {
                        self.eval(step)?;
                    }
                }
                Ok(Flow::Normal)
            }
            Stmt::Return(value) => {
                if let Some(value) = value {
                    self.eval(value)?;
                }
                Ok(Flow::Return)
            }
        }
    }

    fn declare(&mut self, name: &str, value: Value) -> Result<(), ScriptError> {
        if let Some(slot) = self.vars.iter_mut().find(|(n, _)| n == name) {
            slot.1 = value;
            return Ok(());
        }
        if self.vars.len() >= limits::MAX_SCRIPT_VARS {
            return Err(ScriptError::TooManyVars);
        }
        self.vars.push((name.to_string(), value));
        Ok(())
    }

    fn eval(&mut self, expr: &Expr) -> Result<Value, ScriptError> {
        self.step()?;
        match expr {
            Expr::Num(n) => Ok(Value::Num(*n)),
            Expr::Str(s) => Ok(Value::Str(s.clone())),
            Expr::Bool(b) => Ok(Value::Bool(*b)),
            Expr::Null => Ok(Value::Null),
            Expr::Undefined => Ok(Value::Undefined),
            Expr::Name(name) => self.name(name),
            Expr::Array(items) => {
                if items.len() > limits::MAX_SCRIPT_ARRAY {
                    return Err(ScriptError::ArrayTooLong);
                }
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.eval(item)?);
                }
                Ok(Value::Array(out))
            }
            Expr::Not(inner) => Ok(Value::Bool(!truthy(&self.eval(inner)?))),
            Expr::Neg(inner) => Ok(Value::Num(-to_number(&self.eval(inner)?))),
            Expr::Pos(inner) => Ok(Value::Num(to_number(&self.eval(inner)?))),
            Expr::Bin(op, left, right) => {
                let left = self.eval(left)?;
                let right = self.eval(right)?;
                binary(*op, &left, &right)
            }
            Expr::And(left, right) => {
                let left = self.eval(left)?;
                if truthy(&left) {
                    self.eval(right)
                } else {
                    Ok(left)
                }
            }
            Expr::Or(left, right) => {
                let left = self.eval(left)?;
                if truthy(&left) {
                    Ok(left)
                } else {
                    self.eval(right)
                }
            }
            Expr::Cond(cond, then, otherwise) => {
                if truthy(&self.eval(cond)?) {
                    self.eval(then)
                } else {
                    self.eval(otherwise)
                }
            }
            Expr::Member(base, name) => {
                let base = self.eval(base)?;
                self.member(&base, name)
            }
            Expr::Index(base, index) => {
                let base = self.eval(base)?;
                let index = self.eval(index)?;
                match &base {
                    Value::Array(items) => {
                        let n = to_number(&index);
                        if !n.is_finite() || n < 0.0 {
                            return Ok(Value::Undefined);
                        }
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        let at = n as usize;
                        Ok(items.get(at).cloned().unwrap_or(Value::Undefined))
                    }
                    // A named index is a member, which is how `event["value"]`
                    // reaches the same slot `event.value` does.
                    _ => {
                        let name = to_text(&index);
                        self.member(&base, &name)
                    }
                }
            }
            Expr::Call(callee, args) => {
                let callee = self.eval(callee)?;
                let mut values = Vec::with_capacity(args.len());
                for arg in args {
                    values.push(self.eval(arg)?);
                }
                self.call(&callee, &values)
            }
            Expr::Assign(place, op, value) => {
                let place = self.place(place)?;
                let value = match op {
                    None => self.eval(value)?,
                    Some(op) => {
                        let current = self.read_place(&place)?;
                        let right = self.eval(value)?;
                        binary(*op, &current, &right)?
                    }
                };
                self.write_place(&place, value.clone())?;
                Ok(value)
            }
        }
    }

    fn name(&mut self, name: &str) -> Result<Value, ScriptError> {
        if let Some((_, value)) = self.vars.iter().find(|(n, _)| n == name) {
            return Ok(value.clone());
        }
        Ok(match name {
            "event" => Value::Event,
            "this" | "doc" => Value::Doc,
            // The inert stubs the boundary names: present so a script that
            // ends with app.alert(...) still calculates, doing nothing so that
            // "inert" is the truth rather than the intention.
            "app" | "console" => Value::Inert,
            "getField" => Value::Builtin(Builtin::GetField),
            "AFSimple_Calculate" => Value::Builtin(Builtin::SimpleCalculate),
            "AFNumber_Format" => Value::Builtin(Builtin::NumberFormat),
            "AFPercent_Format" => Value::Builtin(Builtin::PercentFormat),
            "AFDate_Format" => Value::Builtin(Builtin::DateFormat),
            "AFSpecial_Format" => Value::Builtin(Builtin::SpecialFormat),
            _ => return Err(ScriptError::UnknownName),
        })
    }

    fn member(&mut self, base: &Value, name: &str) -> Result<Value, ScriptError> {
        self.step()?;
        match base {
            Value::Event => match name {
                "value" => Ok(self.event_value.clone()),
                "target" => Ok(Value::Field(self.target.clone())),
                "targetName" => Ok(Value::Str(self.target.clone())),
                _ => Err(ScriptError::UnknownMember),
            },
            Value::Field(field) => match name {
                "value" => {
                    let Some(text) = self.host.field(field) else {
                        return Err(ScriptError::NoSuchField);
                    };
                    // A field holding a number reads as one, which is what
                    // makes `getField("a").value + getField("b").value` add
                    // rather than concatenate.
                    let trimmed = text.trim();
                    Ok(match trimmed.parse::<f64>() {
                        Ok(n) if !trimmed.is_empty() => Value::Num(n),
                        _ => Value::Str(text),
                    })
                }
                "valueAsString" => {
                    let Some(text) = self.host.field(field) else {
                        return Err(ScriptError::NoSuchField);
                    };
                    Ok(Value::Str(text))
                }
                "name" => Ok(Value::Str(field.clone())),
                _ => Err(ScriptError::UnknownMember),
            },
            Value::Doc => match name {
                "getField" => Ok(Value::Builtin(Builtin::GetField)),
                _ => Err(ScriptError::UnknownMember),
            },
            Value::Inert => Ok(Value::Inert),
            Value::Array(items) if name == "length" => {
                #[allow(clippy::cast_precision_loss)]
                let count = items.len() as f64;
                Ok(Value::Num(count))
            }
            Value::Str(text) if name == "length" => {
                #[allow(clippy::cast_precision_loss)]
                let count = text.chars().count() as f64;
                Ok(Value::Num(count))
            }
            // `getField` on a name the form does not carry gives null, so that
            // the `if (f != null)` idiom works; reaching a member on it is
            // where the missing field becomes an answer nobody can give.
            Value::Null => Err(ScriptError::NoSuchField),
            _ => Err(ScriptError::UnknownMember),
        }
    }

    fn place(&mut self, expr: &Expr) -> Result<Place, ScriptError> {
        match expr {
            Expr::Name(name) => Ok(Place::Var(name.clone())),
            Expr::Member(base, name) => {
                let base = self.eval(base)?;
                match (&base, name.as_str()) {
                    (Value::Event, "value") => Ok(Place::EventValue),
                    (Value::Field(field), "value") => Ok(Place::Field(field.clone())),
                    (Value::Null, _) => Err(ScriptError::NoSuchField),
                    _ => Err(ScriptError::NotAssignable),
                }
            }
            _ => Err(ScriptError::NotAssignable),
        }
    }

    fn read_place(&mut self, place: &Place) -> Result<Value, ScriptError> {
        match place {
            Place::Var(name) => self.name(name),
            Place::EventValue => Ok(self.event_value.clone()),
            Place::Field(field) => {
                let handle = Value::Field(field.clone());
                self.member(&handle, "value")
            }
        }
    }

    fn write_place(&mut self, place: &Place, value: Value) -> Result<(), ScriptError> {
        if let Value::Str(text) = &value {
            if text.len() > limits::MAX_SCRIPT_STRING {
                return Err(ScriptError::StringTooLong);
            }
        }
        match place {
            Place::Var(name) => self.declare(name, value),
            Place::EventValue => {
                self.event_value = value;
                self.event_assigned = true;
                Ok(())
            }
            Place::Field(field) => {
                let text = to_storable(&value)?;
                if !self.host.set_field(field, &text) {
                    return Err(ScriptError::FieldRefused);
                }
                if !self.wrote.iter().any(|w| w == field) {
                    self.wrote.push(field.clone());
                }
                Ok(())
            }
        }
    }

    fn call(&mut self, callee: &Value, args: &[Value]) -> Result<Value, ScriptError> {
        self.step()?;
        let builtin = match callee {
            Value::Builtin(builtin) => *builtin,
            // An inert namespace's members are inert, and calling one is the
            // no-op the boundary promises.
            Value::Inert => return Ok(Value::Undefined),
            _ => return Err(ScriptError::NotCallable),
        };
        match builtin {
            Builtin::GetField => {
                let Some(name) = args.first() else {
                    return Err(ScriptError::BadArgument);
                };
                let name = to_text(name);
                if self.host.field(&name).is_none() {
                    return Ok(Value::Null);
                }
                Ok(Value::Field(name))
            }
            Builtin::SimpleCalculate => self.simple_calculate(args),
            Builtin::NumberFormat => self.number_format(args),
            Builtin::PercentFormat => self.percent_format(args),
            Builtin::DateFormat => self.date_format(args),
            Builtin::SpecialFormat => self.special_format(args),
        }
    }

    /// `AFSimple_Calculate(cFunction, aFields)` — by a wide margin the most
    /// common calculate script there is, because it is the one Acrobat's own
    /// field dialogue generates.
    fn simple_calculate(&mut self, args: &[Value]) -> Result<Value, ScriptError> {
        let (Some(op), Some(list)) = (args.first(), args.get(1)) else {
            return Err(ScriptError::BadArgument);
        };
        let op = to_text(op).trim().to_ascii_uppercase();

        let names: Vec<String> = match list {
            Value::Array(items) => items.iter().map(to_text).collect(),
            // A comma-separated string is the other form generators write.
            Value::Str(text) => text
                .split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect(),
            _ => return Err(ScriptError::BadArgument),
        };
        if names.is_empty() || names.len() > limits::MAX_SCRIPT_ARRAY {
            return Err(ScriptError::BadArgument);
        }

        let mut values = Vec::with_capacity(names.len());
        for name in &names {
            self.step()?;
            let Some(text) = self.host.field(name) else {
                // A sum missing an addend is a wrong total, not a smaller one.
                return Err(ScriptError::NoSuchField);
            };
            values.push(text_number(&text));
        }

        #[allow(clippy::cast_precision_loss)]
        let result = match op.as_str() {
            "SUM" => values.iter().sum::<f64>(),
            "AVG" => values.iter().sum::<f64>() / values.len() as f64,
            "PRD" => values.iter().product::<f64>(),
            "MIN" => values.iter().copied().fold(f64::INFINITY, f64::min),
            "MAX" => values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            _ => return Err(ScriptError::BadArgument),
        };
        if !result.is_finite() {
            return Err(ScriptError::NotStorable);
        }

        // The helper's whole effect is on the event, which is why a script
        // that calls it has no assignment in it.
        self.event_value = Value::Num(result);
        self.event_assigned = true;
        Ok(Value::Num(result))
    }

    /// The event's value as a number, tolerantly.
    ///
    /// A format action runs against a value that has often been through a
    /// format action before — a stored "1,234.50" is not what `parse` takes,
    /// and refusing it would make re-formatting a formatted field fail.
    fn event_number(&self) -> f64 {
        match &self.event_value {
            Value::Str(text) => text_number(text),
            other => to_number(other),
        }
    }

    fn number_format(&mut self, args: &[Value]) -> Result<Value, ScriptError> {
        let decimals = arg_int(args, 0, 2);
        let separator = arg_int(args, 1, 0);
        let negative = arg_int(args, 2, 0);
        let currency = args.get(4).map(to_text).unwrap_or_default();
        let prepend = args.get(5).is_none_or(truthy);

        let value = self.event_number();
        if !value.is_finite() {
            return Err(ScriptError::NotStorable);
        }
        let text = format_number(
            value,
            clamp_decimals(decimals),
            separator,
            negative,
            &currency,
            prepend,
        )?;
        self.event_value = Value::Str(text.clone());
        self.event_assigned = true;
        Ok(Value::Str(text))
    }

    fn percent_format(&mut self, args: &[Value]) -> Result<Value, ScriptError> {
        let decimals = arg_int(args, 0, 2);
        let separator = arg_int(args, 1, 0);

        let value = self.event_number() * 100.0;
        if !value.is_finite() {
            return Err(ScriptError::NotStorable);
        }
        let mut text = format_number(value, clamp_decimals(decimals), separator, 0, "", true)?;
        text.push('%');
        self.event_value = Value::Str(text.clone());
        self.event_assigned = true;
        Ok(Value::Str(text))
    }

    fn date_format(&mut self, args: &[Value]) -> Result<Value, ScriptError> {
        let index = arg_int(args, 0, 0);
        let text = to_text(&self.event_value);
        // A value that is not a date is left exactly as it is, which is what a
        // viewer shows: refusing would fail a whole calculation over a field
        // the user has not filled in yet.
        let Some(date) = parse_date(&text) else {
            return Ok(self.event_value.clone());
        };
        let formatted = format_date(date, index);
        self.event_value = Value::Str(formatted.clone());
        self.event_assigned = true;
        Ok(Value::Str(formatted))
    }

    fn special_format(&mut self, args: &[Value]) -> Result<Value, ScriptError> {
        let style = arg_int(args, 0, 0);
        let text = to_text(&self.event_value);
        let digits: String = text.chars().filter(char::is_ascii_digit).collect();
        let Some(formatted) = format_special(&digits, style) else {
            return Ok(self.event_value.clone());
        };
        self.event_value = Value::Str(formatted.clone());
        self.event_assigned = true;
        Ok(Value::Str(formatted))
    }
}

fn binary(op: BinOp, left: &Value, right: &Value) -> Result<Value, ScriptError> {
    if op == BinOp::Add {
        // 11.6.1: one string operand makes `+` concatenation.
        if matches!(left, Value::Str(_)) || matches!(right, Value::Str(_)) {
            let mut text = to_text(left);
            text.push_str(&to_text(right));
            if text.len() > limits::MAX_SCRIPT_STRING {
                return Err(ScriptError::StringTooLong);
            }
            return Ok(Value::Str(text));
        }
    }
    let (a, b) = (to_number(left), to_number(right));
    Ok(match op {
        BinOp::Add => Value::Num(a + b),
        BinOp::Sub => Value::Num(a - b),
        BinOp::Mul => Value::Num(a * b),
        BinOp::Div => Value::Num(a / b),
        BinOp::Rem => Value::Num(a % b),
        BinOp::Lt => Value::Bool(a < b),
        BinOp::Le => Value::Bool(a <= b),
        BinOp::Gt => Value::Bool(a > b),
        BinOp::Ge => Value::Bool(a >= b),
        BinOp::Eq => Value::Bool(loose_eq(left, right)),
        BinOp::Ne => Value::Bool(!loose_eq(left, right)),
    })
}

fn arg_int(args: &[Value], at: usize, default: i64) -> i64 {
    let Some(value) = args.get(at) else {
        return default;
    };
    let n = to_number(value);
    if !n.is_finite() {
        return default;
    }
    #[allow(clippy::cast_possible_truncation)]
    let n = n.trunc() as i64;
    n
}

fn clamp_decimals(decimals: i64) -> usize {
    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let clamped = decimals.clamp(0, 15) as usize;
    clamped
}

/// Rounds half away from zero, without a transcendental.
///
/// Ruling 4 permits `round`, which is correctly rounded on every target; the
/// power of ten is built by multiplication rather than `powi` so that this
/// file has no float call whose result could differ anywhere.
fn round_to(value: f64, decimals: usize) -> f64 {
    let mut scale = 1.0f64;
    for _ in 0..decimals {
        scale *= 10.0;
    }
    let scaled = value * scale;
    if !scaled.is_finite() {
        return value;
    }
    scaled.round() / scale
}

/// `AFNumber_Format`'s separator styles: which character groups thousands and
/// which marks the decimal.
fn separators(style: i64) -> (&'static str, &'static str) {
    match style {
        1 => ("", "."),
        2 => (".", ","),
        3 => ("", ","),
        _ => (",", "."),
    }
}

fn group_digits(digits: &str, separator: &str) -> String {
    if separator.is_empty() || digits.len() <= 3 {
        return digits.to_string();
    }
    let bytes = digits.as_bytes();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    let lead = bytes.len() % 3;
    if lead > 0 {
        out.push_str(&digits[..lead]);
    }
    let mut at = lead;
    while at < bytes.len() {
        if !out.is_empty() {
            out.push_str(separator);
        }
        out.push_str(&digits[at..at + 3]);
        at += 3;
    }
    out
}

fn format_number(
    value: f64,
    decimals: usize,
    separator_style: i64,
    negative_style: i64,
    currency: &str,
    prepend: bool,
) -> Result<String, ScriptError> {
    let rounded = round_to(value, decimals);
    if !rounded.is_finite() {
        return Err(ScriptError::NotStorable);
    }
    // The sign is taken from the rounded value, so -0.004 at two decimals is
    // "0.00" rather than "-0.00". Negative zero needs saying separately: it
    // compares equal to zero and is not less than it, so neither branch below
    // would have dropped the sign the formatter still prints.
    let negative = rounded < 0.0;
    let magnitude = if rounded == 0.0 {
        0.0
    } else if negative {
        -rounded
    } else {
        rounded
    };

    let plain = format!("{magnitude:.decimals$}");
    let (whole, fraction) = match plain.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (plain.as_str(), None),
    };
    let (thousands, point) = separators(separator_style);

    let mut body = group_digits(whole, thousands);
    if let Some(fraction) = fraction {
        body.push_str(point);
        body.push_str(fraction);
    }

    if !currency.is_empty() {
        if prepend {
            body.insert_str(0, currency);
        } else {
            body.push_str(currency);
        }
    }

    // 0 (minus, black) and 1 (minus, red) both keep the sign; 2 and 3 use
    // parentheses. The colour half of 1 and 3 is not modelled, because a
    // field value has no colour -- that lives in the appearance.
    Ok(match (negative, negative_style) {
        (false, _) => body,
        (true, 2 | 3) => format!("({body})"),
        (true, _) => format!("-{body}"),
    })
}

/// A calendar date, with no arithmetic performed on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Date {
    year: i32,
    month: u32,
    day: u32,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Two-digit years, Acrobat's rule: 00-49 is this century, 50-99 the last.
fn expand_year(year: i32, digits: usize) -> i32 {
    if digits > 2 {
        return year;
    }
    if year < 50 {
        2000 + year
    } else {
        1900 + year
    }
}

fn month_from_name(text: &str) -> Option<u32> {
    let lower = text.to_ascii_lowercase();
    let at = MONTHS
        .iter()
        .position(|m| lower.starts_with(&m.to_ascii_lowercase()))?;
    u32::try_from(at + 1).ok()
}

fn valid(year: i32, month: u32, day: u32) -> Option<Date> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || !(1..=9999).contains(&year) {
        return None;
    }
    Some(Date { year, month, day })
}

/// Reads the date forms a form field is likely to hold.
///
/// Deliberately few: `yyyy-mm-dd`, `m/d/yyyy`, `d-mmm-yyyy` and
/// `mmm d, yyyy`, with two-digit years. A value that is none of them is not a
/// date, and the caller leaves it alone rather than inventing one.
fn parse_date(text: &str) -> Option<Date> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let parts: Vec<&str> = text
        .split(['-', '/', '.', ' ', ','])
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() != 3 {
        return None;
    }

    let first = parts[0].parse::<i32>().ok();
    let second = parts[1].parse::<i32>().ok();
    let third = parts[2].parse::<i32>().ok();

    if let (Some(a), Some(b), Some(c)) = (first, second, third) {
        // A four-digit leading field is a year, which is the only way to tell
        // 2026-01-02 from 01/02/2026 without knowing a locale.
        return if parts[0].len() == 4 {
            valid(a, u32::try_from(b).ok()?, u32::try_from(c).ok()?)
        } else {
            valid(
                expand_year(c, parts[2].len()),
                u32::try_from(a).ok()?,
                u32::try_from(b).ok()?,
            )
        };
    }

    // d-mmm-yyyy.
    if let (Some(day), Some(month), Some(year)) = (first, month_from_name(parts[1]), third) {
        return valid(
            expand_year(year, parts[2].len()),
            month,
            u32::try_from(day).ok()?,
        );
    }

    // mmm d, yyyy.
    if let (Some(month), Some(day), Some(year)) = (month_from_name(parts[0]), second, third) {
        return valid(
            expand_year(year, parts[2].len()),
            month,
            u32::try_from(day).ok()?,
        );
    }

    None
}

/// `AFDate_Format`'s format table, by index.
fn format_date(date: Date, index: i64) -> String {
    let Date { year, month, day } = date;
    let name = MONTHS[(month.max(1) as usize - 1).min(11)];
    let short = year % 100;
    match index {
        0 => format!("{month}/{day}"),
        2 => format!("{month}/{day}/{year}"),
        3 => format!("{month:02}/{day:02}/{short:02}"),
        4 => format!("{month:02}/{day:02}/{year}"),
        5 => format!("{day}-{name}"),
        6 => format!("{day}-{name}-{short:02}"),
        7 => format!("{day}-{name}-{year}"),
        8 => format!("{day:02}-{name}-{short:02}"),
        9 => format!("{day:02}-{name}-{year}"),
        10 => format!("{short:02}-{month:02}-{day:02}"),
        11 => format!("{year}-{month:02}-{day:02}"),
        _ => format!("{month}/{day}/{short:02}"),
    }
}

/// `AFSpecial_Format`'s four fixed shapes. A digit count that does not fit the
/// shape leaves the value alone, because a half-formatted postcode is worse
/// than an unformatted one.
fn format_special(digits: &str, style: i64) -> Option<String> {
    match (style, digits.len()) {
        (0, 5) => Some(digits.to_string()),
        (1, 9) => Some(format!("{}-{}", &digits[..5], &digits[5..])),
        (2, 7) => Some(format!("{}-{}", &digits[..3], &digits[3..])),
        (2, 10) => Some(format!(
            "({}) {}-{}",
            &digits[..3],
            &digits[3..6],
            &digits[6..]
        )),
        (2, 11) if digits.starts_with('1') => Some(format!(
            "1 ({}) {}-{}",
            &digits[1..4],
            &digits[4..7],
            &digits[7..]
        )),
        (3, 9) => Some(format!(
            "{}-{}-{}",
            &digits[..3],
            &digits[3..5],
            &digits[5..]
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A form's worth of fields, with no PDF anywhere near it — which is the
    /// point of [`Host`] being two methods over strings.
    #[derive(Default)]
    struct Fields {
        values: Vec<(String, String)>,
        /// Names this host refuses to write, standing in for read-only,
        /// over-`/MaxLen` and not-an-option.
        refuse: Vec<String>,
    }

    impl Fields {
        fn with(pairs: &[(&str, &str)]) -> Fields {
            Fields {
                values: pairs
                    .iter()
                    .map(|(n, v)| ((*n).to_string(), (*v).to_string()))
                    .collect(),
                refuse: Vec::new(),
            }
        }

        fn get(&self, name: &str) -> Option<&str> {
            self.values
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str())
        }
    }

    impl Host for Fields {
        fn field(&self, name: &str) -> Option<String> {
            self.get(name).map(ToString::to_string)
        }

        fn set_field(&mut self, name: &str, value: &str) -> bool {
            if self.refuse.iter().any(|r| r == name) {
                return false;
            }
            match self.values.iter_mut().find(|(n, _)| n == name) {
                Some(slot) => slot.1 = value.to_string(),
                None => return false,
            }
            true
        }
    }

    /// Runs against a field set, with the ordinary per-script budget.
    fn run_on(source: &str, host: &mut Fields, current: &str) -> Result<Outcome, ScriptError> {
        let mut budget = Budget::new(limits::MAX_SCRIPT_STEPS);
        run(source, "total", current, host, &mut budget)
    }

    fn value_of(source: &str) -> Result<Option<String>, ScriptError> {
        let mut host = Fields::with(&[("a", "10"), ("b", "20"), ("c", "3"), ("total", "0")]);
        run_on(source, &mut host, "0").map(|outcome| outcome.value)
    }

    // -- arithmetic and expressions -------------------------------------

    #[test]
    fn arithmetic_follows_precedence() {
        assert_eq!(
            value_of("event.value = 1 + 2 * 3;").unwrap().as_deref(),
            Some("7")
        );
        assert_eq!(
            value_of("event.value = (1 + 2) * 3;").unwrap().as_deref(),
            Some("9")
        );
        assert_eq!(
            value_of("event.value = 10 / 4;").unwrap().as_deref(),
            Some("2.5")
        );
        assert_eq!(
            value_of("event.value = 10 % 3;").unwrap().as_deref(),
            Some("1")
        );
        assert_eq!(
            value_of("event.value = -3 + 1;").unwrap().as_deref(),
            Some("-2")
        );
    }

    /// The single most common shape after `AFSimple_Calculate`: a short
    /// expression over `getField(...).value`. A field holding digits has to
    /// read as a number, or this concatenates instead of adding.
    #[test]
    fn a_field_holding_digits_adds_rather_than_concatenates() {
        assert_eq!(
            value_of("event.value = getField(\"a\").value + getField(\"b\").value;")
                .unwrap()
                .as_deref(),
            Some("30")
        );
        assert_eq!(
            value_of("event.value = this.getField('a').value * 1.2;")
                .unwrap()
                .as_deref(),
            Some("12")
        );
    }

    #[test]
    fn a_string_operand_makes_plus_concatenate() {
        assert_eq!(
            value_of("event.value = \"n: \" + 1;").unwrap().as_deref(),
            Some("n: 1")
        );
        assert_eq!(
            value_of("event.value = getField('a').valueAsString + 'x';")
                .unwrap()
                .as_deref(),
            Some("10x")
        );
    }

    #[test]
    fn comparison_conditional_and_logic_work() {
        assert_eq!(
            value_of("event.value = 2 > 1 ? \"yes\" : \"no\";")
                .unwrap()
                .as_deref(),
            Some("yes")
        );
        assert_eq!(
            value_of("event.value = (1 > 2 || 3 >= 3) && !false;")
                .unwrap()
                .as_deref(),
            Some("true")
        );
        assert_eq!(
            value_of("event.value = 1 == \"1\";").unwrap().as_deref(),
            Some("true")
        );
    }

    #[test]
    fn statements_branch_and_declare() {
        let source = "var x = getField('a').value; \
                      if (x > 5) { event.value = 'big'; } else { event.value = 'small'; }";
        assert_eq!(value_of(source).unwrap().as_deref(), Some("big"));

        let source = "var total = 0; \
                      for (var i = 0; i < 4; i += 1) { total += i; } \
                      event.value = total;";
        assert_eq!(value_of(source).unwrap().as_deref(), Some("6"));
    }

    #[test]
    fn comments_are_skipped() {
        let source = "// a leading remark\n\
                      event.value = 1 /* inline */ + 1; // trailing";
        assert_eq!(value_of(source).unwrap().as_deref(), Some("2"));
    }

    /// A script that never touches `event.value` leaves the field alone. That
    /// is different from setting it to nothing, and a caller that confuses the
    /// two blanks every field a keystroke script touches.
    #[test]
    fn a_script_that_assigns_nothing_produces_no_value() {
        assert_eq!(value_of("var x = 1;").unwrap(), None);
    }

    #[test]
    fn the_event_names_its_own_field() {
        assert_eq!(
            value_of("event.value = event.target.name;")
                .unwrap()
                .as_deref(),
            Some("total")
        );
    }

    // -- AFSimple_Calculate ---------------------------------------------

    /// The five operations, over the same three fields, so that reading the
    /// operation wrongly changes the number rather than the shape.
    #[test]
    fn simple_calculate_does_each_operation() {
        let cases = [
            ("SUM", "33"),
            ("AVG", "11"),
            ("PRD", "600"),
            ("MIN", "3"),
            ("MAX", "20"),
        ];
        for (op, expected) in cases {
            let source = format!("AFSimple_Calculate(\"{op}\", [\"a\", \"b\", \"c\"]);");
            assert_eq!(
                value_of(&source).unwrap().as_deref(),
                Some(expected),
                "{op}"
            );
        }
    }

    /// Acrobat's own field dialogue writes `new Array(...)`, and other
    /// generators write a comma-separated string. Both have to work or the
    /// helper covers a fraction of the forms that use it.
    #[test]
    fn simple_calculate_takes_both_field_list_forms() {
        assert_eq!(
            value_of("AFSimple_Calculate(\"SUM\", new Array(\"a\", \"b\"));")
                .unwrap()
                .as_deref(),
            Some("30")
        );
        assert_eq!(
            value_of("AFSimple_Calculate(\"sum\", \"a, b\");")
                .unwrap()
                .as_deref(),
            Some("30")
        );
    }

    /// A sum missing an addend is a wrong total, not a smaller one.
    #[test]
    fn simple_calculate_refuses_a_field_that_is_not_there() {
        assert_eq!(
            value_of("AFSimple_Calculate(\"SUM\", [\"a\", \"gone\"]);"),
            Err(ScriptError::NoSuchField)
        );
    }

    #[test]
    fn simple_calculate_refuses_an_operation_it_does_not_know() {
        assert_eq!(
            value_of("AFSimple_Calculate(\"MEDIAN\", [\"a\"]);"),
            Err(ScriptError::BadArgument)
        );
        assert_eq!(
            value_of("AFSimple_Calculate(\"SUM\", []);"),
            Err(ScriptError::BadArgument)
        );
    }

    /// A blank or non-numeric field counts as zero, which is what makes an
    /// untouched form compute its first total instead of refusing.
    #[test]
    fn simple_calculate_reads_an_empty_field_as_zero() {
        let mut host = Fields::with(&[("a", ""), ("b", "20"), ("total", "0")]);
        let outcome = run_on("AFSimple_Calculate('SUM', ['a','b']);", &mut host, "0").unwrap();
        assert_eq!(outcome.value.as_deref(), Some("20"));
    }

    // -- the format helpers ---------------------------------------------

    #[test]
    fn number_format_places_separators_and_currency() {
        let cases = [
            ("AFNumber_Format(2, 0, 0, 0, \"\", true);", "1,234.50"),
            ("AFNumber_Format(0, 0, 0, 0, \"\", true);", "1,235"),
            ("AFNumber_Format(2, 1, 0, 0, \"\", true);", "1234.50"),
            ("AFNumber_Format(2, 2, 0, 0, \"\", true);", "1.234,50"),
            ("AFNumber_Format(2, 3, 0, 0, \"\", true);", "1234,50"),
            (
                "AFNumber_Format(2, 0, 0, 0, \"GBP \", true);",
                "GBP 1,234.50",
            ),
            (
                "AFNumber_Format(2, 0, 0, 0, \" kr\", false);",
                "1,234.50 kr",
            ),
        ];
        for (source, expected) in cases {
            let mut host = Fields::default();
            let outcome = run_on(source, &mut host, "1234.5").unwrap();
            assert_eq!(outcome.value.as_deref(), Some(expected), "{source}");
        }
    }

    #[test]
    fn number_format_styles_a_negative_value() {
        for (style, expected) in [(0, "-1,234.50"), (1, "-1,234.50"), (2, "(1,234.50)")] {
            let source = format!("AFNumber_Format(2, 0, {style}, 0, \"\", true);");
            let mut host = Fields::default();
            let outcome = run_on(&source, &mut host, "-1234.5").unwrap();
            assert_eq!(outcome.value.as_deref(), Some(expected), "{style}");
        }
    }

    /// A value rounding to zero must not keep the sign it had: "-0.00" in a
    /// total is a bug report.
    #[test]
    fn a_value_rounding_to_zero_loses_its_sign() {
        let mut host = Fields::default();
        let outcome = run_on(
            "AFNumber_Format(2, 0, 0, 0, \"\", true);",
            &mut host,
            "-0.001",
        )
        .unwrap();
        assert_eq!(outcome.value.as_deref(), Some("0.00"));
    }

    /// A formatted value fed back through the formatter has to survive, or a
    /// field cannot be reformatted after it has been formatted once.
    #[test]
    fn number_format_reads_a_value_it_wrote() {
        let mut host = Fields::default();
        let outcome = run_on(
            "AFNumber_Format(2, 0, 0, 0, \"\", true);",
            &mut host,
            "1,234.50",
        )
        .unwrap();
        assert_eq!(outcome.value.as_deref(), Some("1,234.50"));
    }

    #[test]
    fn percent_format_scales_by_a_hundred() {
        let mut host = Fields::default();
        let outcome = run_on("AFPercent_Format(1, 0);", &mut host, "0.075").unwrap();
        assert_eq!(outcome.value.as_deref(), Some("7.5%"));
    }

    #[test]
    fn date_format_uses_the_index_table() {
        let cases = [
            (2, "8/14/2026"),
            (4, "08/14/2026"),
            (9, "14-Aug-2026"),
            (11, "2026-08-14"),
        ];
        for (index, expected) in cases {
            let source = format!("AFDate_Format({index});");
            let mut host = Fields::default();
            let outcome = run_on(&source, &mut host, "2026-08-14").unwrap();
            assert_eq!(outcome.value.as_deref(), Some(expected), "{index}");
        }
    }

    #[test]
    fn date_format_reads_the_forms_a_field_holds() {
        for text in ["2026-08-14", "8/14/2026", "14-Aug-2026", "Aug 14, 2026"] {
            let mut host = Fields::default();
            let outcome = run_on("AFDate_Format(11);", &mut host, text).unwrap();
            assert_eq!(outcome.value.as_deref(), Some("2026-08-14"), "{text}");
        }
    }

    /// A value that is not a date is left exactly as it was, because a field
    /// the user has not filled in yet must not fail a whole pass.
    #[test]
    fn date_format_leaves_what_is_not_a_date_alone() {
        let mut host = Fields::default();
        let outcome = run_on("AFDate_Format(4);", &mut host, "not a date").unwrap();
        assert_eq!(
            outcome.value, None,
            "nothing was assigned, so nothing changes"
        );
    }

    #[test]
    fn special_format_shapes_the_four_it_knows() {
        let cases = [
            (0, "12345", "12345"),
            (1, "123456789", "12345-6789"),
            (2, "5551234", "555-1234"),
            (2, "5551234567", "(555) 123-4567"),
            (3, "123456789", "123-45-6789"),
        ];
        for (style, input, expected) in cases {
            let source = format!("AFSpecial_Format({style});");
            let mut host = Fields::default();
            let outcome = run_on(&source, &mut host, input).unwrap();
            assert_eq!(outcome.value.as_deref(), Some(expected), "{style} {input}");
        }
    }

    /// A digit count that does not fit leaves the value alone: a
    /// half-formatted postcode is worse than an unformatted one.
    #[test]
    fn special_format_leaves_a_wrong_length_alone() {
        let mut host = Fields::default();
        let outcome = run_on("AFSpecial_Format(0);", &mut host, "123").unwrap();
        assert_eq!(outcome.value, None);
    }

    // -- writing other fields -------------------------------------------

    #[test]
    fn a_script_can_write_another_field() {
        let mut host = Fields::with(&[("a", "1"), ("b", "2"), ("total", "0")]);
        let outcome = run_on("getField('b').value = 9;", &mut host, "0").unwrap();
        assert_eq!(outcome.wrote, vec!["b".to_string()]);
        assert_eq!(host.get("b"), Some("9"));
    }

    #[test]
    fn a_refused_write_stops_the_script() {
        let mut host = Fields::with(&[("b", "2")]);
        host.refuse.push("b".to_string());
        assert_eq!(
            run_on("getField('b').value = 9;", &mut host, "0"),
            Err(ScriptError::FieldRefused)
        );
    }

    #[test]
    fn getfield_on_a_missing_name_is_null_until_it_is_used() {
        assert_eq!(
            value_of("event.value = getField('gone') == null ? 'none' : 'there';")
                .unwrap()
                .as_deref(),
            Some("none")
        );
        assert_eq!(
            value_of("event.value = getField('gone').value;"),
            Err(ScriptError::NoSuchField)
        );
    }

    // -- the boundary ----------------------------------------------------

    /// `while (true) {}` is depth one and costs everything, which is exactly
    /// why a depth cap is not a work cap.
    #[test]
    fn a_script_that_does_not_terminate_runs_out_of_steps() {
        assert_eq!(value_of("while (true) { }"), Err(ScriptError::OutOfSteps));
        assert_eq!(
            value_of("for (;;) { event.value = 1; }"),
            Err(ScriptError::OutOfSteps)
        );
        assert_eq!(
            value_of("var i = 0; while (i < 1000000) { i += 1; }"),
            Err(ScriptError::OutOfSteps)
        );
    }

    /// Doubling a string is cheap in steps and ruinous in bytes, so the length
    /// cap is a separate bound rather than a consequence of the step one.
    #[test]
    fn a_script_that_doubles_a_string_hits_the_length_cap() {
        let source = "var s = 'xxxxxxxx'; while (true) { s = s + s; } event.value = s;";
        assert_eq!(value_of(source), Err(ScriptError::StringTooLong));
    }

    #[test]
    fn nesting_past_the_cap_is_refused() {
        let depth = limits::MAX_SCRIPT_DEPTH as usize + 4;
        let source = format!("event.value = {}1{};", "(".repeat(depth), ")".repeat(depth));
        assert_eq!(value_of(&source), Err(ScriptError::TooDeep));

        // And ordinary nesting is not: three levels of brackets is a normal
        // expression, and a cap that refused it would refuse real forms.
        assert_eq!(
            value_of("event.value = ((1 + 2) * (3 - 1)) / 2;")
                .unwrap()
                .as_deref(),
            Some("3")
        );
    }

    #[test]
    fn source_past_the_length_cap_is_refused() {
        let source = format!("event.value = 1; {}", "/* pad */".repeat(20_000));
        assert_eq!(value_of(&source), Err(ScriptError::TooLong));
    }

    /// The excluded constructs, each by name. `eval` is reserved rather than
    /// merely undefined, because an interpreter that treats it as an unknown
    /// name is one rename away from running it.
    #[test]
    fn the_constructs_outside_the_subset_are_refused() {
        for source in [
            "eval('1+1');",
            "function f() { return 1; }",
            "try { event.value = 1; } catch (e) { }",
            "switch (1) { }",
            "for (var k in event) { }",
            "typeof event;",
            "delete event.value;",
            "new Date();",
        ] {
            assert_eq!(value_of(source), Err(ScriptError::Syntax), "{source}");
        }
    }

    #[test]
    fn an_unknown_name_or_member_is_refused() {
        assert_eq!(
            value_of("event.value = util.printf('%d', 1);"),
            Err(ScriptError::UnknownName)
        );
        assert_eq!(
            value_of("event.value = event.mystery;"),
            Err(ScriptError::UnknownMember)
        );
        assert_eq!(
            value_of("event.value = getField('a').mystery;"),
            Err(ScriptError::UnknownMember)
        );
    }

    /// `app.*` is an inert stub, so a script that ends by alerting still
    /// calculates -- and the stub is genuinely inert rather than merely
    /// intended to be.
    #[test]
    fn app_is_inert_and_does_not_stop_a_calculation() {
        let source = "event.value = 42; app.alert('done'); console.println('x');";
        assert_eq!(value_of(source).unwrap().as_deref(), Some("42"));
    }

    /// What an inert stub must never do is supply a value. `undefined` in a
    /// total is the document that lies this whole feature exists to avoid.
    #[test]
    fn an_inert_stubs_result_cannot_become_a_value() {
        assert_eq!(
            value_of("event.value = app.response('hello');"),
            Err(ScriptError::NotStorable)
        );
        assert_eq!(
            value_of("event.value = 1 / 0;"),
            Err(ScriptError::NotStorable)
        );
        assert_eq!(
            value_of("event.value = getField('a').value * undefined;"),
            Err(ScriptError::NotStorable)
        );
    }

    #[test]
    fn assignment_to_something_that_is_not_a_place_is_refused() {
        assert_eq!(value_of("1 = 2;"), Err(ScriptError::NotAssignable));
        assert_eq!(value_of("app.thing = 2;"), Err(ScriptError::NotAssignable));
    }

    #[test]
    fn an_unterminated_string_is_refused_rather_than_closed() {
        assert_eq!(value_of("event.value = 'abc;"), Err(ScriptError::BadToken));
    }

    #[test]
    fn a_character_with_no_token_is_refused() {
        assert_eq!(value_of("event.value = 1 @ 2;"), Err(ScriptError::BadToken));
    }

    /// Ruling 1, over the whole grammar: arbitrary text is an error, never a
    /// panic. The fuzz target is the exhaustive form of this; these are the
    /// shapes that have historically broken hand-rolled parsers.
    #[test]
    fn hostile_source_never_panics() {
        let sources = [
            "",
            "\u{0}",
            "((((",
            "))))",
            "var",
            "var = ;",
            "if",
            "if (",
            "if () {",
            "while",
            "for (",
            "for (;;",
            "1e",
            "1e+",
            ".5.5",
            "'\\u",
            "'\\x1",
            "'\\'",
            "/*",
            "//",
            "a.",
            "a[",
            "a(",
            "?:",
            "= 1",
            "event.",
            "event.value =",
            "new",
            "new Array",
            "[,,,]",
            "{{{{",
            "\u{1f600}",
        ];
        for source in sources {
            let mut host = Fields::default();
            let _ = run_on(source, &mut host, "0");
        }
    }

    #[test]
    fn a_budget_shared_across_runs_bounds_the_total() {
        let mut host = Fields::with(&[("a", "1")]);
        let mut budget = Budget::new(12);
        // The first short script fits; a second longer one does not, because
        // the budget is the pair's rather than each one's.
        assert!(run("event.value = 1;", "a", "0", &mut host, &mut budget).is_ok());
        assert_eq!(
            run(
                "event.value = 1 + 1 + 1 + 1;",
                "a",
                "0",
                &mut host,
                &mut budget
            ),
            Err(ScriptError::OutOfSteps)
        );
        assert!(budget.is_spent());
    }
}
