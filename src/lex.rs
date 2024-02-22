use std::{
    collections::VecDeque,
    error::Error,
    fmt,
    hash::Hash,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};

use serde::*;
use serde_tuple::*;
use unicode_segmentation::UnicodeSegmentation;

use crate::{Inputs, Primitive};

/// Lex a Uiua source file
pub fn lex(
    input: &str,
    src: impl IntoInputSrc,
    inputs: &mut Inputs,
) -> (Vec<Sp<Token>>, Vec<Sp<LexError>>) {
    let src = inputs.add_src(src, input);
    Lexer {
        input,
        input_segments: input.graphemes(true).collect(),
        loc: Loc {
            char_pos: 0,
            byte_pos: 0,
            line: 1,
            col: 1,
        },
        src,
        tokens: VecDeque::new(),
        errors: Vec::new(),
    }
    .run()
}

/// An error that occurred while lexing
#[allow(missing_docs)]
#[derive(Debug, Clone)]
pub enum LexError {
    UnexpectedChar(String),
    ExpectedCharacter(Vec<char>),
    InvalidEscape(String),
    ExpectedNumber,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::UnexpectedChar(c) => write!(f, "Unexpected char {c:?}"),
            LexError::ExpectedCharacter(chars) if chars.is_empty() => {
                write!(f, "Expected character")
            }
            LexError::ExpectedCharacter(chars) if chars.len() == 1 => {
                write!(f, "Expected {:?}", chars[0])
            }
            LexError::ExpectedCharacter(chars) if chars.len() == 2 => {
                write!(f, "Expected {:?} or {:?}", chars[0], chars[1])
            }
            LexError::ExpectedCharacter(chars) => write!(f, "Expected one of {:?}", chars),
            LexError::InvalidEscape(c) => write!(f, "Invalid escape character {c:?}"),
            LexError::ExpectedNumber => write!(f, "Expected number"),
        }
    }
}

impl Error for LexError {}

/// A location in a Uiua source file
#[allow(missing_docs)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize_tuple, Deserialize_tuple,
)]
pub struct Loc {
    pub byte_pos: u32,
    pub char_pos: u32,
    pub line: u16,
    pub col: u16,
}

impl fmt::Display for Loc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

impl Default for Loc {
    fn default() -> Self {
        Self {
            char_pos: 0,
            byte_pos: 0,
            line: 1,
            col: 1,
        }
    }
}

/// A runtime span in a Uiua source file
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Span {
    /// A span that has a place in actual code
    Code(CodeSpan),
    /// A span whose origin in the interpreter
    Builtin,
}

impl From<CodeSpan> for Span {
    fn from(span: CodeSpan) -> Self {
        Self::Code(span)
    }
}

impl Span {
    /// Use this span to wrap a value
    pub fn sp<T>(self, value: T) -> Sp<T, Self> {
        Sp { value, span: self }
    }
    /// Merge two spans
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Span::Code(a), Span::Code(b)) => Span::Code(a.merge(b)),
            (Span::Code(a), Span::Builtin) => Span::Code(a),
            (Span::Builtin, Span::Code(b)) => Span::Code(b),
            (Span::Builtin, Span::Builtin) => Span::Builtin,
        }
    }
    /// Get the code span, if any
    pub fn code(self) -> Option<CodeSpan> {
        match self {
            Span::Code(span) => Some(span),
            Span::Builtin => None,
        }
    }
}

/// The source of code input into the interpreter
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(untagged, into = "InputSrcRep", from = "InputSrcRep")]
pub enum InputSrc {
    /// Code from a file with a path
    File(Arc<Path>),
    /// Code from a string
    Str(usize),
}

#[derive(Serialize, Deserialize)]
enum InputSrcRep {
    File(PathBuf),
    Str(usize),
}

impl From<InputSrc> for InputSrcRep {
    fn from(src: InputSrc) -> Self {
        match src {
            InputSrc::File(path) => InputSrcRep::File(path.to_path_buf()),
            InputSrc::Str(index) => InputSrcRep::Str(index),
        }
    }
}

impl From<InputSrcRep> for InputSrc {
    fn from(src: InputSrcRep) -> Self {
        match src {
            InputSrcRep::File(path) => InputSrc::File(path.into()),
            InputSrcRep::Str(index) => InputSrc::Str(index),
        }
    }
}

