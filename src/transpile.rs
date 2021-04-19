use std::{
    fmt,
    fs::{self, File},
    io::{self, Write},
    iter::once,
};

use itertools::*;
use pest::{
    error::{Error as PestError, ErrorVariant},
    Span,
};
use rpds::{List, Queue, RedBlackTreeMap, Vector};

use crate::{ast::*, parse::Rule};

#[derive(Debug, thiserror::Error)]
pub enum TranspileErrorKind {
    #[error("Unknown definition {0}")]
    UnknownDef(String),
}

impl TranspileErrorKind {
    pub fn span(self, span: Span) -> TranspileError {
        TranspileError { kind: self, span }
    }
}

#[derive(Debug)]
pub struct TranspileError<'a> {
    pub kind: TranspileErrorKind,
    pub span: Span<'a>,
}

impl<'a> fmt::Display for TranspileError<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let error = PestError::<Rule>::new_from_span(
            ErrorVariant::CustomError {
                message: self.kind.to_string(),
            },
            self.span.clone(),
        );
        write!(f, "{}", error)
    }
}

struct NootDef {
    is_function: bool,
    c_name: String,
}

macro_rules! builtin_functions {
    ($($name:literal),*) => {
        &[$(($name, concat!("noot_", $name))),*]
    }
}

const BUILTIN_FUNCTIONS: &[(&str, &str)] =
    builtin_functions!("print", "println", "len", "list", "error", "panic");
const BUILTIN_VALUES: &[(&str, &str)] = &[("table", "NOOT_EMPTY_TABLE")];

static RESERVED_NAMES: &[&str] = &[
    // C keywords
    "auto",
    "break",
    "case",
    "char",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extern",
    "float",
    "for",
    "goto",
    "if",
    "inline ",
    "int",
    "long",
    "register",
    "restrict ",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "struct",
    "switch",
    "typedef",
    "union",
    "unsigned",
    "void",
    "volatile",
    "while",
    // Others
    "count",
];

#[derive(Clone)]
struct TranspileStack {
    noot_scopes: Vector<RedBlackTreeMap<String, NootDef>>,
}

impl TranspileStack {
    pub fn new() -> Self {
        TranspileStack {
            noot_scopes: Vector::new().push_back(
                BUILTIN_FUNCTIONS
                    .iter()
                    .map(|&(noot_name, c_name)| {
                        (
                            noot_name.into(),
                            NootDef {
                                c_name: c_name.into(),
                                is_function: true,
                            },
                        )
                    })
                    .chain(BUILTIN_VALUES.iter().map(|&(noot_name, c_name)| {
                        (
                            noot_name.into(),
                            NootDef {
                                c_name: c_name.into(),
                                is_function: false,
                            },
                        )
                    }))
                    .collect(),
            ),
        }
    }
    pub fn with_noot_def(self, name: String, def: NootDef) -> Self {
        TranspileStack {
            noot_scopes: self
                .noot_scopes
                .set(
                    self.noot_scopes.len() - 1,
                    self.noot_scopes.last().unwrap().insert(name, def),
                )
                .unwrap(),
        }
    }
}

#[derive(Clone)]
pub struct Transpilation<'a> {
    functions: RedBlackTreeMap<String, CFunction>,
    function_stack: Vector<String>,
    pub errors: List<TranspileError<'a>>,
}

#[derive(Clone)]
struct CFunction {
    noot_name: String,
    exprs: Queue<String>,
    lines: Vector<CLine>,
    captures: Vector<CCapture>,
    indent: usize,
    max_arg: usize,
}

impl CFunction {
    pub fn new(noot_name: String) -> CFunction {
        CFunction {
            noot_name,
            exprs: Default::default(),
            lines: Default::default(),
            captures: Default::default(),
            indent: 0,
            max_arg: 0,
        }
    }
}

struct CLine {
    var_name: Option<String>,
    value: String,
    indent: usize,
    semicolon: bool,
}

