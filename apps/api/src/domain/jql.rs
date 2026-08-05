//! A small Jira-style query language for tasks: parse text like
//!
//! ```text
//! project = SPR AND status IN (todo, in_progress) AND assignee = currentUser()
//!   AND due <= 7d ORDER BY priority ASC, updated DESC
//! ```
//!
//! into an AST, then compile the AST to a parameterised SQL `WHERE` fragment.
//!
//! Two rules shape the whole module:
//!
//!   1. **No string interpolation of user values, ever.** Every literal becomes
//!      a bound parameter (`Param`), so a query is data, not SQL. Field names
//!      and sort keys come from a fixed allowlist — an unknown field is a parse
//!      error, never a column reference.
//!   2. **Errors point at the offender.** A parse failure carries the byte
//!      offset and the token we choked on, so the UI can say *what* it didn't
//!      understand instead of "invalid query".
//!
//! Project scoping is *not* expressed here: the caller always ANDs in the set
//! of projects the user may see. A query can narrow that, never widen it.

use chrono::{Duration, NaiveDate, Utc};

// ── tokens ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    /// Bare word: field name, keyword, or unquoted value.
    Word(String),
    /// Quoted string — always a value, never a keyword.
    Str(String),
    LParen,
    RParen,
    Comma,
    Op(String),
}

#[derive(Debug, Clone, PartialEq)]
struct Spanned {
    tok: Tok,
    at: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    /// Byte offset into the original query where the problem starts.
    pub at: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at character {})", self.message, self.at + 1)
    }
}

fn err<T>(message: impl Into<String>, at: usize) -> Result<T, ParseError> {
    Err(ParseError {
        message: message.into(),
        at,
    })
}

fn lex(src: &str) -> Result<Vec<Spanned>, ParseError> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let at = i;
        match c {
            '(' => {
                out.push(Spanned {
                    tok: Tok::LParen,
                    at,
                });
                i += 1;
            }
            ')' => {
                out.push(Spanned {
                    tok: Tok::RParen,
                    at,
                });
                i += 1;
            }
            ',' => {
                out.push(Spanned {
                    tok: Tok::Comma,
                    at,
                });
                i += 1;
            }
            '"' | '\'' => {
                let quote = c;
                i += 1;
                let mut s = String::new();
                loop {
                    if i >= b.len() {
                        return err("unterminated quoted value", at);
                    }
                    let ch = b[i] as char;
                    if ch == '\\' && i + 1 < b.len() {
                        s.push(b[i + 1] as char);
                        i += 2;
                        continue;
                    }
                    if ch == quote {
                        i += 1;
                        break;
                    }
                    // Push the raw byte range so multi-byte UTF-8 survives.
                    let start = i;
                    let mut end = i + 1;
                    while end < b.len() && (b[end] & 0b1100_0000) == 0b1000_0000 {
                        end += 1;
                    }
                    s.push_str(&src[start..end]);
                    i = end;
                }
                out.push(Spanned {
                    tok: Tok::Str(s),
                    at,
                });
            }
            '=' | '!' | '<' | '>' | '~' => {
                let two = src.get(i..i + 2).unwrap_or("");
                let op = match two {
                    "!=" | "<=" | ">=" | "!~" => {
                        i += 2;
                        two.to_string()
                    }
                    _ => {
                        i += 1;
                        c.to_string()
                    }
                };
                if op == "!" {
                    return err("expected != here", at);
                }
                out.push(Spanned {
                    tok: Tok::Op(op),
                    at,
                });
            }
            _ => {
                // A bare word: letters, digits, and the punctuation that shows
                // up inside real values — keys (SPR-12), handles (a.b_c),
                // relative dates (-7d), decimals, dates (2026-08-04).
                let start = i;
                while i < b.len() {
                    let ch = b[i] as char;
                    let ok = ch.is_alphanumeric()
                        || matches!(ch, '-' | '_' | '.' | '@' | '+' | '*' | '/' | ':' | '#')
                        || !ch.is_ascii();
                    if !ok {
                        break;
                    }
                    i += 1;
                }
                if i == start {
                    return err(format!("unexpected character `{c}`"), at);
                }
                // A trailing `()` belongs to the word: `currentUser()` and
                // `now()` are single values, not a name followed by an empty
                // group. Nothing else in the grammar puts `(` right after a
                // word, so this can't swallow a real group.
                if src.get(i..i + 2) == Some("()") {
                    i += 2;
                }
                out.push(Spanned {
                    tok: Tok::Word(src[start..i].to_string()),
                    at,
                });
            }
        }
    }
    Ok(out)
}