impl<'a> From<&'a Path> for InputSrc {
    fn from(path: &'a Path) -> Self {
        InputSrc::File(path.into())
    }
}

/// A trait for types that can be converted into an `InputSrc`
pub trait IntoInputSrc {
    /// Convert into an `InputSrc`
    fn into_input_src(self, str_index: usize) -> InputSrc;
}

impl IntoInputSrc for InputSrc {
    fn into_input_src(self, _: usize) -> InputSrc {
        self
    }
}

impl<'a> IntoInputSrc for &'a Path {
    fn into_input_src(self, _: usize) -> InputSrc {
        self.into()
    }
}

impl<'a> IntoInputSrc for &'a PathBuf {
    fn into_input_src(self, _: usize) -> InputSrc {
        self.as_path().into()
    }
}

impl IntoInputSrc for () {
    fn into_input_src(self, str_index: usize) -> InputSrc {
        InputSrc::Str(str_index)
    }
}

/// A span in a Uiua source file
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize_tuple, Deserialize_tuple)]
pub struct CodeSpan {
    /// The path of the file
    pub src: InputSrc,
    /// The starting location
    pub start: Loc,
    /// The ending location
    pub end: Loc,
}

impl fmt::Debug for CodeSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for CodeSpan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.src {
            InputSrc::File(path) => {
                let mut file: String = path.to_string_lossy().into_owned();
                if let Some(s) = file.strip_prefix("C:\\Users\\") {
                    if let Some((_, sub)) = s.split_once('\\') {
                        file = format!("~\\{}", sub);
                    } else {
                        file = s.to_string();
                    }
                }
                let file = file.replace("\\.\\", "\\");
                write!(f, "{}:{}", file, self.start)
            }
            InputSrc::Str(_) => write!(f, "{}", self.start),
        }
    }
}

impl fmt::Debug for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Span::Code(span) => write!(f, "{span}"),
            Span::Builtin => write!(f, "<builtin>"),
        }
    }
}

impl CodeSpan {
    pub(crate) const fn sp<T>(self, value: T) -> Sp<T> {
        Sp { value, span: self }
    }
    pub(crate) fn dummy() -> Self {
        Self {
            src: InputSrc::Str(0),
            start: Loc::default(),
            end: Loc::default(),
        }
    }
    /// Merge two spans
    pub fn merge(self, end: Self) -> Self {
        CodeSpan {
            start: self.start.min(end.start),
            end: self.end.max(end.end),
            ..self
        }
    }
    /// Get the text of the span
    pub fn byte_range(&self) -> Range<usize> {
        self.start.byte_pos as usize..self.end.byte_pos as usize
    }
    /// Check if the span contains a line and column
    pub fn contains_line_col(&self, line: usize, col: usize) -> bool {
        let line = line as u16;
        let col = col as u16;
        if self.start.line == self.end.line {
            self.start.line == line && (self.start.col..=self.end.col).contains(&col)
        } else {
            (self.start.line..=self.end.line).contains(&line)
                && (self.start.line < line || col >= self.start.col)
                && (self.end.line > line || col <= self.end.col)
        }
    }
    /// Get the text of the span from the inputs
    pub fn as_str<T>(&self, inputs: &Inputs, f: impl FnOnce(&str) -> T) -> T {
        inputs.get_with(&self.src, |input| f(&input[self.byte_range()]))
    }
    /// Get just the span of the first character
    pub fn just_start(&self, inputs: &Inputs) -> Self {
        let start = self.start;
        let mut end = self.start;
        end.char_pos += 1;
        end.byte_pos += self.as_str(inputs, |s| {
            s.chars().next().map_or(0, char::len_utf8) as u32
        });
        end.col += 1;
        CodeSpan {
            start,
            end,
            ..self.clone()
        }
    }
    /// Get just the span of the last character
    pub fn just_end(&self, inputs: &Inputs) -> Self {
        let end = self.end;
        let mut start = self.end;
        start.char_pos = start.char_pos.saturating_sub(1);
        start.byte_pos = start.byte_pos.saturating_sub(self.as_str(inputs, |s| {
            s.chars().next_back().map_or(0, char::len_utf8) as u32
        }));
        start.col = start.col.saturating_sub(1);
        CodeSpan {
            start,
            end,
            ..self.clone()
        }
    }
}

