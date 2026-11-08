use std::{
    cell::{Cell, OnceCell, RefCell},
    rc::{Rc, Weak},
};

use crate::{GrammarError, Parser, error::Failure};

// Owns all rule definitions and controls when references may execute.
pub(crate) struct Grammar {
    // Becomes true only after every rule and dependency has been validated.
    ready: Cell<bool>,
    // Keeps heterogeneous rule cells alive for the lifetime of the grammar.
    rules: RefCell<Vec<Rc<dyn ErasedRule>>>,
}

// Records the owner and label of an unresolved nonterminal reference.
#[derive(Clone)]
pub(crate) struct Dependency {
    // Identifies the referenced grammar without creating an ownership cycle.
    pub(crate) owner: Weak<Grammar>,
    // Keeps a diagnostic label available even after the referenced cell expires.
    name: String,
}

// Allows rules with different output types to be validated uniformly.
trait ErasedRule {
    // Checks that a definition exists and its unresolved references belong here.
    fn validate(&self, owner: &Rc<Grammar>) -> Result<(), GrammarError>;
}

// Stores the typed definition that references access through weak handles.
struct RuleCell<T> {
    // Labels this definition in validation errors.
    name: String,
    // Accepts exactly one definition for this nonterminal.
    body: OnceCell<Parser<T>>,
}

impl<T: 'static> ErasedRule for RuleCell<T> {
    // Validates the typed body through the type-erased rule interface.
    fn validate(&self, owner: &Rc<Grammar>) -> Result<(), GrammarError> {
        let body = self.body.get().ok_or_else(|| GrammarError::UndefinedRule {
            rule: self.name.clone(),
        })?;
        validate_dependencies(&body.dependencies, owner)
    }
}

// Rejects unresolved references owned by a different grammar builder.
fn validate_dependencies(
    dependencies: &[Dependency],
    owner: &Rc<Grammar>,
) -> Result<(), GrammarError> {
    for dependency in dependencies {
        if !dependency.owner.ptr_eq(&Rc::downgrade(owner)) {
            return Err(GrammarError::ForeignRule {
                rule: dependency.name.clone(),
            });
        }
    }
    Ok(())
}

/// A typed, non-owning handle to a declared nonterminal.
/// Cloning a handle does not keep a grammar alive or require `T: Clone`.
pub struct Rule<T> {
    // Refers to the typed definition without retaining it.
    cell: Weak<RuleCell<T>>,
    // Identifies the owning grammar and lets execution detect expired owners.
    owner: Weak<Grammar>,
    // Keeps a diagnostic label available even after the referenced cell expires.
    name: String,
}

impl<T> Clone for Rule<T> {
    fn clone(&self) -> Self {
        Self {
            cell: self.cell.clone(),
            owner: self.owner.clone(),
            name: self.name.clone(),
        }
    }
}

/// Declares and defines mutually recursive rules before creating an owning root.
/// Unfinished references can be composed but cannot be executed successfully.
pub struct GrammarBuilder {
    // Retains all declarations until ownership transfers to a finished root.
    owner: Rc<Grammar>,
}

impl Default for GrammarBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl GrammarBuilder {
    /// Creates an empty grammar builder.
    pub fn new() -> Self {
        Self {
            owner: Rc::new(Grammar {
                ready: Cell::new(false),
                rules: RefCell::new(Vec::new()),
            }),
        }
    }

