#![allow(clippy::upper_case_acronyms)]

use itertools::Itertools;
use pest::{
    error::{Error as PestError, ErrorVariant},
    iterators::Pair,
    Parser, RuleType,
};

use crate::ast::*;

pub type ParseResult<T> = Result<T, PestError<Rule>>;

macro_rules! debug_pair {
    ($pair:expr) => {
        #[cfg(feature = "debug")]
        println!("{:?}: {}", $pair.as_rule(), $pair.as_str());
    };
}

fn only<R>(pair: Pair<R>) -> Pair<R>
where
    R: RuleType,
{
    pair.into_inner().next().unwrap()
}

#[derive(pest_derive::Parser)]
#[grammar = "grammar.pest"]
struct NootParser;

pub fn parse(input: &str) -> ParseResult<Items> {
    match NootParser::parse(Rule::file, &input) {
        Ok(mut pairs) => parse_items(only(pairs.next().unwrap())),
        Err(e) => Err(e),
    }
}

fn parse_items(pair: Pair<Rule>) -> ParseResult<Items> {
    debug_pair!(pair);
    let mut items = Vec::new();
    for pair in pair.into_inner() {
        match pair.as_rule() {
            Rule::item => items.push(parse_item(pair)?),
            Rule::EOI => {}
            rule => unreachable!("{:?}", rule),
        }
    }
    Ok(Items { items })
}

fn parse_item(pair: Pair<Rule>) -> ParseResult<Item> {
    debug_pair!(pair);
    let pair = only(pair);
    Ok(match pair.as_rule() {
        Rule::expr => Item::Expression(parse_expr(pair)?),
        Rule::def => Item::Def(parse_def(pair)?),
        rule => unreachable!("{:?}", rule),
    })
}

fn parse_def(pair: Pair<Rule>) -> ParseResult<Def> {
    debug_pair!(pair);
    let mut pairs = pair.into_inner();
    let ident = pairs.next().unwrap().as_str().to_owned();
    let mut params = Vec::new();
    for pair in pairs.by_ref() {
        if let Rule::param = pair.as_rule() {
            let mut pairs = pair.into_inner();
            let ident = pairs.next().unwrap().as_str().to_owned();
            let param = Param { ident };
            params.push(param);
        } else {
            break;
        }
    }
    let pair = pairs.next().unwrap();
    let items = match pair.as_rule() {
        Rule::items => parse_items(pair)?,
        Rule::expr => Items::wrapping(Item::wrapping(parse_expr(pair)?)),
        rule => unreachable!("{:?}", rule),
    };
    Ok(Def {
        ident,
        params: Params { params },
        items,
    })
}

fn parse_expr(pair: Pair<Rule>) -> ParseResult<Expression> {
    debug_pair!(pair);
    let pair = only(pair);
    Ok(match pair.as_rule() {
        Rule::expr_or => parse_expr_or(pair)?,
        rule => unreachable!("{:?}", rule),
    })
}

fn parse_expr_or(pair: Pair<Rule>) -> ParseResult<ExprOr> {
    debug_pair!(pair);
    let mut pairs = pair.into_inner();
    let left = pairs.next().unwrap();
    let left = parse_expr_and(left)?;
    let mut rights = Vec::new();
    for (op, right) in pairs.tuples() {
        let op = match op.as_str() {
            "or" => OpOr,
            rule => unreachable!("{:?}", rule),
        };
        let right = parse_expr_and(right)?;
        rights.push(Right { op, expr: right });
    }
    Ok(ExprOr {
        left: left.into(),
        rights,
    })
}

fn parse_expr_and(pair: Pair<Rule>) -> ParseResult<ExprAnd> {
    debug_pair!(pair);
    let mut pairs = pair.into_inner();
    let left = pairs.next().unwrap();
    let left = parse_expr_cmp(left)?;
    let mut rights = Vec::new();
    for (op, right) in pairs.tuples() {
        let op = match op.as_str() {
            "and" => OpAnd,
            rule => unreachable!("{:?}", rule),
        };
        let right = parse_expr_cmp(right)?;
        rights.push(Right { op, expr: right });
    }
    Ok(ExprAnd {
        left: left.into(),
        rights,
    })
}

fn parse_expr_cmp(pair: Pair<Rule>) -> ParseResult<ExprCmp> {
    debug_pair!(pair);
    let mut pairs = pair.into_inner();
    let left = pairs.next().unwrap();
    let left = parse_expr_as(left)?;
    let mut rights = Vec::new();
    for (op, right) in pairs.tuples() {
        let op = match op.as_str() {
            "is" => OpCmp::Is,
            "isnt" => OpCmp::Isnt,
            "<=" => OpCmp::LessOrEqual,
            ">=" => OpCmp::GreaterOrEqual,
            "<" => OpCmp::Less,
            ">" => OpCmp::Greater,
            rule => unreachable!("{:?}", rule),
        };
        let right = parse_expr_as(right)?;
        rights.push(Right { op, expr: right });
    }
    Ok(ExprCmp {
        left: left.into(),
        rights,
    })
}

fn parse_expr_as(pair: Pair<Rule>) -> ParseResult<ExprAS> {
    debug_pair!(pair);
    let mut pairs = pair.into_inner();
    let left = pairs.next().unwrap();
    let left = parse_expr_mdr(left)?;
    let mut rights = Vec::new();
    for (op, right) in pairs.tuples() {
        let op = match op.as_str() {
            "+" => OpAS::Add,
            "-" => OpAS::Sub,
            rule => unreachable!("{:?}", rule),
        };
        let right = parse_expr_mdr(right)?;
        rights.push(Right { op, expr: right });
    }
    Ok(ExprAS {
        left: left.into(),
        rights,
    })
}