/// A span wrapping a value
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Sp<T, S = CodeSpan> {
    /// The value
    pub value: T,
    /// The span
    pub span: S,
}

impl<T> Sp<T> {
    /// Map the value
    pub fn map<U, F: FnOnce(T) -> U>(self, f: F) -> Sp<U> {
        Sp {
            value: f(self.value),
            span: self.span,
        }
    }
    /// Map the value into a new one
    pub fn map_into<U>(self) -> Sp<U>
    where
        T: Into<U>,
    {
        self.map(Into::into)
    }
    /// Get a spanned reference to the value
    pub fn as_ref(&self) -> Sp<&T> {
        Sp {
            value: &self.value,
            span: self.span.clone(),
        }
    }
    /// Maybe map the value
    pub fn filter_map<U>(self, f: impl FnOnce(T) -> Option<U>) -> Option<Sp<U>> {
        f(self.value).map(|value| Sp {
            value,
            span: self.span,
        })
    }
}

impl<T: Clone> Sp<&T> {
    /// Clone a span-wrapped reference
    pub fn cloned(self) -> Sp<T> {
        Sp {
            value: self.value.clone(),
            span: self.span,
        }
    }
}

impl<T: fmt::Debug, S: fmt::Display> fmt::Debug for Sp<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.span)?;
        self.value.fmt(f)
    }
}

impl<T: fmt::Display, S: fmt::Display> fmt::Display for Sp<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.span, self.value)
    }
}

impl<T: Error> Error for Sp<T> {}

impl<T> From<Sp<T>> for Sp<T, Span> {
    fn from(value: Sp<T>) -> Self {
        Self {
            value: value.value,
            span: Span::Code(value.span),
        }
    }
}

/// A Uiua lexical token
#[allow(missing_docs)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Comment,
    OutputComment(usize),
    Ident,
    Number,
    Char(String),
    Str(String),
    Label(String),
    FormatStr(Vec<String>),
    MultilineString(Vec<String>),
    Simple(AsciiToken),
    Glyph(Primitive),
    LeftArrow,
    LeftStrokeArrow,
    LeftArrowTilde,
    OpenAngle,
    CloseAngle,
    Newline,
    Spaces,
}

impl Token {
    pub(crate) fn as_char(&self) -> Option<String> {
        match self {
            Token::Char(char) => Some(char.clone()),
            _ => None,
        }
    }
    pub(crate) fn as_string(&self) -> Option<&str> {
        match self {
            Token::Str(string) => Some(string),
            _ => None,
        }
    }
    pub(crate) fn as_format_string(&self) -> Option<Vec<String>> {
        match self {
            Token::FormatStr(frags) => Some(frags.clone()),
            _ => None,
        }
    }
    pub(crate) fn as_multiline_string(&self) -> Option<Vec<String>> {
        match self {
            Token::MultilineString(parts) => Some(parts.clone()),
            _ => None,
        }
    }
    pub(crate) fn as_output_comment(&self) -> Option<usize> {
        match self {
            Token::OutputComment(n) => Some(*n),
            _ => None,
        }
    }
    pub(crate) fn as_label(&self) -> Option<&str> {
        match self {
            Token::Label(label) => Some(label),
            _ => None,
        }
    }
}

/// An ASCII lexical token
#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AsciiToken {
    OpenParen,
    CloseParen,
    OpenCurly,
    CloseCurly,
    OpenBracket,
    CloseBracket,
    Underscore,
    Bar,
    Semicolon,
    Star,
    Percent,
    Caret,
    Equal,
    EqualTilde,
    BangEqual,
    LessEqual,
    GreaterEqual,
    Backtick,
    Tilde,
    TripleMinus,
    Quote,
    Quote2,
    Placeholder(crate::ast::PlaceholderOp),
}