    /// Declares a rule with an output type. Names are diagnostic labels, not IDs.
    pub fn declare<T: 'static>(&mut self, name: impl Into<String>) -> Rule<T> {
        let cell = Rc::new(RuleCell {
            name: name.into(),
            body: OnceCell::new(),
        });
        let rule = Rule {
            cell: Rc::downgrade(&cell),
            owner: Rc::downgrade(&self.owner),
            name: cell.name.clone(),
        };
        self.owner.rules.borrow_mut().push(cell);
        rule
    }

    /// Creates a non-owning reference to a rule from this builder.
    pub fn reference<T: 'static>(&self, rule: &Rule<T>) -> Result<Parser<T>, GrammarError> {
        self.check(rule)?;
        let rule = rule.clone();
        let dependency = Dependency {
            owner: rule.owner.clone(),
            name: rule.name.clone(),
        };
        let mut parser = Parser::new(move |ctx, pos| {
            let owner = rule.owner.upgrade().ok_or_else(|| {
                Failure::Fatal(GrammarError::ExpiredRule {
                    rule: rule.name.clone(),
                })
            })?;
            if !owner.ready.get() {
                return Err(Failure::Fatal(GrammarError::UndefinedRule {
                    rule: rule.name.clone(),
                }));
            }
            let cell = rule.cell.upgrade().ok_or_else(|| {
                Failure::Fatal(GrammarError::ExpiredRule {
                    rule: rule.name.clone(),
                })
            })?;
            let body = cell.body.get().ok_or_else(|| {
                Failure::Fatal(GrammarError::UndefinedRule {
                    rule: rule.name.clone(),
                })
            })?;
            let key = (Rc::as_ptr(&cell) as usize, pos);
            if ctx.active.contains(&key) {
                return Err(Failure::Fatal(GrammarError::LeftRecursion {
                    rule: rule.name.clone(),
                    offset: pos,
                }));
            }
            if ctx.active.len() >= ctx.options.recursion_limit {
                return Err(Failure::Fatal(GrammarError::RecursionLimit {
                    limit: ctx.options.recursion_limit,
                    offset: pos,
                }));
            }
            ctx.active.insert(key);
            let result = body.run(ctx, pos).map_err(|error| match error {
                Failure::Mismatch(mut diagnostic) => {
                    diagnostic.context.insert(0, rule.name.clone());
                    Failure::Mismatch(diagnostic)
                }
                error => error,
            });
            ctx.active.remove(&key);
            result
        });
        Rc::make_mut(&mut parser.dependencies).push(dependency);
        Ok(parser)
    }

    /// Sets a rule's body once. References in the body are validated by `finish`.
    pub fn define<T: 'static>(
        &mut self,
        rule: &Rule<T>,
        body: Parser<T>,
    ) -> Result<(), GrammarError> {
        self.check(rule)?;
        let cell = rule
            .cell
            .upgrade()
            .ok_or_else(|| GrammarError::ExpiredRule {
                rule: rule.name.clone(),
            })?;
        cell.body
            .set(body)
            .map_err(|_| GrammarError::DuplicateDefinition {
                rule: rule.name.clone(),
            })
    }

    // Verifies that a typed handle was declared by this builder.
    fn check<T>(&self, rule: &Rule<T>) -> Result<(), GrammarError> {
        if rule.owner.ptr_eq(&Rc::downgrade(&self.owner)) {
            Ok(())
        } else {
            Err(GrammarError::ForeignRule {
                rule: rule.name.clone(),
            })
        }
    }

    /// Validates every declaration and returns a root that keeps the grammar alive.
    /// An error consumes the builder; all of its unfinished references then expire.
    pub fn finish<T: 'static>(self, root: Parser<T>) -> Result<Parser<T>, GrammarError> {
        validate_dependencies(&root.dependencies, &self.owner)?;
        for rule in self.owner.rules.borrow().iter() {
            rule.validate(&self.owner)?;
        }
        self.owner.ready.set(true);
        let mut result = root.wrap(move |root, ctx, pos| {
            let _keep_alive = &self.owner;
            root.run(ctx, pos)
        });
        result.dependencies = Rc::new(Vec::new());
        Ok(result)
    }
}