struct CCapture {
    pub c_name: String,
    pub capture_name: String,
}

impl CFunction {
    pub fn with_line(self, var_name: Option<String>, value: String) -> Self {
        CFunction {
            lines: self.lines.push_back(CLine {
                var_name,
                value,
                indent: self.indent,
                semicolon: true,
            }),
            ..self
        }
    }
    pub fn with_raw_line(self, value: String) -> Self {
        CFunction {
            lines: self.lines.push_back(CLine {
                var_name: None,
                value,
                indent: self.indent,
                semicolon: false,
            }),
            ..self
        }
    }
    pub fn push_expr(self, expr: String) -> Self {
        CFunction {
            exprs: self.exprs.enqueue(expr),
            ..self
        }
    }
    pub fn pop_expr(self) -> (Self, Option<String>) {
        let expr = self.exprs.peek().cloned();
        (
            CFunction {
                exprs: self.exprs.dequeue().unwrap_or_default(),
                ..self
            },
            expr,
        )
    }
    pub fn capture_index_of(&self, c_name: &str) -> usize {
        self.captures
            .iter()
            .position(|cap| cap.c_name == c_name)
            .unwrap()
    }
    pub fn with_capture(self, c_name: String, capture_name: String) -> Self {
        if self.captures.iter().any(|cap| cap.c_name == c_name) {
            self
        } else {
            CFunction {
                captures: self.captures.push_back(CCapture {
                    c_name,
                    capture_name,
                }),
                ..self
            }
        }
    }
    pub fn indent(self) -> Self {
        CFunction {
            indent: self.indent + 1,
            ..self
        }
    }
    pub fn deindent(self) -> Self {
        CFunction {
            indent: self.indent - 1,
            ..self
        }
    }
}

pub fn transpile(items: Items) -> Transpilation {
    Transpilation::new().items(items, TranspileStack::new())
}