// ── AST ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    And(Box<Expr>, Box<Expr>),
    Or(Box<Expr>, Box<Expr>),
    Not(Box<Expr>),
    Cond(Cond),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cond {
    field: Field,
    op: CmpOp,
    value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Like,
    NotLike,
    Gt,
    Gte,
    Lt,
    Lte,
    In,
    NotIn,
    IsEmpty,
    IsNotEmpty,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Text(String),
    List(Vec<String>),
    Num(f64),
    Date(NaiveDate),
    /// `currentUser()` — resolved against the caller at compile time.
    CurrentUser,
    /// `is (not) empty` takes no value.
    None,
}

/// Bound parameters, in placeholder order. Compiled SQL only ever references
/// values through these.
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Text(String),
    TextList(Vec<String>),
    Num(f64),
    Date(NaiveDate),
}

// ── fields ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Key,
    Project,
    Title,
    Description,
    Text,
    Status,
    Priority,
    Type,
    Assignee,
    Reporter,
    Label,
    Sprint,
    Epic,
    Parent,
    Points,
    Estimate,
    Due,
    Created,
    Updated,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Case-insensitive text.
    Text,
    /// `text[]` column — membership rather than equality.
    Array,
    Number,
    Date,
    Timestamp,
    /// Full-text over title + description.
    FullText,
}

impl Field {
    fn parse(word: &str) -> Option<Field> {
        // Aliases are deliberate: people type what they see in the UI, and
        // Jira habits ("summary", "duedate") are worth honouring.
        Some(match word.to_ascii_lowercase().as_str() {
            "key" | "id" => Field::Key,
            "project" => Field::Project,
            "title" | "summary" => Field::Title,
            "description" | "body" => Field::Description,
            "text" => Field::Text,
            "status" | "state" => Field::Status,
            "priority" => Field::Priority,
            "type" | "issuetype" => Field::Type,
            "assignee" | "owner" => Field::Assignee,
            "reporter" | "creator" => Field::Reporter,
            "label" | "labels" => Field::Label,
            "sprint" => Field::Sprint,
            "epic" => Field::Epic,
            "parent" => Field::Parent,
            "points" | "storypoints" | "story_points" => Field::Points,
            "estimate" | "estimate_minutes" => Field::Estimate,
            "due" | "duedate" | "due_date" => Field::Due,
            "created" => Field::Created,
            "updated" => Field::Updated,
            "completed" | "resolved" => Field::Completed,
            _ => return None,
        })
    }

    /// The SQL the field maps to. Fixed strings — never user input.
    fn sql(self) -> &'static str {
        match self {
            Field::Key => "t.key",
            Field::Project => "p.key",
            Field::Title => "t.title",
            Field::Description => "t.description",
            Field::Text => "t.title || ' ' || t.description",
            Field::Status => "t.status",
            Field::Priority => "t.priority",
            Field::Type => "t.type",
            Field::Assignee => "ua.handle",
            Field::Reporter => "ur.handle",
            Field::Label => "t.labels",
            Field::Sprint => "s.name",
            Field::Epic => "e.name",
            Field::Parent => "pt.key",
            Field::Points => "t.story_points",
            Field::Estimate => "t.estimate_minutes",
            Field::Due => "t.due_date",
            Field::Created => "t.created_at",
            Field::Updated => "t.updated_at",
            Field::Completed => "t.completed_at",
        }
    }

    fn kind(self) -> Kind {
        match self {
            Field::Label => Kind::Array,
            Field::Points | Field::Estimate => Kind::Number,
            Field::Due => Kind::Date,
            Field::Created | Field::Updated | Field::Completed => Kind::Timestamp,
            Field::Text | Field::Description => Kind::FullText,
            _ => Kind::Text,
        }
    }

    /// Canonical name, for echoing back in errors and for ORDER BY labels.
    pub fn name(self) -> &'static str {
        match self {
            Field::Key => "key",
            Field::Project => "project",
            Field::Title => "title",
            Field::Description => "description",
            Field::Text => "text",
            Field::Status => "status",
            Field::Priority => "priority",
            Field::Type => "type",
            Field::Assignee => "assignee",
            Field::Reporter => "reporter",
            Field::Label => "label",
            Field::Sprint => "sprint",
            Field::Epic => "epic",
            Field::Parent => "parent",
            Field::Points => "points",
            Field::Estimate => "estimate",
            Field::Due => "due",
            Field::Created => "created",
            Field::Updated => "updated",
            Field::Completed => "completed",
        }
    }
}