impl fmt::Display for AsciiToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsciiToken::OpenParen => write!(f, "("),
            AsciiToken::CloseParen => write!(f, ")"),
            AsciiToken::OpenCurly => write!(f, "{{"),
            AsciiToken::CloseCurly => write!(f, "}}"),
            AsciiToken::OpenBracket => write!(f, "["),
            AsciiToken::CloseBracket => write!(f, "]"),
            AsciiToken::Underscore => write!(f, "_"),
            AsciiToken::Bar => write!(f, "|"),
            AsciiToken::Semicolon => write!(f, ";"),
            AsciiToken::Star => write!(f, "*"),
            AsciiToken::Percent => write!(f, "%"),
            AsciiToken::Caret => write!(f, "^"),
            AsciiToken::Equal => write!(f, "="),
            AsciiToken::BangEqual => write!(f, "!="),
            AsciiToken::EqualTilde => write!(f, "=~"),
            AsciiToken::LessEqual => write!(f, "<="),
            AsciiToken::GreaterEqual => write!(f, ">="),
            AsciiToken::Backtick => write!(f, "`"),
            AsciiToken::Tilde => write!(f, "~"),
            AsciiToken::TripleMinus => write!(f, "---"),
            AsciiToken::Quote => write!(f, "'"),
            AsciiToken::Quote2 => write!(f, "''"),
            AsciiToken::Placeholder(op) => write!(f, "{op}"),
        }
    }
}

impl From<AsciiToken> for Token {
    fn from(s: AsciiToken) -> Self {
        Self::Simple(s)
    }
}

impl From<Primitive> for Token {
    fn from(p: Primitive) -> Self {
        Self::Glyph(p)
    }
}

struct Lexer<'a> {
    input: &'a str,
    input_segments: Vec<&'a str>,
    loc: Loc,
    src: InputSrc,
    tokens: VecDeque<Sp<Token>>,
    errors: Vec<Sp<LexError>>,
}

