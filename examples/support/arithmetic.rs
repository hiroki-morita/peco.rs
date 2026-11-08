use peg_combinator::{Parser, literal, one_of, recursive, satisfy};

#[derive(Debug, PartialEq)]
pub enum Expr {
    Number(i64),
    Binary {
        op: char,
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

impl Expr {
    pub fn evaluate(&self) -> Option<i64> {
        match self {
            Self::Number(value) => Some(*value),
            Self::Binary { op, left, right } => {
                let (left, right) = (left.evaluate()?, right.evaluate()?);
                match op {
                    '+' => left.checked_add(right),
                    '-' => left.checked_sub(right),
                    '*' => left.checked_mul(right),
                    _ => None,
                }
            }
        }
    }
}

fn whitespace() -> Parser<()> {
    satisfy("whitespace", |c| c.is_whitespace()).star().ignore()
}

fn fold(first: Expr, rest: Vec<(char, Expr)>) -> Expr {
    rest.into_iter()
        .fold(first, |left, (op, right)| Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        })
}

pub fn expression() -> Parser<Expr> {
    let expression = recursive("Expression", |expression| {
        let number = satisfy("ASCII digit", |c| c.is_ascii_digit())
            .plus()
            .text()
            .try_map(|s| s.parse::<i64>())
            .map(Expr::Number)
            .then_ignore(whitespace());
        let group = literal("(")
            .then_ignore(whitespace())
            .ignore_then(expression)
            .then_ignore(literal(")"))
            .then_ignore(whitespace());
        let atom = number.or(group);
        let product = atom
            .clone()
            .then(
                literal("*")
                    .then_ignore(whitespace())
                    .map(|_| '*')
                    .then(atom)
                    .star(),
            )
            .map(|(first, rest)| fold(first, rest));
        product
            .clone()
            .then(one_of("+-").then_ignore(whitespace()).then(product).star())
            .map(|(first, rest)| fold(first, rest))
    });
    whitespace().ignore_then(expression)
}