// ── parser ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct OrderBy {
    pub field: Field,
    pub desc: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// `None` when the query is only an ORDER BY (or empty) — matches everything.
    pub filter: Option<Expr>,
    pub order: Vec<OrderBy>,
}

struct Parser {
    toks: Vec<Spanned>,
    pos: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Spanned> {
        self.toks.get(self.pos)
    }

    fn at(&self) -> usize {
        self.toks.get(self.pos).map(|t| t.at).unwrap_or(self.end)
    }

    fn word_is(&self, kw: &str) -> bool {
        matches!(self.peek(), Some(Spanned { tok: Tok::Word(w), .. }) if w.eq_ignore_ascii_case(kw))
    }

    fn bump(&mut self) -> Option<Spanned> {
        let t = self.toks.get(self.pos).cloned();
        if t.is_some() {
            self.pos += 1;
        }
        t
    }

    /// expr := and_expr (OR and_expr)*  — OR binds loosest.
    fn expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.and_expr()?;
        while self.word_is("or") {
            self.bump();
            let right = self.and_expr()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, ParseError> {
        let mut left = self.term()?;
        while self.word_is("and") {
            self.bump();
            let right = self.term()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn term(&mut self) -> Result<Expr, ParseError> {
        if self.word_is("not") {
            self.bump();
            return Ok(Expr::Not(Box::new(self.term()?)));
        }
        if matches!(self.peek().map(|s| &s.tok), Some(Tok::LParen)) {
            let open = self.at();
            self.bump();
            let inner = self.expr()?;
            match self.peek().map(|s| &s.tok) {
                Some(Tok::RParen) => {
                    self.bump();
                    Ok(inner)
                }
                _ => err("unclosed `(`", open),
            }
        } else {
            Ok(Expr::Cond(self.cond()?))
        }
    }

    fn cond(&mut self) -> Result<Cond, ParseError> {
        let at = self.at();
        let word = match self.bump() {
            Some(Spanned {
                tok: Tok::Word(w), ..
            }) => w,
            Some(Spanned {
                tok: Tok::Str(_),
                at,
            }) => {
                return err(
                    "a quoted string can't start a condition — put the field name first",
                    at,
                )
            }
            Some(s) => return err("expected a field name here", s.at),
            None => return err("query ended early — expected a field name", at),
        };
        let field = match Field::parse(&word) {
            Some(f) => f,
            None => return err(format!("unknown field `{word}`"), at),
        };

        // `is empty` / `is not empty`
        if self.word_is("is") {
            self.bump();
            let negate = if self.word_is("not") {
                self.bump();
                true
            } else {
                false
            };
            let at_kw = self.at();
            let ok = self.word_is("empty") || self.word_is("null");
            if !ok {
                return err("expected `empty` after `is`", at_kw);
            }
            self.bump();
            return Ok(Cond {
                field,
                op: if negate {
                    CmpOp::IsNotEmpty
                } else {
                    CmpOp::IsEmpty
                },
                value: Value::None,
            });
        }

        // `in (…)` / `not in (…)`
        if self.word_is("in") || self.word_is("not") {
            let negate = self.word_is("not");
            if negate {
                self.bump();
                if !self.word_is("in") {
                    return err("expected `in` after `not`", self.at());
                }
            }
            self.bump();
            let items = self.value_list()?;
            return Ok(Cond {
                field,
                op: if negate { CmpOp::NotIn } else { CmpOp::In },
                value: Value::List(items),
            });
        }

        let op_at = self.at();
        let op = match self.bump() {
            Some(Spanned {
                tok: Tok::Op(o), ..
            }) => match o.as_str() {
                "=" => CmpOp::Eq,
                "!=" => CmpOp::Ne,
                "~" => CmpOp::Like,
                "!~" => CmpOp::NotLike,
                ">" => CmpOp::Gt,
                ">=" => CmpOp::Gte,
                "<" => CmpOp::Lt,
                "<=" => CmpOp::Lte,
                other => return err(format!("unknown operator `{other}`"), op_at),
            },
            _ => {
                return err(
                    format!(
                    "expected an operator after `{}` — one of = != ~ > >= < <= in, or `is empty`",
                    field.name()
                ),
                    op_at,
                )
            }
        };

        let value = self.single_value(field)?;
        Ok(Cond { field, op, value })
    }

    fn value_list(&mut self) -> Result<Vec<String>, ParseError> {
        let open = self.at();
        if !matches!(self.peek().map(|s| &s.tok), Some(Tok::LParen)) {
            return err("expected `(` to open the list", open);
        }
        self.bump();
        let mut items = Vec::new();
        loop {
            match self.bump() {
                Some(Spanned {
                    tok: Tok::Word(w), ..
                }) => items.push(w),
                Some(Spanned {
                    tok: Tok::Str(s), ..
                }) => items.push(s),
                Some(Spanned {
                    tok: Tok::RParen,
                    at,
                }) => {
                    if items.is_empty() {
                        return err("an empty list matches nothing — drop the condition", at);
                    }
                    break;
                }
                Some(s) => return err("expected a value in the list", s.at),
                None => return err("unclosed list — expected `)`", open),
            }
            match self.peek().map(|s| &s.tok) {
                Some(Tok::Comma) => {
                    self.bump();
                }
                Some(Tok::RParen) => {
                    self.bump();
                    break;
                }
                _ => return err("expected `,` or `)` in the list", self.at()),
            }
        }
        Ok(items)
    }

    fn single_value(&mut self, field: Field) -> Result<Value, ParseError> {
        let at = self.at();
        let raw = match self.bump() {
            Some(Spanned {
                tok: Tok::Word(w), ..
            }) => w,
            Some(Spanned {
                tok: Tok::Str(s), ..
            }) => return coerce(field, &s, true).map_err(|m| ParseError { message: m, at }),
            Some(s) => return err("expected a value", s.at),
            None => return err("query ended early — expected a value", at),
        };
        coerce(field, &raw, false).map_err(|m| ParseError { message: m, at })
    }

    fn order_by(&mut self) -> Result<Vec<OrderBy>, ParseError> {
        let mut out = Vec::new();
        loop {
            let at = self.at();
            let word = match self.bump() {
                Some(Spanned {
                    tok: Tok::Word(w), ..
                }) => w,
                _ => return err("expected a field name to sort by", at),
            };
            let field = match Field::parse(&word) {
                Some(f) => f,
                None => return err(format!("can't sort by unknown field `{word}`"), at),
            };
            if matches!(field.kind(), Kind::FullText | Kind::Array) {
                return err(format!("`{}` isn't sortable", field.name()), at);
            }
            let mut desc = false;
            if self.word_is("desc") {
                self.bump();
                desc = true;
            } else if self.word_is("asc") {
                self.bump();
            }
            out.push(OrderBy { field, desc });
            if matches!(self.peek().map(|s| &s.tok), Some(Tok::Comma)) {
                self.bump();
                continue;
            }
            break;
        }
        Ok(out)
    }
}

/// Turn a raw literal into a typed value, checking it against the field.
fn coerce(field: Field, raw: &str, quoted: bool) -> Result<Value, String> {
    if !quoted && raw.eq_ignore_ascii_case("currentUser()") {
        return match field {
            Field::Assignee | Field::Reporter => Ok(Value::CurrentUser),
            _ => Err(format!(
                "`currentUser()` only makes sense for assignee or reporter, not `{}`",
                field.name()
            )),
        };
    }
    if !quoted && (raw.eq_ignore_ascii_case("empty") || raw.eq_ignore_ascii_case("null")) {
        // `assignee = empty` is such a common reflex that treating it as
        // `is empty` is kinder than an error.
        return Ok(Value::None);
    }
    match field.kind() {
        Kind::Number => raw
            .parse::<f64>()
            .map(Value::Num)
            .map_err(|_| format!("`{}` needs a number, got `{raw}`", field.name())),
        Kind::Date | Kind::Timestamp => parse_date(raw).map(Value::Date).ok_or_else(|| {
            format!(
                "`{}` needs a date — try 2026-08-04, today, now, or a relative offset like -7d",
                field.name()
            )
        }),
        _ => Ok(Value::Text(raw.to_string())),
    }
}

/// Dates: ISO, `today`/`now`, or a signed offset in days/weeks/months
/// (`-7d`, `2w`, `-1m`). Anchored on today in UTC — good enough for filters,
/// and it keeps the query text portable between people.
fn parse_date(raw: &str) -> Option<NaiveDate> {
    let today = Utc::now().date_naive();
    let lower = raw.to_ascii_lowercase();
    if lower == "today" || lower == "now" || lower == "now()" {
        return Some(today);
    }
    if lower == "tomorrow" {
        return Some(today + Duration::days(1));
    }
    if lower == "yesterday" {
        return Some(today - Duration::days(1));
    }
    if let Ok(d) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") {
        return Some(d);
    }
    let bytes = lower.as_bytes();
    if bytes.len() >= 2 {
        let unit = *bytes.last().unwrap() as char;
        let num: String = lower[..lower.len() - 1].to_string();
        if let Ok(n) = num.parse::<i64>() {
            let days = match unit {
                'd' => n,
                'w' => n * 7,
                'm' => n * 30,
                'y' => n * 365,
                _ => return None,
            };
            return Some(today + Duration::days(days));
        }
    }
    None
}

/// Parse a whole query: an optional filter, an optional `ORDER BY`.
pub fn parse(src: &str) -> Result<Query, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser {
        toks,
        pos: 0,
        end: src.len(),
    };

    // ORDER BY only, or nothing at all.
    if p.peek().is_none() {
        return Ok(Query {
            filter: None,
            order: Vec::new(),
        });
    }
    let filter = if p.word_is("order") {
        None
    } else {
        Some(p.expr()?)
    };

    let mut order = Vec::new();
    if p.word_is("order") {
        p.bump();
        if !p.word_is("by") {
            return err("expected `by` after `order`", p.at());
        }
        p.bump();
        order = p.order_by()?;
    }
    if let Some(s) = p.peek() {
        let what = match &s.tok {
            Tok::Word(w) => format!("`{w}`"),
            Tok::Str(_) => "a quoted string".into(),
            Tok::Op(o) => format!("`{o}`"),
            Tok::LParen => "`(`".into(),
            Tok::RParen => "`)`".into(),
            Tok::Comma => "`,`".into(),
        };
        return err(
            format!("didn't expect {what} here — missing an `AND` or `OR`?"),
            s.at,
        );
    }
    Ok(Query { filter, order })
}

// ── compile to SQL ───────────────────────────────────────────────────────

pub struct Compiled {
    /// A boolean SQL expression over the aliases t/p/ua/ur/s/e/pt. Empty query
    /// compiles to "true".
    pub where_sql: String,
    /// `ORDER BY …` without the keywords, or empty for the caller's default.
    pub order_sql: String,
    pub params: Vec<Param>,
}

/// Compile a parsed query. `current_user_handle` resolves `currentUser()`;
/// `first_param` is the 1-based index of the first placeholder this fragment
/// may use (the caller has already bound its project scope).
pub fn compile(q: &Query, current_user_handle: &str, first_param: usize) -> Compiled {
    let mut c = Ctx {
        params: Vec::new(),
        next: first_param,
        me: current_user_handle.to_string(),
    };
    let where_sql = match &q.filter {
        Some(e) => compile_expr(e, &mut c),
        None => "true".to_string(),
    };
    let order_sql = q
        .order
        .iter()
        .map(|o| {
            format!(
                "{} {} NULLS LAST",
                o.field.sql(),
                if o.desc { "DESC" } else { "ASC" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    Compiled {
        where_sql,
        order_sql,
        params: c.params,
    }
}

struct Ctx {
    params: Vec<Param>,
    next: usize,
    me: String,
}

impl Ctx {
    fn bind(&mut self, p: Param) -> String {
        self.params.push(p);
        let ph = format!("${}", self.next);
        self.next += 1;
        ph
    }
}

fn compile_expr(e: &Expr, c: &mut Ctx) -> String {
    match e {
        Expr::And(a, b) => format!("({} AND {})", compile_expr(a, c), compile_expr(b, c)),
        Expr::Or(a, b) => format!("({} OR {})", compile_expr(a, c), compile_expr(b, c)),
        Expr::Not(a) => format!("(NOT {})", compile_expr(a, c)),
        Expr::Cond(cond) => compile_cond(cond, c),
    }
}

fn compile_cond(cond: &Cond, c: &mut Ctx) -> String {
    let col = cond.field.sql();
    let kind = cond.field.kind();

    // `field = empty` was normalised to Value::None by the parser; treat it as
    // the emptiness test regardless of which operator got us here.
    if matches!(cond.value, Value::None) {
        let empty = match kind {
            Kind::Array => format!("COALESCE(cardinality({col}), 0) = 0"),
            _ => format!("{col} IS NULL"),
        };
        let negate = matches!(
            cond.op,
            CmpOp::IsNotEmpty | CmpOp::Ne | CmpOp::NotIn | CmpOp::NotLike
        );
        return if negate {
            format!("(NOT ({empty}))")
        } else {
            format!("({empty})")
        };
    }

    match (kind, &cond.value) {
        // ── text[] ───────────────────────────────────────────────────────
        (Kind::Array, Value::Text(v)) => {
            let ph = c.bind(Param::Text(v.clone()));
            let has =
                format!("EXISTS (SELECT 1 FROM unnest({col}) l WHERE lower(l) = lower({ph}))");
            match cond.op {
                CmpOp::Ne | CmpOp::NotIn | CmpOp::NotLike => format!("(NOT {has})"),
                _ => format!("({has})"),
            }
        }
        (Kind::Array, Value::List(items)) => {
            let ph = c.bind(Param::TextList(lower_all(items)));
            let any = format!("EXISTS (SELECT 1 FROM unnest({col}) l WHERE lower(l) = ANY({ph}))");
            match cond.op {
                CmpOp::NotIn => format!("(NOT {any})"),
                _ => format!("({any})"),
            }
        }

        // ── full text (title + description) ──────────────────────────────
        (Kind::FullText, Value::Text(v)) => {
            let ph = c.bind(Param::Text(format!("%{}%", escape_like(v))));
            let like = format!("({col}) ILIKE {ph}");
            match cond.op {
                CmpOp::Ne | CmpOp::NotLike | CmpOp::NotIn => format!("(NOT ({like}))"),
                _ => format!("({like})"),
            }
        }
        (Kind::FullText, Value::List(items)) => {
            let parts: Vec<String> = items
                .iter()
                .map(|i| {
                    let ph = c.bind(Param::Text(format!("%{}%", escape_like(i))));
                    format!("({col}) ILIKE {ph}")
                })
                .collect();
            let any = format!("({})", parts.join(" OR "));
            match cond.op {
                CmpOp::NotIn => format!("(NOT {any})"),
                _ => any,
            }
        }

        // ── numbers ──────────────────────────────────────────────────────
        (Kind::Number, Value::Num(n)) => {
            let ph = c.bind(Param::Num(*n));
            let op = sql_cmp(cond.op);
            format!("({col} {op} {ph})")
        }
        (Kind::Number, Value::List(items)) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|i| i.parse::<f64>().ok())
                .map(|n| {
                    let ph = c.bind(Param::Num(n));
                    format!("{col} = {ph}")
                })
                .collect();
            if parts.is_empty() {
                return "false".into();
            }
            let any = format!("({})", parts.join(" OR "));
            match cond.op {
                CmpOp::NotIn => format!("(NOT {any})"),
                _ => any,
            }
        }

        // ── dates / timestamps ───────────────────────────────────────────
        (Kind::Date, Value::Date(d)) | (Kind::Timestamp, Value::Date(d)) => {
            let ph = c.bind(Param::Date(*d));
            let lhs = if kind == Kind::Timestamp {
                format!("({col})::date")
            } else {
                col.to_string()
            };
            let op = sql_cmp(cond.op);
            format!("({lhs} {op} {ph})")
        }

        // ── plain text ───────────────────────────────────────────────────
        (_, Value::CurrentUser) => {
            let ph = c.bind(Param::Text(c.me.clone()));
            let eq = format!("lower({col}) = lower({ph})");
            match cond.op {
                CmpOp::Ne | CmpOp::NotIn | CmpOp::NotLike => format!("(NOT ({eq}))"),
                _ => format!("({eq})"),
            }
        }
        (_, Value::Text(v)) => match cond.op {
            CmpOp::Like | CmpOp::NotLike => {
                let ph = c.bind(Param::Text(format!("%{}%", escape_like(v))));
                let like = format!("{col} ILIKE {ph}");
                if cond.op == CmpOp::NotLike {
                    format!("(NOT ({like}))")
                } else {
                    format!("({like})")
                }
            }
            CmpOp::Gt | CmpOp::Gte | CmpOp::Lt | CmpOp::Lte => {
                let ph = c.bind(Param::Text(v.clone()));
                format!("({col} {} {ph})", sql_cmp(cond.op))
            }
            CmpOp::Ne => {
                let ph = c.bind(Param::Text(v.clone()));
                // NULL != 'x' is NULL, not true — but "status != done" should
                // include unassigned/empty rows, which is what people mean.
                format!("({col} IS NULL OR lower({col}) <> lower({ph}))")
            }
            _ => {
                let ph = c.bind(Param::Text(v.clone()));
                format!("(lower({col}) = lower({ph}))")
            }
        },
        (_, Value::List(items)) => {
            let ph = c.bind(Param::TextList(lower_all(items)));
            let any = format!("lower({col}) = ANY({ph})");
            match cond.op {
                CmpOp::NotIn => format!("({col} IS NULL OR NOT ({any}))"),
                _ => format!("({any})"),
            }
        }
        (_, Value::Num(n)) => {
            // e.g. `title = 12` — compare as text rather than refusing.
            let ph = c.bind(Param::Text(trim_num(*n)));
            format!("(lower({col}) = lower({ph}))")
        }
        (_, Value::Date(d)) => {
            let ph = c.bind(Param::Text(d.to_string()));
            format!("(lower({col}) = lower({ph}))")
        }
        (_, Value::None) => "true".into(),
    }
}

fn sql_cmp(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Ne | CmpOp::NotIn | CmpOp::NotLike => "<>",
        CmpOp::Gt => ">",
        CmpOp::Gte => ">=",
        CmpOp::Lt => "<",
        CmpOp::Lte => "<=",
        _ => "=",
    }
}

fn lower_all(items: &[String]) -> Vec<String> {
    items.iter().map(|i| i.to_lowercase()).collect()
}

/// `%` and `_` are wildcards in ILIKE; a user typing them means the character.
fn escape_like(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn trim_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// The field list, for the UI's cheatsheet and autocomplete.
pub fn field_names() -> Vec<&'static str> {
    vec![
        "key",
        "project",
        "title",
        "description",
        "text",
        "status",
        "priority",
        "type",
        "assignee",
        "reporter",
        "label",
        "sprint",
        "epic",
        "parent",
        "points",
        "estimate",
        "due",
        "created",
        "updated",
        "completed",
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sql(src: &str) -> (String, Vec<Param>) {
        let q = parse(src).expect("parses");
        let c = compile(&q, "masein", 2);
        (c.where_sql, c.params)
    }

    #[test]
    fn empty_query_matches_everything() {
        let q = parse("").unwrap();
        assert!(q.filter.is_none());
        let c = compile(&q, "me", 2);
        assert_eq!(c.where_sql, "true");
        assert!(c.params.is_empty());
    }

    #[test]
    fn and_binds_tighter_than_or() {
        let q = parse("status = todo OR status = review AND priority = p0").unwrap();
        // Expect Or(status=todo, And(status=review, priority=p0)).
        match q.filter.unwrap() {
            Expr::Or(_, right) => assert!(matches!(*right, Expr::And(_, _))),
            other => panic!("expected OR at the top, got {other:?}"),
        }
    }

    #[test]
    fn values_are_always_bound_never_interpolated() {
        let (where_sql, params) = sql("title ~ \"'; DROP TABLE tasks; --\"");
        assert!(
            !where_sql.contains("DROP"),
            "value leaked into SQL: {where_sql}"
        );
        assert_eq!(params.len(), 1);
        assert!(matches!(&params[0], Param::Text(t) if t.contains("DROP TABLE")));
    }

    #[test]
    fn placeholders_start_after_the_callers_params() {
        let (where_sql, _) = sql("status = todo AND priority = p1");
        assert!(where_sql.contains("$2"), "{where_sql}");
        assert!(where_sql.contains("$3"), "{where_sql}");
        assert!(
            !where_sql.contains("$1"),
            "clobbered the caller's $1: {where_sql}"
        );
    }

    #[test]
    fn current_user_resolves_to_the_caller() {
        let (_, params) = sql("assignee = currentUser()");
        assert_eq!(params, vec![Param::Text("masein".into())]);
    }

    #[test]
    fn in_list_becomes_one_array_param() {
        let (where_sql, params) = sql("status in (todo, \"in_progress\")");
        assert!(where_sql.contains("ANY($2)"), "{where_sql}");
        assert_eq!(
            params,
            vec![Param::TextList(vec!["todo".into(), "in_progress".into()])]
        );
    }

    #[test]
    fn labels_test_membership_not_equality() {
        let (where_sql, _) = sql("label = backend");
        assert!(where_sql.contains("unnest(t.labels)"), "{where_sql}");
    }

    #[test]
    fn is_empty_and_equals_empty_agree() {
        let (a, _) = sql("assignee is empty");
        let (b, _) = sql("assignee = empty");
        assert_eq!(a, b);
        assert!(a.contains("IS NULL"), "{a}");
        let (c, _) = sql("assignee is not empty");
        assert!(c.starts_with("(NOT"), "{c}");
    }

    #[test]
    fn not_equals_includes_null_rows() {
        let (where_sql, _) = sql("status != done");
        assert!(where_sql.contains("IS NULL OR"), "{where_sql}");
    }

    #[test]
    fn relative_dates_resolve_against_today() {
        let q = parse("due <= 7d").unwrap();
        let today = Utc::now().date_naive();
        match q.filter.unwrap() {
            Expr::Cond(c) => assert_eq!(c.value, Value::Date(today + Duration::days(7))),
            other => panic!("expected a condition, got {other:?}"),
        }
    }

    #[test]
    fn like_wildcards_in_values_are_escaped() {
        let (_, params) = sql("title ~ \"100%\"");
        assert_eq!(params, vec![Param::Text("%100\\%%".into())]);
    }

    #[test]
    fn order_by_only_is_a_valid_query() {
        let q = parse("ORDER BY due ASC, priority DESC").unwrap();
        assert!(q.filter.is_none());
        let c = compile(&q, "me", 2);
        assert_eq!(
            c.order_sql,
            "t.due_date ASC NULLS LAST, t.priority DESC NULLS LAST"
        );
    }

    #[test]
    fn unknown_field_points_at_the_word() {
        let e = parse("status = todo AND banana = 3").unwrap_err();
        assert!(e.message.contains("banana"), "{}", e.message);
        assert_eq!(e.at, 18, "should point at `banana`, not the whole query");
    }

    #[test]
    fn missing_operator_names_the_field() {
        let e = parse("status todo").unwrap_err();
        assert!(e.message.contains("status"), "{}", e.message);
        assert!(e.message.contains("operator"), "{}", e.message);
    }

    #[test]
    fn unclosed_paren_and_quote_are_caught() {
        assert!(parse("(status = todo").is_err());
        assert!(parse("title ~ \"oops").is_err());
        assert!(parse("status in (todo").is_err());
    }

    #[test]
    fn trailing_condition_without_a_connective_is_an_error() {
        let e = parse("status = todo priority = p0").unwrap_err();
        assert!(e.message.contains("AND"), "{}", e.message);
    }

    #[test]
    fn number_fields_reject_words() {
        let e = parse("points = high").unwrap_err();
        assert!(e.message.contains("number"), "{}", e.message);
    }

    #[test]
    fn unsortable_fields_are_rejected() {
        assert!(parse("ORDER BY label").is_err());
        assert!(parse("ORDER BY text").is_err());
    }

    #[test]
    fn current_user_only_applies_to_people_fields() {
        assert!(parse("status = currentUser()").is_err());
        assert!(parse("reporter = currentUser()").is_ok());
    }

    #[test]
    fn quoted_keywords_are_values_not_syntax() {
        let (_, params) = sql("title = \"and or not\"");
        assert_eq!(params, vec![Param::Text("and or not".into())]);
    }

    #[test]
    fn nested_groups_survive_the_round_trip() {
        let (where_sql, _) =
            sql("(status = todo OR status = review) AND NOT (assignee = currentUser())");
        assert!(where_sql.contains(" OR "), "{where_sql}");
        assert!(where_sql.contains("NOT"), "{where_sql}");
    }
}