/// Defines one recursive rule by passing a non-owning self reference to `build`.
/// Left recursion is rejected at execution. Builder errors become fatal parse errors.
///
/// ```
/// use peco::{literal, recursive};
/// let nested = recursive("nested", |nested| {
///     literal("(").ignore_then(nested).then_ignore(literal(")")).or(literal("x"))
/// });
/// assert_eq!(nested.parse("((x))"), Ok(()));
/// ```
pub fn recursive<T: 'static>(
    name: impl Into<String>,
    build: impl FnOnce(Parser<T>) -> Parser<T>,
) -> Parser<T> {
    let result = (|| {
        let mut builder = GrammarBuilder::new();
        let rule = builder.declare(name);
        let reference = builder.reference(&rule)?;
        let body = build(reference.clone());
        builder.define(&rule, body)?;
        builder.finish(reference)
    })();
    match result {
        Ok(parser) => parser,
        Err(error) => Parser::new(move |_, _| Err(Failure::Fatal(error.clone()))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ParseError, literal};

    #[test]
    fn validates_definitions_and_foreign_references() {
        let mut builder = GrammarBuilder::new();
        let rule = builder.declare::<()>("A");
        let reference = builder.reference(&rule).unwrap();
        assert!(matches!(
            reference.parse(""),
            Err(ParseError::Fatal(GrammarError::UndefinedRule { .. }))
        ));
        assert!(matches!(
            builder.finish(reference),
            Err(GrammarError::UndefinedRule { .. })
        ));
        let mut builder = GrammarBuilder::new();
        let rule = builder.declare("A");
        builder.define(&rule, literal("a")).unwrap();
        assert!(matches!(
            builder.define(&rule, literal("b")),
            Err(GrammarError::DuplicateDefinition { .. })
        ));
        let mut other = GrammarBuilder::new();
        assert!(matches!(
            other.reference(&rule),
            Err(GrammarError::ForeignRule { .. })
        ));
        assert!(matches!(
            other.define(&rule, literal("")),
            Err(GrammarError::ForeignRule { .. })
        ));
        let foreign = builder.reference(&rule).unwrap().optional().ignore();
        assert!(matches!(
            other.finish(foreign),
            Err(GrammarError::ForeignRule { .. })
        ));
    }

    #[test]
    fn validates_foreign_references_inside_bodies() {
        let mut first = GrammarBuilder::new();
        let foreign = first.declare("foreign");
        first.define(&foreign, literal("a")).unwrap();
        let mut second = GrammarBuilder::new();
        let local = second.declare("local");
        second
            .define(
                &local,
                literal("")
                    .ignore_then(first.reference(&foreign).unwrap())
                    .map(|_| ()),
            )
            .unwrap();
        assert!(matches!(
            second.finish(literal("")),
            Err(GrammarError::ForeignRule { .. })
        ));
    }

    #[test]
    fn roots_keep_heterogeneous_rules_alive_without_cycles() {
        let mut builder = GrammarBuilder::new();
        let a = builder.declare("A");
        let b = builder.declare("B");
        builder.define(&a, literal("a").map(|_| 42_u32)).unwrap();
        builder.define(&b, literal("b").text()).unwrap();
        let raw = builder.reference(&a).unwrap();
        let root = raw.clone().then(builder.reference(&b).unwrap());
        let owner = Rc::downgrade(&builder.owner);
        let cell = a.cell.clone();
        let root = builder.finish(root).unwrap();
        let clone = root.clone().then_ignore(literal("!"));
        drop(root);
        assert_eq!(clone.parse("ab!"), Ok((42, "b".into())));
        drop(clone);
        assert!(owner.upgrade().is_none());
        assert!(cell.upgrade().is_none());
        assert!(matches!(
            raw.parse("a"),
            Err(ParseError::Fatal(GrammarError::ExpiredRule { .. }))
        ));
    }
}

#[cfg(test)]
mod recursion_tests {
    use super::*;
    use crate::{ParseError, ParseOptions, and, literal, not};

    #[test]
    fn nested_and_right_recursive_rules_restore_execution_state() {
        let p = recursive("nested", |p| {
            literal("(")
                .ignore_then(p)
                .then_ignore(literal(")"))
                .or(literal("x"))
        });
        for _ in 0..2 {
            assert!(p.parse("((x)").is_err());
            assert_eq!(p.parse("((x))"), Ok(()));
        }
        let right = recursive("right", |p| literal("a").ignore_then(p.optional()).ignore());
        assert_eq!(right.parse("aaaa"), Ok(()));
        // A completed call at the same offset must be removed from the active set.
        assert_eq!(and(p.clone()).ignore_then(p).parse("(x)"), Ok(()));
    }

    #[test]
    fn left_recursion_is_never_hidden() {
        let left = recursive("left", |p| p.or(literal("x")));
        let parsers = [
            left.clone(),
            left.clone().optional().ignore(),
            left.clone().star().ignore(),
            left.clone().plus().ignore(),
            and(left.clone()),
            not(left),
        ];
        for parser in parsers {
            assert!(matches!(
                parser.parse("x"),
                Err(ParseError::Fatal(GrammarError::LeftRecursion {
                    offset: 0,
                    ..
                }))
            ));
        }
        let look = recursive("look", and);
        assert!(matches!(
            look.parse(""),
            Err(ParseError::Fatal(GrammarError::LeftRecursion { .. }))
        ));
        let mut builder = GrammarBuilder::new();
        let a = builder.declare::<()>("A");
        let b = builder.declare("B");
        builder.define(&a, builder.reference(&b).unwrap()).unwrap();
        builder.define(&b, builder.reference(&a).unwrap()).unwrap();
        let root = builder.reference(&a).unwrap();
        assert!(matches!(
            builder.finish(root).unwrap().parse(""),
            Err(ParseError::Fatal(GrammarError::LeftRecursion { .. }))
        ));
    }

    #[test]
    fn mutual_recursion_and_independent_grammar_ids_work() {
        let mut builder = GrammarBuilder::new();
        let a = builder.declare("A");
        let b = builder.declare("B");
        builder
            .define(
                &a,
                literal("a")
                    .ignore_then(builder.reference(&b).unwrap().optional())
                    .ignore(),
            )
            .unwrap();
        builder
            .define(
                &b,
                literal("b")
                    .ignore_then(builder.reference(&a).unwrap().optional())
                    .ignore(),
            )
            .unwrap();
        let root = builder.reference(&a).unwrap();
        assert_eq!(builder.finish(root).unwrap().parse("abab"), Ok(()));
        let inner = recursive("same name", |_| literal("x"));
        let outer = recursive("same name", |_| inner);
        assert_eq!(outer.parse("x"), Ok(()));
    }

    #[test]
    fn recursion_limits_and_cleanup_are_per_call() {
        let p = recursive("nested", |p| {
            literal("(")
                .ignore_then(p)
                .then_ignore(literal(")"))
                .or(literal("x"))
        });
        let options = ParseOptions { recursion_limit: 2 };
        assert!(matches!(
            p.parse_with_options("((x))", options),
            Err(ParseError::Fatal(GrammarError::RecursionLimit {
                limit: 2,
                offset: 2
            }))
        ));
        assert_eq!(p.parse_with_options("(x)", options), Ok(()));
        assert!(matches!(
            p.parse_prefix_with_options("x!", ParseOptions { recursion_limit: 0 }),
            Err(ParseError::Fatal(GrammarError::RecursionLimit { .. }))
        ));
        assert_eq!(
            p.parse(&format!("{}x{}", "(".repeat(30), ")".repeat(30))),
            Ok(())
        );
        assert!(matches!(
            p.parse(&format!("{}x{}", "(".repeat(65), ")".repeat(65))),
            Err(ParseError::Fatal(GrammarError::RecursionLimit { .. }))
        ));
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::literal;

    #[test]
    fn dropping_all_roots_drops_recursive_bodies_and_action_captures() {
        let marker = Rc::new(());
        let weak = Rc::downgrade(&marker);
        let parser = recursive("Recursive", |reference| {
            let owned = marker;
            literal("a")
                .ignore_then(reference.optional())
                .map(move |_| {
                    let _keep_alive = &owned;
                })
        });
        assert_eq!(parser.parse("aaa"), Ok(()));
        let clone = parser.clone();
        drop(parser);
        assert!(weak.upgrade().is_some());
        drop(clone);
        assert!(weak.upgrade().is_none());
    }
}
