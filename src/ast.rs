use std::fmt;

use crate::{
    function::FunctionId,
    lex::{Sp, Span},
    ops::Primitive,
    Ident,
};

#[derive(Debug, Clone)]
pub enum Item {
    Words(Vec<Sp<Word>>),
    Let(Let),
    Const(Const),
}

impl Item {
    pub fn span(&self) -> Span {
        match self {
            Item::Words(words) => words[0]
                .span
                .clone()
                .merge(words.last().unwrap().span.clone()),
            Item::Let(r#let) => r#let
                .name
                .span
                .clone()
                .merge(r#let.words.last().unwrap().span.clone()),
            Item::Const(r#const) => r#const
                .name
                .span
                .clone()
                .merge(r#const.words.last().unwrap().span.clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Let {
    pub name: Sp<Ident>,
    pub words: Vec<Sp<Word>>,
}

#[derive(Debug, Clone)]
pub struct Const {
    pub name: Sp<Ident>,
    pub words: Vec<Sp<Word>>,
}

#[derive(Clone)]
pub enum Word {
    Real(String),
    Char(char),
    String(String),
    Ident(Ident),
    Array(Vec<Sp<Word>>),
    Strand(Vec<Sp<Word>>),
    Func(Func),
    Primitive(Primitive),
}

impl fmt::Debug for Word {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Word::Real(real) => write!(f, "{real:?}"),
            Word::Char(char) => write!(f, "{char:?}"),
            Word::String(string) => write!(f, "{{{string}}}"),
            Word::Ident(ident) => write!(f, "ident({ident})"),
            Word::Array(array) => write!(f, "array({array:?})"),
            Word::Strand(items) => write!(f, "strand({items:?})"),
            Word::Func(func) => func.fmt(f),
            Word::Primitive(prim) => prim.fmt(f),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Func {
    pub id: FunctionId,
    pub body: Vec<Sp<Word>>,
}