impl<'a> Transpilation<'a> {
    pub fn new() -> Self {
        Transpilation {
            functions: once("main")
                // .chain(BUILTINS.iter().map(|bi| bi.0))
                .map(|name| (name.into(), CFunction::new(name.into())))
                .collect(),
            function_stack: once("main".into()).collect(),
            errors: Default::default(),
        }
    }
    pub fn write(self) -> io::Result<()> {
        fs::create_dir_all("build")?;
        let mut source = File::create("build/main.c")?;

        // Write headers
        writeln!(source, "#include \"../clibs/noot.h\"")?;
        writeln!(source, "#include \"../clibs/tgc.h\"")?;
        writeln!(source)?;

        // Write function declarations
        for (name, cf) in self.functions.iter().filter(|&(name, _)| name != "main") {
            if cf.captures.is_empty() {
                writeln!(
                    source,
                    "NootValue {}(uint8_t count, NootValue* args);",
                    name
                )?;
            } else {
                writeln!(
                    source,
                    "NootValue {}(uint8_t count, NootValue* args, NootValue* captures);",
                    name
                )?;
            }
        }
        writeln!(source)?;

        // Write function definitions
        for (name, cf) in &self.functions {
            let main = name == "main";
            // Write signature
            if main {
                writeln!(source, "int main(int argc, char** argv) {{")?;
                writeln!(source, "    tgc_start(&noot_gc, &argc);")?;
            } else if cf.captures.is_empty() {
                writeln!(
                    source,
                    "NootValue {}(uint8_t count, NootValue* args) {{",
                    name
                )?;
            } else {
                writeln!(
                    source,
                    "NootValue {}(uint8_t count, NootValue* args, NootValue* captures) {{",
                    name
                )?;
            }
            // Write lines
            for line in &cf.lines {
                write!(source, "{:indent$}", "", indent = (line.indent + 1) * 4)?;
                if let Some(var_name) = &line.var_name {
                    write!(source, "NootValue {} = ", var_name)?;
                }
                writeln!(
                    source,
                    "{}{}",
                    line.value,
                    if line.semicolon { ";" } else { "" }
                )?;
            }
            // Clean up main
            if main {
                if let (_, Some(expr)) = cf.clone().pop_expr() {
                    writeln!(source, "    {};", expr)?;
                }
                writeln!(source, "    tgc_stop(&noot_gc);")?;
                writeln!(source, "    return 0;")?;
            }
            // Close function
            writeln!(source, "}}\n")?;
        }

        Ok(())
    }
    fn c_name_exists(&self, c_name: &str, function: bool) -> bool {
        RESERVED_NAMES.contains(&c_name)
            || function && self.functions.keys().any(|name| name == c_name)
            || !function
                && self
                    .functions
                    .values()
                    .flat_map(|cf| &cf.lines)
                    .filter_map(|cf| cf.var_name.as_ref())
                    .any(|var_name| var_name == c_name)
    }
    fn c_name_for(&self, noot_name: &str, function: bool) -> String {
        let mut c_name = noot_name.to_owned();
        let mut i = 1;
        while self.c_name_exists(&c_name, function) {
            i += 1;
            c_name = format!("{}_{}", noot_name, i);
        }
        c_name
    }
    fn start_c_function(self, c_name: String, noot_name: String) -> Self {
        Transpilation {
            functions: self
                .functions
                .insert(c_name.clone(), CFunction::new(noot_name)),
            function_stack: self.function_stack.push_back(c_name),
            ..self
        }
    }
    fn finish_c_function(self) -> Self {
        let result = self.map_c_function(|cf| {
            let ret_expr = cf
                .exprs
                .peek()
                .cloned()
                .unwrap_or_else(|| "NOOT_NIL".into());
            let cf = CFunction {
                exprs: cf.exprs.dequeue().unwrap_or_default(),
                ..cf
            };
            cf.with_line(None, format!("return {}", ret_expr))
        });
        Transpilation {
            function_stack: result.function_stack.drop_last().unwrap(),
            ..result
        }
    }
    fn curr_c_function(&self) -> &CFunction {
        self.functions
            .get(self.function_stack.last().unwrap())
            .unwrap()
    }
    fn map_c_function_at<F>(self, i: usize, f: F) -> Self
    where
        F: FnOnce(CFunction) -> CFunction,
    {
        let function_name = self.function_stack.get(i).unwrap();
        let cf = self.functions.get(function_name).unwrap();
        Transpilation {
            functions: self.functions.insert(function_name.clone(), f(cf.clone())),
            ..self
        }
    }
    fn map_c_function<F>(self, f: F) -> Self
    where
        F: FnOnce(CFunction) -> CFunction,
    {
        let last_index = self.function_stack.len() - 1;
        self.map_c_function_at(last_index, f)
    }
    fn push_expr(self, expr: String) -> Self {
        self.map_c_function(|cf| cf.push_expr(expr))
    }
    fn pop_expr(self) -> (Self, String) {
        let mut expr = None;
        let result = self.map_c_function(|cf| {
            let (cf, ex) = cf.pop_expr();
            expr = ex;
            cf
        });
        (result, expr.unwrap_or_else(|| "NOOT_NIL".into()))
    }
    fn error(self, error: TranspileError<'a>) -> Self {
        Transpilation {
            errors: self.errors.push_front(error),
            ..self
        }
    }
    fn items(self, items: Items<'a>, stack: TranspileStack) -> Self {
        let item_count = items.len();
        items
            .into_iter()
            .enumerate()
            .fold((self, stack), |(result, stack), (i, item)| {
                let (result, stack) = result.item(item, stack);
                let result = if i == item_count - 1 {
                    result
                } else {
                    result.map_c_function(|cf| {
                        if let Some(expr) = cf.exprs.peek().cloned() {
                            let cf = CFunction {
                                exprs: cf.exprs.dequeue().unwrap_or_default(),
                                ..cf
                            };
                            cf.with_line(None, expr)
                        } else {
                            cf
                        }
                    })
                };
                (result, stack)
            })
            .0
    }