impl<'a> Lexer<'a> {
    fn peek_char(&self) -> Option<&'a str> {
        self.input_segments.get(self.loc.char_pos as usize).copied()
    }
    fn update_loc(&mut self, c: &'a str) {
        for c in c.chars() {
            match c {
                '\n' => {
                    self.loc.line += 1;
                    self.loc.col = 1;
                }
                '\r' => {}
                _ => self.loc.col += 1,
            }
        }
        self.loc.char_pos += 1;
        self.loc.byte_pos += c.len() as u32;
    }
    fn next_char_if(&mut self, f: impl Fn(&str) -> bool) -> Option<&'a str> {
        let c = *self.input_segments.get(self.loc.char_pos as usize)?;
        if !f(c) {
            return None;
        }
        self.update_loc(c);
        Some(c)
    }
    fn next_char_if_all(&mut self, f: impl Fn(char) -> bool + Copy) -> Option<&'a str> {
        self.next_char_if(|c| c.chars().all(f))
    }
    fn next_char_exact(&mut self, c: &str) -> bool {
        self.next_char_if(|c2| c2 == c).is_some()
    }
    fn next_char(&mut self) -> Option<&'a str> {
        self.next_char_if(|_| true)
    }
    fn next_chars_exact<'b>(&mut self, s: impl IntoIterator<Item = &'b str>) -> bool {
        let start = self.loc;
        for c in s {
            if !self.next_char_exact(c) {
                self.loc = start;
                return false;
            }
        }
        true
    }
    fn make_span(&self, start: Loc, end: Loc) -> CodeSpan {
        CodeSpan {
            start,
            end,
            src: self.src.clone(),
        }
    }
    fn end_span(&self, start: Loc) -> CodeSpan {
        assert!(self.loc.char_pos >= start.char_pos, "empty span");
        self.make_span(start, self.loc)
    }
    fn end(&mut self, token: impl Into<Token>, start: Loc) {
        self.tokens.push_back(Sp {
            value: token.into(),
            span: self.end_span(start),
        })
    }
    fn run(mut self) -> (Vec<Sp<Token>>, Vec<Sp<LexError>>) {
        use {self::AsciiToken::*, Token::*};
        // Initial scope delimiters
        let start = self.loc;
        if self.next_chars_exact(["-", "-", "-"]) {
            self.end(TripleMinus, start);
        }
        // Main loop
        'main: loop {
            let start = self.loc;
            let Some(c) = self.next_char() else {
                break;
            };
            match c {
                // Backwards compatibility
                "❥" | "⇉" => self.end(Primitive::Fork, start),
                "→" => self.end(Primitive::Dip, start),
                "∷" => self.end(Primitive::Both, start),
                "·" => self.end(Primitive::Identity, start),
                "⍛" => self.end(Primitive::Fill, start),
                "⌂" => self.end(Primitive::Rise, start),
                "↰" => self.end(Primitive::Spawn, start),
                "↲" => self.end(Primitive::Wait, start),
                "≅" => self.end(Primitive::Match, start),
                "∶" => self.end(Primitive::Flip, start),
                "⍘" => self.end(Primitive::Un, start),
                "⊝" => self.end(Primitive::Deduplicate, start),
                "⊔" => self.end(Primitive::Content, start),

                "(" => self.end(OpenParen, start),
                ")" => self.end(CloseParen, start),
                "{" => self.end(OpenCurly, start),
                "}" => self.end(CloseCurly, start),
                "[" => self.end(OpenBracket, start),
                "]" => self.end(CloseBracket, start),
                "〈" => self.end(OpenAngle, start),
                "〉" => self.end(CloseAngle, start),
                "_" => self.end(Underscore, start),
                "|" => self.end(Bar, start),
                ";" => self.end(Semicolon, start),
                "'" if self.next_char_exact("'") => self.end(Quote2, start),
                "'" => self.end(Quote, start),
                "~" => self.end(Tilde, start),
                "`" => {
                    if self.number("-") {
                        self.end(Number, start)
                    } else {
                        self.end(Backtick, start)
                    }
                }
                "¯" if self
                    .peek_char()
                    .filter(|c| c.chars().all(|c| c.is_ascii_digit()))
                    .is_some() =>
                {
                    self.number("-");
                    self.end(Number, start)
                }
                "*" => self.end(Star, start),
                "%" => self.end(Percent, start),
                "^" if self.next_char_exact("!") => {
                    self.end(Placeholder(crate::ast::PlaceholderOp::Call), start)
                }
                "^" if self.next_char_exact(".") => {
                    self.end(Placeholder(crate::ast::PlaceholderOp::Dup), start)
                }
                "^" if self.next_char_exact(":") => {
                    self.end(Placeholder(crate::ast::PlaceholderOp::Flip), start)
                }
                "^" if self.next_char_exact(",") => {
                    self.end(Placeholder(crate::ast::PlaceholderOp::Over), start)
                }
                "^" => self.end(Caret, start),
                "=" if self.next_char_exact("~") => self.end(EqualTilde, start),
                "=" => self.end(Equal, start),
                "<" if self.next_char_exact("=") => self.end(LessEqual, start),
                ">" if self.next_char_exact("=") => self.end(GreaterEqual, start),
                "!" if self.next_char_exact("=") => self.end(BangEqual, start),
                "←" if self.next_char_exact("~") => self.end(LeftArrowTilde, start),
                "←" => self.end(LeftArrow, start),
                "↚" => self.end(LeftStrokeArrow, start),
                // Comments
                "#" => {
                    let mut n = 0;
                    while self.next_char_exact("#") {
                        n += 1;
                    }
                    if n == 0 {
                        let mut comment = String::new();
                        while let Some(c) = self.next_char_if(|c| !c.ends_with('\n')) {
                            comment.push_str(c);
                        }
                        if comment.starts_with(' ') {
                            comment.remove(0);
                        }
                        self.end(Comment, start);
                    } else {
                        loop {
                            while self.next_char_if(|c| !c.ends_with('\n')).is_some() {}
                            let restore = self.loc;
                            self.next_char_exact("\r");
                            self.next_char_exact("\n");
                            while self
                                .next_char_if(|c| c.chars().all(char::is_whitespace))
                                .is_some()
                            {}
                            if !self.next_chars_exact(["#", "#"]) {
                                self.loc = restore;
                                self.end(OutputComment(n), start);
                                continue 'main;
                            }
                            while self.next_char_exact("#") {}
                        }
                    }
                }
                // Characters
                "@" => {
                    let mut escaped = false;
                    let char = match self.character(&mut escaped, None) {
                        Ok(Some(c)) => c,
                        Ok(None) => {
                            self.errors
                                .push(self.end_span(start).sp(LexError::ExpectedCharacter(vec![])));
                            continue;
                        }
                        Err(e) => {
                            self.errors
                                .push(self.end_span(start).sp(LexError::InvalidEscape(e.into())));
                            continue;
                        }
                    };
                    self.end(Char(char), start)
                }
                // Strings
                "\"" | "$" => {
                    let format = c == "$";
                    if format
                        && (self.next_char_exact(" ")
                            || self.peek_char().map_or(true, |c| "\r\n".contains(c)))
                    {
                        // Multiline strings
                        let mut start = start;
                        loop {
                            let inner = self.parse_string_contents(start, None);
                            let string = parse_format_fragments(&inner);
                            self.end(MultilineString(string), start);
                            let checkpoint = self.loc;
                            while self.next_char_exact("\r") {}
                            if self.next_char_if(|c| c.ends_with('\n')).is_some() {
                                while self
                                    .next_char_if(|c| {
                                        c.chars().all(char::is_whitespace) && !c.ends_with('\n')
                                    })
                                    .is_some()
                                {}
                                start = self.loc;
                                if self.next_chars_exact(["$", " "]) || self.next_char_exact("$") {
                                    continue;
                                }
                            }
                            self.loc = checkpoint;
                            break;
                        }
                        continue;
                    }
                    if format && !self.next_char_exact("\"") {
                        let mut label = String::new();
                        while let Some(c) = self.next_char_if(|c| c.chars().all(is_ident_char)) {
                            label.push_str(c);
                        }
                        self.end(Label(label), start);
                        continue;
                    }
                    // Single-line strings
                    let inner = self.parse_string_contents(start, Some('"'));
                    if !self.next_char_exact("\"") {
                        self.errors.push(
                            self.end_span(start)
                                .sp(LexError::ExpectedCharacter(vec!['"'])),
                        );
                    }
                    if format {
                        let frags = parse_format_fragments(&inner);
                        self.end(FormatStr(frags), start)
                    } else {
                        self.end(Str(inner), start)
                    }
                }
                // Identifiers and unformatted glyphs
                c if is_custom_glyph(c) => self.end(Ident, start),
                c if c.chars().all(is_ident_char) || c == "&" => {
                    let mut ident = c.to_string();
                    // Collect characters
                    while let Some(c) = self.next_char_if_all(is_ident_char) {
                        ident.push_str(c);
                    }
                    let mut exclam_count = 0;
                    while let Some((ch, count)) = if self.next_char_exact("!") {
                        Some(('!', 1))
                    } else if self.next_char_exact("‼") {
                        Some(('‼', 2))
                    } else {
                        None
                    } {
                        ident.push(ch);
                        exclam_count += count;
                    }
                    let ambiguous_ne = exclam_count == 1
                        && self.input_segments.get(self.loc.char_pos as usize) == Some(&"=");
                    if ambiguous_ne {
                        ident.pop();
                    }
                    // Try to parse as primitives
                    let lowercase_end = ident
                        .char_indices()
                        .find(|(_, c)| c.is_ascii_uppercase())
                        .map_or(ident.len(), |(i, _)| i);
                    if let Some(prims) = Primitive::from_format_name_multi(&ident[..lowercase_end])
                    {
                        if ambiguous_ne {
                            self.loc.char_pos -= 1;
                            self.loc.byte_pos -= 1;
                        }
                        let mut start = start;
                        for (prim, frag) in prims {
                            let end = Loc {
                                col: start.col + frag.chars().count() as u16,
                                char_pos: start.char_pos + frag.chars().count() as u32,
                                byte_pos: start.byte_pos + frag.len() as u32,
                                ..start
                            };
                            self.tokens.push_back(Sp {
                                value: Glyph(prim),
                                span: self.make_span(start, end),
                            });
                            start = end;
                        }
                        let rest = &ident[lowercase_end..];
                        if !rest.is_empty() {
                            self.end(Ident, start);
                        }
                    } else if ident[..lowercase_end].starts_with("bind") {
                        let mut start = start;
                        for (token, a, b) in
                            [(Glyph(Primitive::Bind), 0, 4), (Ident, 4, lowercase_end)]
                        {
                            let end = Loc {
                                col: start.col + ident[a..b].chars().count() as u16,
                                char_pos: start.char_pos + ident[a..b].chars().count() as u32,
                                byte_pos: start.byte_pos + ident[a..b].len() as u32,
                                ..start
                            };
                            self.tokens.push_back(Sp {
                                value: token,
                                span: self.make_span(start, end),
                            });
                            start = end;
                        }
                        let rest = &ident[lowercase_end..];
                        if !rest.is_empty() {
                            self.end(Ident, start);
                        }
                    } else {
                        // Lone ident
                        self.end(Ident, start)
                    }
                }
                // Numbers
                c if c.chars().all(|c| c.is_ascii_digit()) => {
                    self.number(c);
                    self.end(Number, start)
                }
                // Newlines
                "\n" | "\r\n" => {
                    self.end(Newline, start);
                    // Scope delimiters
                    let start = self.loc;
                    if self.next_chars_exact(["-", "-", "-"]) {
                        self.end(TripleMinus, start);
                    }
                }
                " " | "\t" => {
                    while self.next_char_exact(" ") || self.next_char_exact("\t") {}
                    self.end(Spaces, start)
                }
                c if c.chars().all(|c| c.is_whitespace()) => continue,
                c => {
                    if c.chars().count() == 1 {
                        let c = c.chars().next().unwrap();
                        if let Some(prim) = Primitive::from_glyph(c) {
                            self.end(Glyph(prim), start);
                            continue;
                        }
                    }
                    self.errors
                        .push(self.end_span(start).sp(LexError::UnexpectedChar(c.into())));
                }
            };
        }

        // Combine some tokens

        struct PostLexer<'a> {
            tokens: VecDeque<Sp<Token>>,
            input: &'a str,
        }

        impl<'a> PostLexer<'a> {
            fn nth_is(&self, n: usize, f: impl Fn(&str) -> bool) -> bool {
                self.tokens
                    .get(n)
                    .is_some_and(|t| f(&self.input[t.span.byte_range()]))
            }
            fn next_if(&mut self, f: impl Fn(&str) -> bool) -> Option<Sp<Token>> {
                if self.nth_is(0, f) {
                    self.next()
                } else {
                    None
                }
            }
            fn next(&mut self) -> Option<Sp<Token>> {
                self.tokens.pop_front()
            }
        }

        let mut post = PostLexer {
            tokens: self.tokens,
            input: self.input,
        };

        let mut processed = Vec::new();
        while let Some(token) = post.next() {
            let s = &self.input[token.span.byte_range()];
            processed.push(
                if is_numbery(s) || (["`", "¯"].contains(&s) && post.nth_is(0, is_numbery)) {
                    let mut span = token.span;
                    if ["`", "¯"].contains(&s) {
                        let n_tok = post.next().unwrap();
                        span = span.merge(n_tok.span);
                    }
                    if post.nth_is(0, |s| s == "/")
                        && post.nth_is(1, |s| {
                            is_numbery(s) || (["`", "¯"].contains(&s) && post.nth_is(2, is_numbery))
                        })
                    {
                        let _slash = post.next().unwrap();
                        let _neg = post.next_if(|s| ["`", "¯"].contains(&s));
                        span = span.merge(post.next().unwrap().span);
                    }
                    span.sp(Number)
                } else {
                    token
                },
            );
        }

        (processed, self.errors)
    }
    fn number(&mut self, init: &str) -> bool {
        // Whole part
        let mut got_digit = false;
        while self
            .next_char_if(|c| c.chars().all(|c| c.is_ascii_digit()))
            .is_some()
        {
            got_digit = true;
        }
        if !init.chars().all(|c| c.is_ascii_digit()) && !got_digit {
            return false;
        }
        // Fractional part
        let before_dot = self.loc;
        if self.next_char_exact(".") {
            let mut has_decimal = false;
            while self
                .next_char_if(|c| c.chars().all(|c| c.is_ascii_digit()))
                .is_some()
            {
                has_decimal = true;
            }
            if !has_decimal {
                self.loc = before_dot;
            }
        }
        // Exponent
        let loc_before_e = self.loc;
        if self.next_char_if(|c| c == "e" || c == "E").is_some() {
            self.next_char_if(|c| c == "-" || c == "`" || c == "¯");
            let mut got_digit = false;
            while self
                .next_char_if(|c| c.chars().all(|c| c.is_ascii_digit()))
                .is_some()
            {
                got_digit = true;
            }
            if !got_digit {
                self.loc = loc_before_e;
            }
        }
        true
    }
    fn character(
        &mut self,
        escaped: &mut bool,
        escape_char: Option<char>,
    ) -> Result<Option<String>, &'a str> {
        let Some(c) =
            self.next_char_if_all(|c| !"\r\n".contains(c) && (Some(c) != escape_char || *escaped))
        else {
            return Ok(None);
        };
        Ok(Some(if *escaped {
            *escaped = false;
            match c {
                "n" => '\n'.to_string(),
                "r" => '\r'.to_string(),
                "t" => '\t'.to_string(),
                "0" => '\0'.to_string(),
                "s" => ' '.to_string(),
                "b" => '\x07'.to_string(),
                "\\" => '\\'.to_string(),
                "\"" => '"'.to_string(),
                "'" => '\''.to_string(),
                "_" => char::MAX.to_string(),
                "x" => {
                    let mut code = 0;
                    for _ in 0..2 {
                        let c = self
                            .next_char_if_all(|c| c.is_ascii_hexdigit())
                            .ok_or("x")?;
                        code = code << 4 | c.chars().next().unwrap().to_digit(16).unwrap();
                    }
                    std::char::from_u32(code).ok_or("x")?.into()
                }
                "u" => {
                    let mut code = 0;
                    match self.peek_char().ok_or("u")? {
                        "{" => {
                            self.next_char_if(|c| c == "{").ok_or("u")?;
                            for _ in 0..7 {
                                match self
                                    .next_char_if_all(|c| c.is_ascii_hexdigit() || c == '}')
                                    .ok_or("u")?
                                {
                                    "}" => break,
                                    c => {
                                        code = code << 4
                                            | c.chars().next().unwrap().to_digit(16).unwrap()
                                    }
                                }
                            }
                        }
                        _ => {
                            for _ in 0..4 {
                                let c = self
                                    .next_char_if_all(|c| c.is_ascii_hexdigit())
                                    .ok_or("u")?;
                                code = code << 4 | c.chars().next().unwrap().to_digit(16).unwrap();
                            }
                        }
                    }
                    std::char::from_u32(code).ok_or("u")?.into()
                }
                c => return Err(c),
            }
        } else if c == "\\" {
            *escaped = true;
            return self.character(escaped, escape_char);
        } else {
            c.into()
        }))
    }
    fn parse_string_contents(&mut self, start: Loc, escape_char: Option<char>) -> String {
        let mut string = String::new();
        let mut escaped = false;
        loop {
            match self.character(&mut escaped, escape_char) {
                Ok(Some(c)) => string.push_str(&c),
                Ok(None) => break,
                Err(e) => {
                    self.errors
                        .push(self.end_span(start).sp(LexError::InvalidEscape(e.into())));
                }
            }
        }
        string
    }
}