fn parse_expr_mdr(pair: Pair<Rule>) -> ParseResult<ExprMDR> {
    debug_pair!(pair);
    let mut pairs = pair.into_inner();
    let left = pairs.next().unwrap();
    let left = parse_expr_not(left)?;
    let mut rights = Vec::new();
    for (op, right) in pairs.tuples() {
        let op = match op.as_str() {
            "*" => OpMDR::Mul,
            "/" => OpMDR::Div,
            "%" => OpMDR::Rem,
            rule => unreachable!("{:?}", rule),
        };
        let right = parse_expr_not(right)?;
        rights.push(Right { op, expr: right });
    }
    Ok(ExprMDR {
        left: left.into(),
        rights,
    })
}

fn parse_expr_not(pair: Pair<Rule>) -> ParseResult<ExprNot> {
    debug_pair!(pair);
    let mut pairs = pair.into_inner();
    let first = pairs.next().unwrap();
    let op = match first.as_str() {
        "not" => Some(OpNot),
        _ => None,
    };
    let pair = if op.is_some() {
        pairs.next().unwrap()
    } else {
        first
    };
    let expr = parse_expr_call(pair)?;
    Ok(ExprNot { op, expr })
}

fn parse_expr_call(pair: Pair<Rule>) -> ParseResult<ExprCall> {
    debug_pair!(pair);
    let pairs = pair.into_inner();
    let mut calls = Vec::new();
    let mut chained = None;
    for pair in pairs {
        match pair.as_rule() {
            Rule::expr_call_single => {
                let mut pairs = pair.into_inner();
                let term = parse_term(pairs.next().unwrap())?;
                let mut args = Vec::new();
                for pair in pairs {
                    let arg = parse_term(pair)?;
                    args.push(arg);
                }
                calls.push(ExprCall {
                    term,
                    args,
                    chained: chained.take(),
                });
            }
            Rule::chain_call => chained = Some(pair.as_str().into()),
            rule => unreachable!("{:?}", rule),
        }
    }
    let mut calls = calls.into_iter();
    let mut call = calls.next().unwrap();
    for mut chained_call in calls {
        chained_call.args.insert(
            0,
            Term::wrapping(Items::wrapping(Item::wrapping(ExprOr::wrapping(
                ExprAnd::wrapping(ExprCmp::wrapping(ExprAS::wrapping(ExprMDR::wrapping(
                    ExprNot::wrapping(call),
                )))),
            )))),
        );
        call = chained_call;
    }
    Ok(call)
}

fn parse_term(pair: Pair<Rule>) -> ParseResult<Term> {
    debug_pair!(pair);
    let pair = only(pair);
    macro_rules! number_literal {
        ($term:ident) => {
            pair.as_str().parse().map(Term::$term).map_err(|_| {
                PestError::new_from_span(
                    ErrorVariant::CustomError {
                        message: format!(
                            concat!("Invalid ", stringify!($term), " literal \"{}\""),
                            pair.as_str()
                        ),
                    },
                    pair.as_span(),
                )
            })
        };
    }
    Ok(match pair.as_rule() {
        Rule::nat => number_literal!(Nat)?,
        Rule::int => number_literal!(Int)?,
        Rule::real => number_literal!(Real)?,
        Rule::nil => Term::Nil,
        Rule::bool_literal => Term::Bool(pair.as_str() == "true"),
        Rule::ident => Term::Ident(pair.as_str().into()),
        Rule::paren_expr => {
            let pair = only(pair);
            let items = parse_items(pair)?;
            Term::wrapping(items)
        }
        Rule::string => {
            let string = parse_string_literal(pair);
            Term::String(string)
        }
        Rule::closure => {
            let mut pairs = pair.into_inner();
            let mut params = Vec::new();
            for pair in pairs.by_ref() {
                if let Rule::param = pair.as_rule() {
                    let mut pairs = pair.into_inner();
                    let ident = pairs.next().unwrap().as_str().to_owned();
                    let param = Param { ident };
                    params.push(param);
                } else {
                    break;
                }
            }
            let body = parse_items(pairs.next().unwrap())?;
            Term::Closure(
                Closure {
                    params: Params { params },
                    body,
                }
                .into(),
            )
        }
        rule => unreachable!("{:?}", rule),
    })
}

fn parse_string_literal(pair: Pair<Rule>) -> std::string::String {
    debug_pair!(pair);
    let mut s = String::new();
    for pair in pair.into_inner() {
        match pair.as_rule() {
            Rule::raw_string => s.push_str(pair.as_str()),
            Rule::escape => {
                let pair = only(pair);
                match pair.as_rule() {
                    Rule::predefined => s.push(match pair.as_str() {
                        "0" => '\0',
                        "r" => '\r',
                        "t" => '\t',
                        "n" => '\n',
                        "\\" => '\\',
                        "'" => '\'',
                        "\"" => '"',
                        s => unreachable!("{}", s),
                    }),
                    Rule::byte => {
                        let byte = pair
                            .into_inner()
                            .map(|pair| pair.as_str())
                            .collect::<String>()
                            .parse::<u8>()
                            .unwrap();
                        s.push(byte as char);
                    }
                    Rule::unicode => {
                        let u = pair
                            .into_inner()
                            .map(|pair| pair.as_str())
                            .collect::<String>()
                            .parse::<u32>()
                            .unwrap();
                        s.push(
                            std::char::from_u32(u)
                                .unwrap_or_else(|| panic!("invalid unicode {}", u)),
                        );
                    }
                    rule => unreachable!("{:?}", rule),
                }
            }
            rule => unreachable!("{:?}", rule),
        }
    }
    s
}
