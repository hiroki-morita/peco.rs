use peco::*;

#[path = "../examples/support/arithmetic.rs"]
mod arithmetic;

#[test]
fn assignment_returns_a_user_struct() {
    // Models a library user's record returned directly by a parser.
    #[derive(Debug, PartialEq)]
    struct Assignment {
        name: String,
        value: u64,
    }
    let digit = satisfy("digit", |c| c.is_ascii_digit());
    let name = char_range('a'..='z').plus().text();
    let value = digit.clone().plus().text().try_map(|s| s.parse::<u64>());
    let parser = name
        .then_ignore(literal("="))
        .then(value)
        .map(|(name, value)| Assignment { name, value });
    assert_eq!(
        parser.parse("count=42"),
        Ok(Assignment {
            name: "count".into(),
            value: 42
        })
    );
    assert_eq!(digit.parse("7"), Ok('7'));
}

#[test]
fn recursive_rule_returns_an_owned_enum() {
    // Models recursively nested output without borrowing the input.
    #[derive(Debug, PartialEq)]
    enum Expr {
        Number(u64),
        Group(Box<Expr>),
    }
    let number = satisfy("digit", |c| c.is_ascii_digit())
        .plus()
        .text()
        .try_map(|s| s.parse::<u64>())
        .map(Expr::Number);
    let expr = recursive("Expr", |expr| {
        literal("(")
            .ignore_then(expr)
            .then_ignore(literal(")"))
            .map(|e| Expr::Group(Box::new(e)))
            .or(number)
    });
    assert_eq!(
        expr.parse("((42))"),
        Ok(Expr::Group(Box::new(Expr::Group(Box::new(Expr::Number(
            42
        ))))))
    );
}

#[test]
fn arithmetic_precedence_associativity_and_parentheses() {
    let parser = arithmetic::expression();
    for (input, expected) in [
        ("2+3*4", 14),
        ("(2+3)*4", 20),
        ("10-3-2", 5),
        (" 10 - 2 * (3 + 1) ", 2),
    ] {
        assert_eq!(parser.parse(input).unwrap().evaluate(), Some(expected));
    }
    use arithmetic::Expr;
    assert_eq!(
        parser.parse("10-3-2").unwrap(),
        Expr::Binary {
            op: '-',
            left: Box::new(Expr::Binary {
                op: '-',
                left: Box::new(Expr::Number(10)),
                right: Box::new(Expr::Number(3))
            }),
            right: Box::new(Expr::Number(2))
        }
    );
    for invalid in ["", "1+", "(1+2", "1 2", "9223372036854775808"] {
        assert!(parser.parse(invalid).is_err(), "{invalid}");
    }
    assert_eq!(
        parser.parse("9223372036854775807+1").unwrap().evaluate(),
        None
    );
}

#[test]
fn all_basic_operations_compose_through_the_public_api() {
    // Lookahead, an optional sign, a nonterminal, class, choice and both repetitions.
    let digit = char_range('0'..='9');
    let integer = recursive("Integer", |_| {
        one_of("+-").optional().then(digit.plus()).text()
    });
    let token = and(any_char())
        .ignore_then(not(literal("!")))
        .ignore_then(integer);
    assert_eq!(
        token.clone().then_ignore(end()).parse("-42"),
        Ok("-42".into())
    );
    let line = token.or(literal("x").text());
    assert_eq!(
        line.clone()
            .then_ignore(char('\n').optional())
            .star()
            .parse("1\nx\n"),
        Ok(vec!["1".into(), "x".into()])
    );
    assert_eq!(line.parse_prefix("2!").unwrap().consumed, 1);
    assert!(line.parse("2!").is_err());
}

#[test]
fn mutual_recursion_is_available_to_library_users() {
    let mut grammar = GrammarBuilder::new();
    let a = grammar.declare::<()>("A");
    let b = grammar.declare::<()>("B");
    grammar
        .define(
            &a,
            literal("a")
                .ignore_then(grammar.reference(&b).unwrap().optional())
                .ignore(),
        )
        .unwrap();
    grammar
        .define(
            &b,
            literal("b")
                .ignore_then(grammar.reference(&a).unwrap().optional())
                .ignore(),
        )
        .unwrap();
    let root = grammar.reference(&a).unwrap();
    let parser = grammar.finish(root).unwrap();
    assert_eq!(parser.parse("ababa"), Ok(()));
}

#[test]
fn completed_grammars_compose_but_unfinished_foreign_rules_are_rejected() {
    let first = recursive("First", |_| literal("a").text());
    let second = recursive("Second", |_| literal("b").text());
    assert_eq!(first.then(second).parse("ab"), Ok(("a".into(), "b".into())));
    let mut unfinished = GrammarBuilder::new();
    let rule = unfinished.declare::<()>("Foreign");
    let raw = unfinished.reference(&rule).unwrap();
    let invalid = recursive("Invalid", |_| raw);
    assert!(matches!(
        invalid.parse(""),
        Err(ParseError::Fatal(GrammarError::ForeignRule { .. }))
    ));
}