fn is_numbery(mut s: &str) -> bool {
    if s.starts_with(['`', '¯']) {
        let c_len = s.chars().next().unwrap().len_utf8();
        s = &s[c_len..];
    }
    if s.is_empty() {
        return false;
    }
    s.chars().all(|c| c.is_ascii_digit())
        || s == "∞"
        || (3..="infinity".len()).rev().any(|n| s == &"infinity"[..n])
        || [Primitive::Eta, Primitive::Pi, Primitive::Tau]
            .iter()
            .any(|p| {
                p.name() == s
                    || s.chars().count() == 1 && p.glyph().unwrap() == s.chars().next().unwrap()
            })
}

fn parse_format_fragments(s: &str) -> Vec<String> {
    let mut frags: Vec<String> = Vec::new();
    let mut curr = String::new();
    for c in s.chars() {
        match c {
            '_' => {
                frags.push(curr);
                curr = String::new();
            }
            char::MAX => curr.push('_'),
            c => curr.push(c),
        }
    }
    frags.push(curr);
    frags
}

/// Whether a character can be part of a Uiua identifier
pub fn is_ident_char(c: char) -> bool {
    c.is_alphabetic() && !"ⁿₙπτηℂλ".contains(c)
}

/// Whether a string is a custom glyph
pub fn is_custom_glyph(c: &str) -> bool {
    match c.chars().count() {
        0 => false,
        1 => {
            let c = c.chars().next().unwrap();
            !c.is_ascii() && !is_ident_char(c) && Primitive::from_glyph(c).is_none()
        }
        _ => c
            .chars()
            .all(|c| !c.is_ascii() && !is_ident_char(c) && Primitive::from_glyph(c).is_none()),
    }
}