    fn item(self, item: Item<'a>, stack: TranspileStack) -> (Self, TranspileStack) {
        match item {
            Item::Def(def) => self.def(def, stack),
            Item::Node(node) => {
                let result = self.node(node, stack.clone());
                (result, stack)
            }
        }
    }

    fn def(self, def: Def<'a>, stack: TranspileStack) -> (Self, TranspileStack) {
        let c_name = self.c_name_for(&def.ident.name, def.is_function());
        if def.is_function() {
            // Function
            let stack = stack.with_noot_def(
                def.ident.name.clone(),
                NootDef {
                    c_name: c_name.clone(),
                    is_function: true,
                },
            );
            let result =
                self.function(c_name, def.ident.name, def.params, def.items, stack.clone());
            (result, stack)
        } else {
            // Value
            let result = self.items(def.items, stack.clone());
            let result = result.map_c_function(|cf| {
                let (cf, line) = cf.pop_expr();
                if let Some(line) = line {
                    cf.with_line(Some(c_name.clone()), line)
                } else {
                    cf
                }
            });
            let stack = stack.with_noot_def(
                def.ident.name,
                NootDef {
                    c_name,
                    is_function: false,
                },
            );
            (result, stack)
        }
    }
    fn node(self, node: Node<'a>, stack: TranspileStack) -> Self {
        match node {
            Node::Term(term) => self.term(term, stack),
            Node::BinExpr(expr) => self.bin_expr(expr, stack),
            Node::UnExpr(expr) => self.un_expr(expr, stack),
            Node::Call(expr) => self.call_expr(expr, stack),
            Node::Insert(expr) => self.insert_expr(expr, stack),
            Node::Get(expr) => self.get_expr(expr, stack),
        }
    }
    fn bin_expr(self, expr: BinExpr<'a>, stack: TranspileStack) -> Self {
        let result = self.node(*expr.left, stack.clone());
        let (result, left) = result.pop_expr();
        let (f, can_fail) = match expr.op {
            BinOp::Or | BinOp::And => {
                let or = expr.op == BinOp::Or;
                let temp_name = result.c_name_for("temp", false);
                let result = result.map_c_function(|cf| {
                    cf.with_line(Some(temp_name.clone()), left)
                        .with_raw_line(format!(
                            "if ({}noot_is_true({})) {{",
                            if or { "!" } else { "" },
                            temp_name
                        ))
                        .indent()
                });
                let result = result.node(*expr.right, stack);
                let (result, right) = result.pop_expr();
                return result.map_c_function(|cf| {
                    cf.with_raw_line(format!("{} = {};", temp_name, right))
                        .deindent()
                        .with_raw_line("}".into())
                        .push_expr(temp_name)
                });
            }
            BinOp::Is => ("noot_eq", false),
            BinOp::Isnt => ("noot_neq", false),
            BinOp::Less => ("noot_lt", true),
            BinOp::LessOrEqual => ("noot_le", true),
            BinOp::Greater => ("noot_gt", true),
            BinOp::GreaterOrEqual => ("noot_ge", true),
            BinOp::Add => ("noot_add", true),
            BinOp::Sub => ("noot_sub", true),
            BinOp::Mul => ("noot_mul", true),
            BinOp::Div => ("noot_div", true),
            BinOp::Rem => ("noot_rem", true),
        };
        let result = result.node(*expr.right, stack);
        let (result, right) = result.pop_expr();
        if can_fail {
            let function_name = &result.curr_c_function().noot_name;
            let (line, col) = expr.span.split().0.line_col();
            let call_line = format!(
                "noot_call_bin_op({}, {}, {}, \"{} {}:{}\")",
                f, left, right, function_name, line, col
            );
            result.push_expr(call_line)
        } else {
            result.push_expr(format!("{}({}, {})", f, left, right))
        }
    }
    fn un_expr(self, expr: UnExpr<'a>, stack: TranspileStack) -> Self {
        let result = self.node(*expr.inner, stack);
        let (result, inner) = result.pop_expr();
        let f = match expr.op {
            UnOp::Neg => "noot_neg",
            UnOp::Not => "noot_not",
        };
        result.push_expr(format!("{}({})", f, inner))
    }
    fn call_expr(self, call: CallExpr<'a>, stack: TranspileStack) -> Self {
        let result = self.node(*call.expr, stack.clone());
        let (result, f) = result.pop_expr();
        let (result, params) =
            call.args
                .into_iter()
                .fold((result, Vector::new()), |(result, params), node| {
                    let result = result.node(node, stack.clone());
                    let (result, param) = result.pop_expr();
                    (result, params.push_back(param))
                });
        let param_count = params.len();
        let params: String = params
            .into_iter()
            .cloned()
            .intersperse(", ".into())
            .collect();
        let function_name = &result.curr_c_function().noot_name;
        let (line, col) = call.span.split().0.line_col();
        let call_line = format!(
            "noot_call({}, {}, (NootValue[]) {{ {} }}, \"{} {}:{}\")",
            f, param_count, params, function_name, line, col
        );
        result.push_expr(call_line)
    }
    fn insert_expr(self, expr: InsertExpr<'a>, stack: TranspileStack) -> Self {
        let (result, inner) = self.node(*expr.inner, stack.clone()).pop_expr();
        let (result, expr) =
            expr.insertions
                .into_iter()
                .fold((result, inner), |(result, inner), ins| {
                    let (result, key) = match ins.key {
                        Access::Index(term) => result.term(term, stack.clone()).pop_expr(),
                        Access::Field(ident) => (
                            result,
                            format!("new_string({:?}, {})", ident.name, ident.name.len()),
                        ),
                    };
                    let (result, val) = result.node(ins.val, stack.clone()).pop_expr();
                    (result, format!("noot_insert({}, {}, {})", inner, key, val))
                });
        result.push_expr(expr)
    }
    fn get_expr(self, expr: GetExpr<'a>, stack: TranspileStack) -> Self {
        let (result, inner) = self.node(*expr.inner, stack.clone()).pop_expr();
        let (result, index) = match expr.access {
            Access::Index(term) => result.term(term, stack).pop_expr(),
            Access::Field(ident) => (
                result,
                format!("new_string({:?}, {})", ident.name, ident.name.len()),
            ),
        };
        result.push_expr(format!("noot_get({}, {})", inner, index))
    }
    fn term(self, term: Term<'a>, stack: TranspileStack) -> Self {
        match term {
            Term::Nil => self.push_expr("NOOT_NIL".into()),
            Term::Bool(b) => self.push_expr(format!("new_bool({})", b as u8)),
            Term::Int(i) => self.push_expr(format!("new_int({})", i)),
            Term::Real(f) => self.push_expr(format!("new_real({})", f)),
            Term::String(s) => self.push_expr(format!("new_string({:?}, {})", s, s.len())),
            Term::Expr(items) => self.items(items, stack),
            Term::Closure(closure) => {
                let c_name = self.c_name_for("anon", true);
                let result = self.function(
                    c_name.clone(),
                    "closure".into(),
                    closure.params,
                    closure.body,
                    stack,
                );
                if result.functions.get(&c_name).unwrap().captures.is_empty() {
                    result.push_expr(format!("new_function(&{})", c_name))
                } else {
                    result.push_expr(format!("{}_closure", c_name))
                }
            }
            Term::Ident(ident) => {
                if let Some(def) = stack
                    .noot_scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.get(&ident.name))
                {
                    if let Some(ident_i) = self
                        .function_stack
                        .iter()
                        .position(|c_name| {
                            let cf = self.functions.get(c_name).unwrap();
                            cf.lines.iter().any(|line| {
                                line.var_name.as_ref().map_or(false, |vn| vn == &def.c_name)
                            })
                        })
                        .filter(|&i| self.function_stack.len() - i > 1)
                    {
                        // Captures
                        let curr_stack_i = self.function_stack.len() - 1;
                        let (result, _) = (ident_i..self.function_stack.len()).fold(
                            (self, None),
                            |(result, mut prev), stack_i| {
                                let last = curr_stack_i == stack_i;
                                let result = if last {
                                    result.map_c_function(|cf| {
                                        let cap_i = cf.capture_index_of(&def.c_name);
                                        cf.push_expr(format!("captures[{}]", cap_i))
                                    })
                                } else {
                                    result.map_c_function_at(stack_i + 1, |cf| {
                                        let cf = cf.with_capture(
                                            def.c_name.clone(),
                                            prev.clone().unwrap_or_else(|| def.c_name.clone()),
                                        );
                                        let cap_i = cf.capture_index_of(&def.c_name);
                                        prev = Some(format!("captures[{}]", cap_i));
                                        cf
                                    })
                                };
                                (result, prev)
                            },
                        );
                        result
                    } else {
                        // Non-captures
                        let is_closure = self
                            .functions
                            .get(&def.c_name)
                            .map_or(false, |cf| !cf.captures.is_empty());
                        self.push_expr(if def.is_function {
                            if is_closure {
                                format!("{}_closure", def.c_name)
                            } else {
                                format!("new_function(&{})", def.c_name)
                            }
                        } else {
                            def.c_name.clone()
                        })
                    }
                } else if let Some(&(_, c_name)) = BUILTIN_VALUES
                    .iter()
                    .find(|(noot_name, _)| noot_name == &ident.name)
                {
                    self.push_expr(c_name.into())
                } else {
                    self.error(TranspileErrorKind::UnknownDef(ident.name.clone()).span(ident.span))
                }
            }
        }
    }
    fn function(
        self,
        c_name: String,
        noot_name: String,
        params: Params<'a>,
        items: Items<'a>,
        stack: TranspileStack,
    ) -> Self {
        let result = self.start_c_function(c_name.clone(), noot_name);
        let result = result.map_c_function(|cf| {
            (0..params.len()).fold(cf, |cf, i| {
                cf.with_line(
                    Some(format!("{}_arg{}", c_name, i)),
                    format!("{i} < count ? args[{i}] : NOOT_NIL", i = i),
                )
            })
        });
        let stack = params
            .into_iter()
            .enumerate()
            .fold(stack, |stack, (i, param)| {
                stack.with_noot_def(
                    param.ident.name,
                    NootDef {
                        c_name: format!("{}_arg{}", c_name, i),
                        is_function: false,
                    },
                )
            });
        // Transpile body items and finish function
        let result = result.items(items, stack);
        let captures = result.curr_c_function().captures.clone();
        let result = result.finish_c_function();
        // Set captures in parent scope
        if captures.is_empty() {
            result
        } else {
            let captures_name = format!("{}_captures", c_name);
            let closure_name = format!("{}_closure", c_name);
            let result = result.map_c_function(|cf| {
                cf.with_raw_line(format!(
                    "NootValue* {} = (NootValue*)tgc_alloc(&noot_gc, {} * sizeof(NootValue));",
                    captures_name,
                    captures.len()
                ))
            });
            captures
                .iter()
                .enumerate()
                .fold(result, |result, (i, cap)| {
                    result.map_c_function(|cf| {
                        cf.with_raw_line(format!(
                            "{}[{}] = {};",
                            captures_name, i, cap.capture_name
                        ))
                    })
                })
                .map_c_function(|cf| {
                    cf.with_line(
                        Some(closure_name.clone()),
                        format!("new_closure(&{}, {})", c_name, captures_name),
                    )
                })
        }
    }
}
